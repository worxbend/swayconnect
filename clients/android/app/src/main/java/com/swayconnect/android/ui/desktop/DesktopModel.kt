package com.swayconnect.android.ui.desktop

import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.android.ui.common.GroupPosition
import com.swayconnect.android.ui.map.MapGeometry
import com.swayconnect.android.ui.state.ViewMode
import com.swayconnect.protocol.App
import com.swayconnect.protocol.ContainerKind
import com.swayconnect.protocol.ContainerTile
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.MAX_WORKSPACE_NUMBER
import com.swayconnect.protocol.Tile
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.WindowLayout
import com.swayconnect.protocol.WindowTile
import com.swayconnect.protocol.Workspace
import com.swayconnect.protocol.replaceUnsafe
import java.text.BreakIterator

// Pure rules of the Desktop area (docs/UX.md 5). No Compose import, so the JVM tests cover them.

/**
 * Names, titles and app ids come from applications (docs/PROTOCOL.md 9.3): every one that reaches the screen
 * gets its control and format characters replaced, once, here.
 */
fun Desktop.forDisplay(): Desktop =
    copy(
        workspaces = workspaces.map { it.copy(name = replaceUnsafe(it.name)) },
        windows = windows.map { it.copy(title = it.title?.let(::displayTitle), appId = it.appId?.let(::replaceUnsafe)) },
    )

private const val REPLACEMENT = '\uFFFD'
private val REPLACEMENT_RUN = Regex("\uFFFD+")

/**
 * A window title as the screen shows it. Apps put invisible direction marks (U+200E, U+2068, U+2069) in titles;
 * the server and [replaceUnsafe] turn them into U+FFFD, which is right for safety but draws black diamonds
 * where the app drew nothing. So runs of U+FFFD at the ends are dropped and inner runs become one space. A title
 * left blank shows as no title (the app id stands in).
 */
fun displayTitle(raw: String): String? {
    val safe = replaceUnsafe(raw)
    if (REPLACEMENT !in safe) return safe
    val shown = safe.trim(REPLACEMENT).replace(REPLACEMENT_RUN, " ").trim()
    return shown.ifEmpty { null }
}

/** The two text lines of a workspace card (UX 5.4). [name] is empty when it says nothing beyond the number. */
data class CardText(val name: String, val number: String)

private const val UNNUMBERED_GRAPHEMES = 2

/** Name without its `N:` prefix, empty when it equals the number. Without a number: its first 2 graphemes. */
fun cardText(workspace: Workspace): CardText {
    val name = workspace.name
    val number = workspace.number ?: return CardText(name, firstGraphemes(name, UNNUMBERED_GRAPHEMES))
    val digits = number.toString()
    val rest = name.removePrefix("$digits:").trim()
    return CardText(if (rest == digits) "" else rest, digits)
}

/** The page header and sheet title: the card's name line, else "number". */
fun CardText.label(): String = name.ifEmpty { number }

internal fun firstGraphemes(text: String, count: Int): String {
    val breaks = BreakIterator.getCharacterInstance()
    breaks.setText(text)
    var end = 0
    repeat(count) {
        val next = breaks.next()
        if (next == BreakIterator.DONE) return text
        end = next
    }
    return text.substring(0, end)
}

/** Why a page shows the list although the user chose the map (UX 5.9). */
enum class ListReason {
    /** `layouts_cut` is true: "Layout too large to send; showing the list". */
    LayoutCut,

    /** A `null` layout without `layouts_cut` (docs/PROTOCOL.md 17.4.3: no measurable area): a neutral line. */
    NoLayout,

    /** No layout field at all (an older daemon): the toggle is off with "Needs a newer swayconnectd". */
    OldDaemon,
}

/** How one page shows its windows. [forced] is set when the map is not possible. */
data class PageMode(val showMap: Boolean, val forced: ListReason?)

/**
 * The view of one page: the user's [chosen] mode, unless the layout is missing (forced list) or the user
 * picked "Dense workspace · Show list" for this page ([listForPage]).
 */
fun pageMode(desktop: Desktop, workspace: Workspace, chosen: ViewMode, listForPage: Boolean): PageMode {
    if (workspace.layout != null) return PageMode(showMap = chosen == ViewMode.Map && !listForPage, forced = null)
    // A daemon that knows 17.4 sends `outputs`; one that does not sends no layout at all.
    val oldDaemon = desktop.outputs == null && desktop.layoutsCut == null && desktop.workspaces.all { it.layout == null }
    val reason = when {
        oldDaemon -> ListReason.OldDaemon
        desktop.layoutsCut == true -> ListReason.LayoutCut
        else -> ListReason.NoLayout
    }
    return PageMode(showMap = false, forced = reason)
}

/**
 * The windows of one page in layout tree order (tiled, then floating), then those the layout leaves out
 * (17.4.3 server rule 5) in the desktop's order.
 *
 * @param groups the tabbed or stacked container each window sits in, for its pill.
 * @param sameAppIndex `#N` for windows that share an `app_id` on the page (UX 6.3), as the map numbers them.
 * @param outsideLayout windows that the map cannot show: listed beside it.
 */
data class PageWindows(
    val ordered: List<Window>,
    val groups: Map<Long, GroupPosition>,
    val sameAppIndex: Map<Long, Int>,
    val outsideLayout: List<Window>,
)

fun pageWindows(desktop: Desktop, workspace: Workspace): PageWindows {
    val own = desktop.windows.filter { it.workspaceId == workspace.id }
    val byId = own.associateBy { it.id }
    val walk = TreeWalk(byId.keys)
    workspace.layout?.let { layout ->
        layout.tiled?.let { walk.visit(it, group = null) }
        layout.floating.forEach { walk.visit(it, group = null) }
    }
    val inTree = walk.order.mapNotNull(byId::get)
    val outside = own.filter { it.id !in walk.order }
    val ordered = inTree + outside
    return PageWindows(
        ordered = ordered,
        groups = walk.groups,
        sameAppIndex = MapGeometry.sameAppIndexes(ordered.map { it.id }, byId),
        outsideLayout = if (workspace.layout == null) emptyList() else outside,
    )
}

/** Collects window ids in tree order with their nearest tabbed or stacked parent. Ids not on the page are skipped. */
private class TreeWalk(private val known: Set<Long>) {
    val order = LinkedHashSet<Long>()
    val groups = mutableMapOf<Long, GroupPosition>()

    fun visit(tile: Tile, group: GroupPosition?) {
        when (tile) {
            is WindowTile -> {
                // 17.4.3 client rule 1 and server rule 3: skip unknown ids, keep the first of a repeated one.
                if (tile.windowId !in known || !order.add(tile.windowId)) return
                if (group != null) groups[tile.windowId] = group
            }
            is ContainerTile -> {
                val kind = when (tile.kind) {
                    ContainerKind.TABBED -> GroupKind.Tabbed
                    ContainerKind.STACKED -> GroupKind.Stacked
                    ContainerKind.SPLIT_H, ContainerKind.SPLIT_V -> null
                }
                val count = tile.children.size
                tile.children.forEachIndexed { index, child ->
                    visit(child, if (kind != null) GroupPosition(kind, index + 1, count) else group)
                }
            }
        }
    }
}

/** The window focused on the computer, when it is on [workspace]. */
fun focusedWindowOn(desktop: Desktop, workspace: Workspace): Window? =
    desktop.windows.firstOrNull { it.focused && it.workspaceId == workspace.id }

/**
 * "+ new" of the Move sheet (UX 5.8): the lowest number from 1 that no workspace has. Sway creates that workspace
 * on the move. Null when 1 to 999 are all taken.
 */
fun lowestFreeNumber(desktop: Desktop): Int? {
    val taken = desktop.workspaces.mapNotNullTo(HashSet()) { it.number }
    return (1..MAX_WORKSPACE_NUMBER).firstOrNull { it.toLong() !in taken }
}

/**
 * The layout of the container that holds [windowId] (UX 5.8 "Layout of its container"), to mark the current
 * segment. Display only (17.4.3). Null when the workspace has no layout, the window is floating or not in the
 * tiled tree, or it is the workspace's only tile (no container is sent around it).
 */
fun containerLayoutOf(workspace: Workspace, windowId: Long): WindowLayout? {
    val root = workspace.layout?.tiled as? ContainerTile ?: return null
    return parentOf(root, windowId)?.kind?.let(::windowLayoutOf)
}

private fun parentOf(container: ContainerTile, windowId: Long): ContainerTile? {
    for (child in container.children) {
        when (child) {
            is WindowTile -> if (child.windowId == windowId) return container
            is ContainerTile -> parentOf(child, windowId)?.let { return it }
        }
    }
    return null
}

private fun windowLayoutOf(kind: ContainerKind): WindowLayout =
    when (kind) {
        ContainerKind.SPLIT_H -> WindowLayout.SPLIT_H
        ContainerKind.SPLIT_V -> WindowLayout.SPLIT_V
        ContainerKind.TABBED -> WindowLayout.TABBED
        ContainerKind.STACKED -> WindowLayout.STACKING
    }

fun windowCount(desktop: Desktop, workspace: Workspace): Int = desktop.windows.count { it.workspaceId == workspace.id }

/** "last seen 12 s ago" counts seconds for the first minute, then minutes (UX 5.9). */
data class LastSeen(val amount: Long, val inMinutes: Boolean) {
    /** How long until the text changes, so the ticker wakes no more often than needed. */
    val nextTickMs: Long get() = if (inMinutes) MINUTE_MS else SECOND_MS

    companion object {
        private const val SECOND_MS = 1_000L
        private const val MINUTE_MS = 60_000L

        /** A clock that went backwards reads as "0 s". */
        fun of(nowMs: Long, seenAtMs: Long): LastSeen {
            val ago = (nowMs - seenAtMs).coerceAtLeast(0)
            return if (ago < MINUTE_MS) LastSeen(ago / SECOND_MS, inMinutes = false) else LastSeen(ago / MINUTE_MS, inMinutes = true)
        }
    }
}

/** Above this font scale the cards drop their name line and buttons go icon-only (UX 15). */
const val LARGE_FONT_SCALE = 1.5f

private const val DESKTOP_SUFFIX = ".desktop"

/**
 * The desktop entry names by entry id, lower-cased and without `.desktop`, made safe; blank names are left out.
 * Built once per `apps` list, so a lookup per tile is a map read. The first entry of an id wins.
 */
fun appNames(apps: List<App>): Map<String, String> = buildMap {
    for (app in apps) {
        val id = app.entryId.removeSuffix(DESKTOP_SUFFIX).lowercase()
        val name = replaceUnsafe(app.name)
        if (name.isNotBlank() && id !in this) put(id, name)
    }
}

/**
 * The name a window's app shows under its title: the name of the desktop entry whose id equals [appId] ignoring
 * case (`firefox` → "Firefox"), else the entry named by its last reverse-DNS segment (`com.vscodium.codium` →
 * `codium.desktop`). [appId] itself when neither matches.
 */
fun appNameFor(appId: String, names: Map<String, String>): String {
    val id = appId.lowercase()
    return names[id] ?: names[id.substringAfterLast('.')] ?: appId
}

/** The parts of the focused-window row (UX 5.7) in dp: Full, Float and Move with labels, a divider, Close. */
object FocusedRow {
    const val GAP = 8f
    const val DIVIDER = 1f

    /** The icon-only Close: a 48dp circle, as the hold button and as the locked or read-only gated button. */
    const val CLOSE = 48f

    /** Side padding of a labelled button: 10dp, so Full, Float and Move keep their labels beside Close on a 411dp phone. */
    const val SIDE_PADDING = 10f
    const val ICON = 18f
    const val ICON_GAP = 8f

    /**
     * Each part is laid out in whole pixels (at 420dpi 12dp is 31.5px and rounds to 32, the 1dp divider takes
     * 3px), so the row can run about 1dp past its dp sum; a label short by one pixel wraps and is cut.
     */
    const val PIXEL_SLACK = 2f

    /**
     * True when the labelled buttons (their measured [labelWidths]), the divider and the icon-only Close fit in
     * [available] with [PIXEL_SLACK] to spare. Otherwise all buttons go icon-only, so the row never wraps.
     */
    fun labelsFit(available: Float, labelWidths: List<Float>): Boolean {
        val buttons = labelWidths.sumOf { (2 * SIDE_PADDING + ICON + ICON_GAP + it).toDouble() }.toFloat()
        val gaps = GAP * (labelWidths.size + 1)
        return buttons + gaps + DIVIDER + CLOSE + PIXEL_SLACK <= available
    }
}

/**
 * The keep-in-view scroll of the card row (UX 17.3), in px: 0 while the card lies inside [start, end], else the
 * least scroll that brings its near edge back inside. A card wider than the span lines up on [start].
 */
fun keepInViewDelta(offset: Int, size: Int, start: Int, end: Int): Int =
    when {
        offset < start -> offset - start
        offset + size > end -> minOf(offset + size - end, offset - start)
        else -> 0
    }

/**
 * The scroll (px, negative) that shows the row's first card whole when the start edge cuts it ([firstOffset] < 0)
 * and the target card's end ([targetEnd]) still fits inside [end] after it; else 0.
 */
fun revealFirstDelta(firstOffset: Int, targetEnd: Int, end: Int): Int =
    if (firstOffset < 0 && targetEnd - firstOffset <= end) firstOffset else 0
