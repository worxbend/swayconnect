package com.swayconnect.android.ui.desktop

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.common.WindowCard
import com.swayconnect.android.ui.launch.GhostTile
import com.swayconnect.android.ui.map.MapActions
import com.swayconnect.android.ui.map.MapSlab
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PendingLaunch
import com.swayconnect.android.ui.state.ViewMode
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace

/** M2b op behind the "open an app" card. Same name as the shell's round button. */
private const val APP_LAUNCH_OP = "app.launch"

/** What a page can ask for. Every action goes to the view model; the page renders state only. */
internal class PageActions(
    val onFocusWindow: (Long) -> Unit,
    val onWindowSheet: (Long) -> Unit,
    val onFullscreen: (Window) -> Unit,
    val onSetView: (ViewMode) -> Unit,
    val onShowOnComputer: (Long) -> Unit,
    val onFollowChip: () -> Unit,
    /** Hides "Larger text · Show list" for the rest of this run (UX 15: suggested once). */
    val onDismissListHint: () -> Unit,
    val onOpenGroup: (workspaceId: Long, windowIds: List<Long>) -> Unit,
    val onSmallWindows: (workspaceId: Long, windowIds: List<Long>) -> Unit,
    val onOpenLaunch: () -> Unit,
    val onLocked: (String) -> Unit,
    val window: WindowOps,
)

/** The facts every page shares. [live] false: the desktop is stale, taps are off (UX 5.9). */
internal class PageFacts(
    val viewMode: ViewMode,
    val browse: Boolean,
    val live: Boolean,
    val pendingId: Long?,
    val granted: Granted?,
    /** "Computer moved to 7 →": the label of the computer's workspace, or null. */
    val movedTo: String?,
    /** One-line state texts of the whole desktop (read-only, screens off, refusals, ...). */
    val lines: List<String>,
    val listHintDismissed: Boolean,
) {
    fun availabilityOf(op: String): OpAvailability? = granted?.availabilityOf(op)
}

/**
 * One workspace page (docs/UX.md 5.1, 5.2, 17.5): a single `LazyColumn`, the only vertical scroll owner of
 * the page. Map view: header with its lines, chips, map slab, windows the map leaves out, focused-window card.
 * List view: header, window cards in tree order (the focused one carries its buttons), the "open an app" card.
 * [ghost]: an app started for this workspace that has no window yet (UX 8.2), under the header.
 */
@Composable
internal fun WorkspacePage(
    desktop: Desktop,
    workspace: Workspace,
    facts: PageFacts,
    actions: PageActions,
    contentPadding: PaddingValues,
    modifier: Modifier = Modifier,
    ghost: PendingLaunch? = null,
) {
    // "Dense workspace · Show list" switches this page only and is not stored (UX 5.9).
    var listForPage by rememberSaveable(workspace.id) { mutableStateOf(false) }
    val mode = pageMode(desktop, workspace, facts.viewMode, listForPage)
    val windows = remember(desktop, workspace) { pageWindows(desktop, workspace) }
    val label = cardText(workspace).label()
    val focused = focusedWindowOn(desktop, workspace)
    // A focused window that is already one of the cards gets its buttons there, not a second card (UX 5.7).
    val carded = if (mode.showMap) windows.outsideLayout else windows.ordered
    val cards = PageCards(windows, focusedId = focused?.id?.takeIf { id -> carded.any { it.id == id } })
    val chips = hasChips(workspace, mode, facts)
    LazyColumn(modifier = modifier.fillMaxSize(), contentPadding = contentPadding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item(key = "header") {
            PageHeader(label, windows.ordered.size, workspace, mode, facts) { view ->
                listForPage = false
                actions.onSetView(view)
            }
        }
        if (chips) item(key = "chips") { PageChips(workspace, mode, facts, actions) }
        if (ghost != null) item(key = "ghost") { GhostTile(ghost) }
        if (mode.showMap) {
            mapItems(desktop, workspace, cards, label, facts, actions) { listForPage = true }
        } else {
            windowItems(windows.ordered, cards, facts, actions)
        }
        if (windows.ordered.isEmpty() || !mode.showMap) {
            item(key = "open-app") { OpenAppCard(label, empty = windows.ordered.isEmpty(), facts, actions) }
        }
        if (cards.focusedId == null) item(key = "focused") { FocusedWindowCard(focused, windows, facts, actions) }
    }
}

/** The window cards of a page and the focused window among them, which carries the focused-window buttons. */
private class PageCards(val windows: PageWindows, val focusedId: Long?)

private fun LazyListScope.mapItems(
    desktop: Desktop,
    workspace: Workspace,
    cards: PageCards,
    label: String,
    facts: PageFacts,
    actions: PageActions,
    onShowList: () -> Unit,
) {
    val layout = workspace.layout ?: return
    item(key = "map") {
        MapSlab(
            layout = layout,
            windows = desktop.windows.filter { it.workspaceId == workspace.id },
            workspaceLabel = label,
            actions = MapActions(
                onFocus = { id -> tapWindow(id, facts, actions) },
                onLongPress = actions.onWindowSheet,
                onOpenGroup = { ids -> actions.onOpenGroup(workspace.id, ids) },
                onShowSmallWindows = { ids -> actions.onSmallWindows(workspace.id, ids) },
                onShowList = onShowList,
            ),
            pendingWindowId = facts.pendingId,
            enabled = facts.live,
        )
    }
    // 17.4.3 server rule 5: a window the layout leaves out is listed beside the map.
    val outside = cards.windows.outsideLayout
    if (outside.isNotEmpty()) {
        item(key = "outside") { Text(stringResource(R.string.desktop_outside_map), style = MaterialTheme.typography.labelLarge) }
        windowItems(outside, cards, facts, actions)
    }
}

private fun LazyListScope.windowItems(list: List<Window>, cards: PageCards, facts: PageFacts, actions: PageActions) {
    items(list, key = { "window:${it.id}" }) { window ->
        val pending = facts.pendingId == window.id
        WindowCard(
            window = window,
            onClick = { tapWindow(window.id, facts, actions) },
            onClickLabel = stringResource(R.string.desktop_focus_window),
            onLongClick = { actions.onWindowSheet(window.id) },
            enabled = facts.live,
            pending = pending,
            group = cards.windows.groups[window.id],
            sameAppIndex = cards.windows.sameAppIndex[window.id],
            modifier = Modifier.animateItem(),
            actions = if (window.id == cards.focusedId) ({ FocusedButtons(window, enabled = facts.live && !pending, facts, actions) }) else null,
        )
    }
}

/** A tap on a tile or card focuses the window (UX 6.10); a locked op explains itself instead. */
internal fun tapWindow(windowId: Long, facts: PageFacts, actions: PageActions) {
    when (val availability = facts.availabilityOf(Op.WINDOW_FOCUS)) {
        OpAvailability.Available -> actions.onFocusWindow(windowId)
        is OpAvailability.Locked -> actions.onLocked(availability.scope)
        else -> Unit
    }
}

/**
 * `<name> · <count>`, "On screen" and the map / list toggle (UX 5.6), with the page's one-line states tight
 * under it: why the list is forced, read-only, screens off, the last refusal.
 */
@Composable
private fun PageHeader(label: String, count: Int, workspace: Workspace, mode: PageMode, facts: PageFacts, onSetView: (ViewMode) -> Unit) {
    val countText = if (count == 0) stringResource(R.string.desktop_count_empty) else pluralStringResource(R.plurals.desktop_window_count, count, count)
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth().heightIn(min = 40.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = stringResource(R.string.desktop_header, label, countText),
                style = MaterialTheme.typography.titleMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).semantics { heading() },
            )
            if (showsBrowseMark(facts) && workspace.focused) StatePill(stringResource(R.string.desktop_on_screen), PillTone.Neutral)
            val shown = if (mode.showMap) ViewMode.Map else ViewMode.List
            ViewToggle(shown, enabled = mode.forced == null, onSetView)
        }
        pageLines(mode, facts).forEach { StateLine(it) }
    }
}

@Composable
private fun pageLines(mode: PageMode, facts: PageFacts): List<String> =
    listOfNotNull(
        when (mode.forced) {
            ListReason.OldDaemon -> stringResource(R.string.desktop_view_needs_newer)
            ListReason.LayoutCut -> stringResource(R.string.desktop_layout_cut)
            ListReason.NoLayout -> stringResource(R.string.desktop_no_layout)
            null -> null
        },
    ) + facts.lines

/** Browse mode with `workspace.focus` known to the server: the header says "On screen" or offers to show it. */
private fun showsBrowseMark(facts: PageFacts): Boolean =
    facts.browse && facts.availabilityOf(Op.WORKSPACE_FOCUS).let { it != null && it != OpAvailability.Unsupported }

@Composable
private fun ViewToggle(shown: ViewMode, enabled: Boolean, onSetView: (ViewMode) -> Unit) {
    val haptics = LocalHaptics.current
    Row(
        modifier = Modifier.clip(CircleShape).background(MaterialTheme.colorScheme.surface).selectableGroup(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for ((view, icon, spoken) in listOf(
            Triple(ViewMode.Map, SwayIcons.Map, R.string.desktop_view_map),
            Triple(ViewMode.List, SwayIcons.List, R.string.desktop_view_list),
        )) {
            val selected = view == shown
            ToggleSegment(selected, enabled, if (selected) icon.filled else icon.outline, stringResource(spoken)) {
                if (!selected) {
                    haptics.perform(SwayHaptic.SegmentTick)
                    onSetView(view)
                }
            }
        }
    }
}

@Composable
private fun ToggleSegment(selected: Boolean, enabled: Boolean, icon: ImageVector, spoken: String, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val tint = when {
        !enabled -> scheme.onSurfaceVariant.copy(alpha = DISABLED_ALPHA)
        selected -> scheme.onPrimary
        else -> scheme.onSurfaceVariant
    }
    Box(
        modifier = Modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(if (selected && enabled) scheme.primary else scheme.surface)
            .selectable(selected = selected, enabled = enabled, role = Role.Tab, onClick = onClick)
            .semantics { contentDescription = spoken },
        contentAlignment = Alignment.Center,
    ) {
        Icon(imageVector = icon, contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
    }
}

private const val DISABLED_ALPHA = 0.38f

/** Above font scale 1.8 the map suggests the list once (UX 15). */
private const val LIST_HINT_FONT_SCALE = 1.8f

@Composable
private fun showsListHint(mode: PageMode, facts: PageFacts): Boolean =
    mode.showMap && !facts.listHintDismissed && LocalDensity.current.fontScale > LIST_HINT_FONT_SCALE

@Composable
private fun hasChips(workspace: Workspace, mode: PageMode, facts: PageFacts): Boolean =
    (showsBrowseMark(facts) && !workspace.focused) || facts.movedTo != null || showsListHint(mode, facts)

/** "Show on computer" in Browse mode, "Computer moved to 7 →", "Larger text · Show list" (UX 5.6, 15). */
@Composable
private fun PageChips(workspace: Workspace, mode: PageMode, facts: PageFacts, actions: PageActions) {
    val listHint = showsListHint(mode, facts)
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        if (showsBrowseMark(facts) && !workspace.focused) {
            GatedButton(
                availability = facts.availabilityOf(Op.WORKSPACE_FOCUS),
                icon = SwayIcons.Desktop.outline,
                label = stringResource(R.string.desktop_show_on_computer),
                iconOnly = false,
                enabled = facts.live && facts.pendingId != workspace.id,
                onLocked = actions.onLocked,
                onClick = { actions.onShowOnComputer(workspace.id) },
            )
        }
        facts.movedTo?.let { target ->
            val spoken = stringResource(R.string.desktop_moved_spoken, target)
            AssistChip(
                onClick = actions.onFollowChip,
                label = { Text(stringResource(R.string.desktop_moved, target)) },
                border = outlinedChipBorder(enabled = true),
                modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = spoken },
            )
        }
        if (listHint) ListHint(actions)
    }
}

/** The once-only suggestion of the list at a very large font scale; its close button dismisses it for this run. */
@Composable
private fun ListHint(actions: PageActions) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        AssistChip(
            onClick = {
                actions.onDismissListHint()
                actions.onSetView(ViewMode.List)
            },
            label = { Text(stringResource(R.string.desktop_list_hint)) },
            border = outlinedChipBorder(enabled = true),
            modifier = Modifier.heightIn(min = 48.dp),
        )
        IconButton(onClick = actions.onDismissListHint) {
            Icon(SwayIcons.Close.outline, contentDescription = stringResource(R.string.desktop_list_hint_dismiss), modifier = Modifier.size(18.dp))
        }
    }
}

/** UX 15: an outline that reaches 3:1 on the background, not the default `outlineVariant` (1.3:1 on greige). */
@Composable
internal fun outlinedChipBorder(enabled: Boolean): BorderStroke =
    AssistChipDefaults.assistChipBorder(enabled = enabled, borderColor = MaterialTheme.colorScheme.outline)

@Composable
internal fun StateLine(text: String, modifier: Modifier = Modifier) {
    Text(text = text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

/** "+ Open an app here" (empty page) or "+ Open an app on 3" (list view): Launch aimed at this workspace (M2b). */
@Composable
private fun OpenAppCard(label: String, empty: Boolean, facts: PageFacts, actions: PageActions) {
    val availability = facts.availabilityOf(APP_LAUNCH_OP) ?: return
    if (availability == OpAvailability.Unsupported) return
    val text = if (empty) stringResource(R.string.desktop_open_app_here) else stringResource(R.string.desktop_open_app_on, label)
    val onClick: (() -> Unit)? = when {
        !facts.live -> null
        availability == OpAvailability.Available -> actions.onOpenLaunch
        availability is OpAvailability.Locked -> ({ actions.onLocked(availability.scope) })
        else -> null
    }
    val icon = if (availability is OpAvailability.Locked) SwayIcons.Lock.outline else SwayIcons.Plus.outline
    DashedCard(text = text, icon = icon, onClick = onClick)
}

/**
 * The focused-window card (UX 5.7): [window] is the window focused on the computer when it is on this page, with
 * its buttons; else (null) the hint "Tap a window to focus it".
 */
@Composable
private fun FocusedWindowCard(window: Window?, windows: PageWindows, facts: PageFacts, actions: PageActions) {
    if (window == null) {
        DashedCard(text = stringResource(R.string.desktop_tap_to_focus))
        return
    }
    val pending = facts.pendingId == window.id
    WindowCard(
        window = window,
        onClick = { tapWindow(window.id, facts, actions) },
        onClickLabel = stringResource(R.string.desktop_focus_window),
        onLongClick = { actions.onWindowSheet(window.id) },
        enabled = facts.live,
        pending = pending,
        group = windows.groups[window.id],
        sameAppIndex = windows.sameAppIndex[window.id],
    ) {
        FocusedButtons(window, enabled = facts.live && !pending, facts, actions)
    }
}

@Composable
internal fun FullscreenButton(window: Window, iconOnly: Boolean, enabled: Boolean, facts: PageFacts, actions: PageActions) {
    GatedButton(
        availability = facts.availabilityOf(Op.WINDOW_FULLSCREEN),
        icon = SwayIcons.Fullscreen.outline,
        label = stringResource(if (window.fullscreen) R.string.desktop_exit_fullscreen else R.string.desktop_fullscreen),
        spoken = stringResource(if (window.fullscreen) R.string.desktop_exit_fullscreen_spoken else R.string.desktop_fullscreen_spoken),
        iconOnly = iconOnly,
        enabled = enabled,
        onLocked = actions.onLocked,
        onClick = { actions.onFullscreen(window) },
    )
}

/**
 * A 48dp button that follows `FeatureAvailability` (docs/PROTOCOL.md 17.3, UX 5.7): available acts; read-only
 * and "no connection" (null) show it disabled; locked shows a lock and opens the Locked sheet; unsupported
 * hides it.
 */
@Composable
internal fun GatedButton(
    availability: OpAvailability?,
    icon: ImageVector,
    label: String,
    iconOnly: Boolean,
    enabled: Boolean,
    onLocked: (String) -> Unit,
    onClick: () -> Unit,
    spoken: String = label,
) {
    if (availability == OpAvailability.Unsupported) return
    val locked = availability as? OpAvailability.Locked
    val lockedSpoken = stringResource(R.string.desktop_action_locked, spoken)
    val active = enabled && (availability == OpAvailability.Available || locked != null)
    FilledTonalButton(
        onClick = { if (locked != null) onLocked(locked.scope) else onClick() },
        enabled = active,
        // The tonal fill alone is 1.1:1 on greige; the outline keeps the edge at 3:1 (UX 15).
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = if (active) 1f else DISABLED_ALPHA)),
        contentPadding = PaddingValues(horizontal = if (iconOnly) 12.dp else 16.dp),
        modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = if (locked != null) lockedSpoken else spoken },
    ) {
        Icon(imageVector = if (locked != null) SwayIcons.Lock.outline else icon, contentDescription = null, modifier = Modifier.size(18.dp))
        if (!iconOnly) Text(label, modifier = Modifier.padding(start = 8.dp), maxLines = 1)
    }
}
