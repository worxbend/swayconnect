package com.swayconnect.android.ui.map

import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.protocol.ContainerKind
import com.swayconnect.protocol.ContainerTile
import com.swayconnect.protocol.Layout
import com.swayconnect.protocol.Tile
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.WindowTile
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min

// The mini-map rules of docs/UX.md 6, as pure Kotlin (no Compose). Every length is in dp.

/** A rectangle in dp, relative to the top left of the slab's inner box. */
data class MapRect(val left: Float, val top: Float, val width: Float, val height: Float) {
    val right: Float get() = left + width
    val bottom: Float get() = top + height
    val area: Float get() = width * height

    fun contains(x: Float, y: Float): Boolean = x >= left && x < right && y >= top && y < bottom

    /** 0 inside, else the distance from the point to the nearest edge. */
    fun edgeDistance(x: Float, y: Float): Float {
        val dx = max(max(left - x, 0f), x - right)
        val dy = max(max(top - y, 0f), y - bottom)
        return hypot(dx, dy)
    }

    fun inset(by: Float): MapRect =
        MapRect(left + by, top + by, max(width - 2 * by, 0f), max(height - 2 * by, 0f))
}

/** UX 6.2, chosen on the inset rect. */
enum class SizeClass {
    /** ≥ 120 × 80 and tall enough for one title line at the current font scale: icon, title, app id. */
    Large,

    /** ≥ 48 × 48: a centred icon and the same-app badge. Its own touch target. */
    Medium,

    /** 24 to 48 in its smaller side: a small icon; taps go through the shared hit rule. */
    Small,

    /** Under 24 in either side: a wash sliver, not tappable, counted in "+N small windows". */
    Sliver,
}

enum class TileLayer { Tiled, Floating, Fullscreen }

/** Where a tile sits on the map, for its spoken description. */
enum class MapPosition { Whole, TopLeft, Top, TopRight, Left, Centre, Right, BottomLeft, Bottom, BottomRight }

/**
 * One window drawn on the map.
 *
 * @param treeOrder the window's index in layout tree order (tiled, then floating), for traversal.
 * @param dimmed behind a fullscreen window: drawn at 30 % alpha, not tappable.
 * @param showsTitleFragment a same-app tile under the large class that still has room for 12 title characters.
 */
data class PlacedTile(
    val windowId: Long,
    val rect: MapRect,
    val sizeClass: SizeClass,
    val layer: TileLayer,
    val treeOrder: Int,
    val position: MapPosition,
    val dimmed: Boolean,
    val showsTitleFragment: Boolean,
) {
    val tappable: Boolean get() = !dimmed && sizeClass != SizeClass.Sliver
}

/**
 * The header strip of a tabbed or stacked container (UX 6.5). The shown child's windows are separate tiles
 * placed in the body under the strip.
 *
 * @param tabs the window ids of each child, in tree order.
 * @param shown the index of the child shown.
 */
data class PlacedGroup(
    val header: MapRect,
    val kind: GroupKind,
    val tabs: List<List<Long>>,
    val shown: Int,
    val layer: TileLayer,
    val treeOrder: Int,
    val dimmed: Boolean,
) {
    val windowCount: Int get() = tabs.sumOf { it.size }

    /** UX 6.5: a tab segment at least 48dp wide focuses its child; any other strip opens the Tab group sheet. */
    val segmentsTappable: Boolean get() = kind == GroupKind.Tabbed && header.width / tabs.size >= MapGeometry.OWN_TARGET

    /** The stacked rows drawn, at most 4; the rest is the "+N" label. */
    val stackedRowsShown: Int get() = min(tabs.size, MapGeometry.MAX_STACKED_ROWS)
}

/** What the root of the tiled tree is, for the map's spoken summary. */
enum class MapKind { SplitH, SplitV, Tabbed, Stacked, OneWindow, FloatingOnly, Empty }

/** The result of [MapGeometry.place]: everything the map draws, in draw order (bottom first). */
data class MapPlan(
    val map: MapRect,
    val tiles: List<PlacedTile>,
    val groups: List<PlacedGroup>,
    val kind: MapKind,
    /** The same-app index of each window that shares its `app_id` with another window of the layout (UX 6.3). */
    val sameAppIndex: Map<Long, Int>,
    val fullscreenWindowId: Long?,
    /** Share of the floating windows' union in the map area, 0 to 1 (UX 6.6), counted even while they are hidden. */
    val floatingCoverage: Float,
    val floatingCount: Int,
    /** Every window of the layout, hidden tab children included. */
    val windowCount: Int,
) {
    /** Windows drawn as slivers: the "+N small windows" chip lists them. */
    val smallWindowIds: List<Long> = tiles.filter { it.sizeClass == SizeClass.Sliver && !it.dimmed }.map { it.windowId }

    /** Share of tiles under 24dp (UX 5.9 dense workspace). */
    val denseRatio: Float = if (tiles.isEmpty()) 0f else tiles.count { it.sizeClass == SizeClass.Sliver }.toFloat() / tiles.size

    /** UX 5.9: more than 30 % slivers, and at least 3 of them, so one thin window among three is no alarm. */
    val isDense: Boolean get() = denseRatio > MapGeometry.DENSE_RATIO && smallWindowIds.size >= MapGeometry.DENSE_MIN_SLIVERS

    val showsFloatingChip: Boolean get() = floatingCoverage > MapGeometry.FLOATING_CHIP_COVERAGE

    /**
     * The hit rule of UX 6.2: the topmost tile that contains the point, else the tappable tile whose nearest edge is
     * closest within [radius]. Never the nearest centre. A point on a sliver or on a dimmed tile hits nothing.
     */
    fun hit(x: Float, y: Float, radius: Float = MapGeometry.HIT_RADIUS): PlacedTile? {
        val topFirst = tiles.asReversed()
        val container = topFirst.firstOrNull { it.rect.contains(x, y) }
        if (container != null) return container.takeIf { it.tappable }
        var best: PlacedTile? = null
        var bestDistance = radius
        for (tile in topFirst) {
            if (!tile.tappable) continue
            val distance = tile.rect.edgeDistance(x, y)
            if (distance <= bestDistance && (best == null || distance < bestDistance)) {
                best = tile
                bestDistance = distance
            }
        }
        return best
    }
}

object MapGeometry {
    const val BASIS = 10_000f
    const val INSET = 2f
    const val OWN_TARGET = 48f
    const val MIN_TARGET = 24f
    const val HIT_RADIUS = 24f
    const val LARGE_MIN_WIDTH = 120f
    const val LARGE_MIN_HEIGHT = 80f
    const val FRAGMENT_MIN_WIDTH = 120f
    const val FRAGMENT_MIN_HEIGHT = 56f
    const val TAB_HEADER = 20f
    const val STACKED_ROW = 8f
    const val MAX_STACKED_ROWS = 4
    const val DENSE_RATIO = 0.3f
    const val DENSE_MIN_SLIVERS = 3
    const val FLOATING_CHIP_COVERAGE = 0.6f
    const val ANIMATED_TILE_LIMIT = 25

    // A large tile: 8dp padding around a row of the 24dp icon and one labelLarge (20sp) plus one labelSmall (16sp) line.
    private const val LARGE_PADDING = 16f

    // Beside the title: the 32dp icon disc and the 8dp gap of the large tile's row.
    private const val LARGE_ICON_SPAN = 40f
    private const val LARGE_TEXT_LINES_SP = 36f
    private const val FULL_SPAN = 0.9f
    private const val SIDE_LOW = 0.4f
    private const val SIDE_HIGH = 0.6f
    private const val COVERAGE_GRID = 64

    /**
     * Letterboxes [layout] into the slab's inner box of [innerWidth] × [innerHeight] dp (UX 6.1) and places every
     * window. [windows] are the desktop's windows: a tile whose window is not among them is skipped. The title
     * line height scales with [fontScale]; [titleWidth] is the measured width (dp) of a short title at the current
     * font scale (UX 6.2), so a large text falls to icon-only sooner. With [showFloating] false the floating layer
     * is left out (the "Floating · N" chip hid it).
     */
    fun place(
        layout: Layout,
        windows: List<Window>,
        innerWidth: Float,
        innerHeight: Float,
        fontScale: Float,
        showFloating: Boolean = true,
        titleWidth: Float = 0f,
    ): MapPlan {
        val map = letterbox(layout.size, innerWidth, innerHeight)
        val byId = windows.associateBy { it.id }
        val placer = Placer(byId, TextFit(fontScale, titleWidth), map)
        layout.tiled?.let { placer.place(it, Frame.root(map), TileLayer.Tiled) }
        val tiledEnd = placer.tiles.size
        layout.floating.forEach { placer.place(it, Frame.root(map), TileLayer.Floating) }
        val floating = placer.tiles.subList(tiledEnd, placer.tiles.size).toList()
        val coverage = coverageOf(floating.map { it.rect }, map)
        val fullscreenId = layout.fullscreen?.takeIf { it in byId }
        var tiles: List<PlacedTile> = placer.tiles
        var groups: List<PlacedGroup> = placer.groups
        if (!showFloating) {
            tiles = tiles.filter { it.layer != TileLayer.Floating }
            groups = groups.filter { it.layer != TileLayer.Floating }
        }
        if (fullscreenId != null) {
            val order = placer.order[fullscreenId] ?: placer.order.size
            tiles = tiles.filter { it.windowId != fullscreenId }.map { it.copy(dimmed = true) } +
                placer.tile(fullscreenId, map, TileLayer.Fullscreen, order)
            groups = groups.map { it.copy(dimmed = true) }
        }
        return MapPlan(
            map = map,
            tiles = tiles,
            groups = groups,
            kind = kindOf(layout),
            sameAppIndex = sameAppIndexes(placer.order.keys.toList(), byId),
            fullscreenWindowId = fullscreenId,
            floatingCoverage = coverage,
            floatingCount = floating.size,
            windowCount = placer.order.size,
        )
    }

    /** UX 6.1 rule 2: the map keeps the aspect of `size` and is centred in the inner box. */
    fun letterbox(size: List<Long>, innerWidth: Float, innerHeight: Float): MapRect {
        val w = size.getOrNull(0)?.toFloat() ?: 0f
        val h = size.getOrNull(1)?.toFloat() ?: 0f
        if (w <= 0f || h <= 0f || innerWidth <= 0f || innerHeight <= 0f) {
            return MapRect(0f, 0f, max(innerWidth, 0f), max(innerHeight, 0f))
        }
        val mapW = min(innerWidth, innerHeight * w / h)
        val mapH = mapW * h / w
        return MapRect((innerWidth - mapW) / 2, (innerHeight - mapH) / 2, mapW, mapH)
    }

    /**
     * UX 6.2 on the inset rect. [fontScale] raises the large class's minimum height; [titleWidth] (dp) its
     * minimum width, so a tile that would cut its title to a stub shows the centred icon instead.
     */
    fun sizeClassOf(rect: MapRect, fontScale: Float, titleWidth: Float = 0f): SizeClass {
        val small = min(rect.width, rect.height)
        val largeHeight = max(LARGE_MIN_HEIGHT, LARGE_PADDING + LARGE_TEXT_LINES_SP * fontScale)
        val largeWidth = max(LARGE_MIN_WIDTH, LARGE_PADDING + LARGE_ICON_SPAN + titleWidth)
        return when {
            small < MIN_TARGET -> SizeClass.Sliver
            small < OWN_TARGET -> SizeClass.Small
            rect.width >= largeWidth && rect.height >= largeHeight -> SizeClass.Large
            else -> SizeClass.Medium
        }
    }

    /**
     * UX 6.3: the `#N` badge of a medium tile shows when its measured text plus 12dp of padding fits the tile.
     * Narrower tiles keep the index in their spoken label.
     */
    fun indexBadgeFits(tileWidth: Float, badgeTextWidth: Float): Boolean = tileWidth >= badgeTextWidth + INDEX_BADGE_PADDING

    private const val INDEX_BADGE_PADDING = 12f

    /**
     * UX 6.2: the title lines of a titled medium tile. Compose splits a word wider than the line inside it
     * ("Downloa" / "ds"), so a title whose widest word ([longestWordWidth]) does not fit [lineWidth] gets one line
     * and an ellipsis; else two.
     */
    fun mediumTitleLines(longestWordWidth: Float, lineWidth: Float): Int = if (longestWordWidth <= lineWidth) 2 else 1

    /** Breaks at spaces, and after a slash or a hyphen, which keep their part. */
    private val WORD_BREAK = Regex("""\s+|(?<=[/-])""")

    /**
     * The pieces of a title that a line can never split without breaking inside a word: Compose breaks lines at
     * spaces and after `/` and `-` ("worxbend/" + "macropad"), so those are where the pieces end. Blank pieces
     * are dropped; a blank title gives none.
     */
    fun titleWords(title: String): List<String> = title.split(WORD_BREAK).filter { it.isNotBlank() }

    // A medium tile with a title: 8dp padding around the 32dp icon disc, an 8dp gap and two labelSmall lines.
    private const val MEDIUM_TITLE_MIN_WIDTH = 72f
    private const val MEDIUM_TITLE_FIXED = 32f + 8f + 16f
    private const val TITLE_LINES_SP = 2 * 16f

    /**
     * UX 6.2: a medium tile shows its window's title under the icon, so it reads as a window and not as a block,
     * when the disc, the gap and two `labelSmall` lines at [fontScale] fit its height and it is about 72dp wide.
     */
    fun mediumShowsTitle(rect: MapRect, fontScale: Float): Boolean =
        rect.width >= MEDIUM_TITLE_MIN_WIDTH && rect.height >= MEDIUM_TITLE_FIXED + TITLE_LINES_SP * fontScale

    /**
     * UX 6.3: windows that share an `app_id` get their index (from 1) in [treeOrder]. Windows without an
     * `app_id` and apps with one window get none.
     */
    fun sameAppIndexes(treeOrder: List<Long>, windows: Map<Long, Window>): Map<Long, Int> {
        val byApp = treeOrder.groupBy { windows[it]?.appId }
        return buildMap {
            for ((appId, ids) in byApp) {
                if (appId == null || ids.size < 2) continue
                ids.forEachIndexed { index, id -> put(id, index + 1) }
            }
        }
    }

    fun positionOf(rect: MapRect, map: MapRect): MapPosition {
        if (map.width <= 0f || map.height <= 0f) return MapPosition.Whole
        val fullW = rect.width / map.width >= FULL_SPAN
        val fullH = rect.height / map.height >= FULL_SPAN
        if (fullW && fullH) return MapPosition.Whole
        val cx = (rect.left + rect.width / 2 - map.left) / map.width
        val cy = (rect.top + rect.height / 2 - map.top) / map.height
        val column = if (fullW) 1 else sideOf(cx)
        val row = if (fullH) 1 else sideOf(cy)
        return MapPosition.entries[1 + row * 3 + column]
    }

    private fun sideOf(centre: Float): Int = when {
        centre < SIDE_LOW -> 0
        centre > SIDE_HIGH -> 2
        else -> 1
    }

    private fun kindOf(layout: Layout): MapKind = when (val root = layout.tiled) {
        null -> if (layout.floating.isEmpty()) MapKind.Empty else MapKind.FloatingOnly
        is WindowTile -> MapKind.OneWindow
        is ContainerTile -> when (root.kind) {
            ContainerKind.SPLIT_H -> MapKind.SplitH
            ContainerKind.SPLIT_V -> MapKind.SplitV
            ContainerKind.TABBED -> MapKind.Tabbed
            ContainerKind.STACKED -> MapKind.Stacked
        }
    }

    /** The union of [rects] over [map], sampled on a 64 × 64 grid (floats overlap, so areas do not add up). */
    private fun coverageOf(rects: List<MapRect>, map: MapRect): Float {
        if (rects.isEmpty() || map.area <= 0f) return 0f
        var covered = 0
        for (i in 0 until COVERAGE_GRID) {
            val y = map.top + (i + 0.5f) * map.height / COVERAGE_GRID
            for (j in 0 until COVERAGE_GRID) {
                val x = map.left + (j + 0.5f) * map.width / COVERAGE_GRID
                if (rects.any { it.contains(x, y) }) covered++
            }
        }
        return covered.toFloat() / (COVERAGE_GRID * COVERAGE_GRID)
    }
}

/** Maps a basis-point rect of [source] onto [target] dp. The root frame maps the whole workspace onto the map. */
private class Frame(val source: List<Float>, val target: MapRect) {
    fun map(bounds: List<Long>): MapRect {
        val (sx, sy, sw, sh) = source
        val x0 = clampBasis(bounds[0])
        val y0 = clampBasis(bounds[1])
        val x1 = min(clampBasis(bounds[0] + bounds[2]), MapGeometry.BASIS)
        val y1 = min(clampBasis(bounds[1] + bounds[3]), MapGeometry.BASIS)
        val left = target.left + (x0 - sx) / sw * target.width
        val top = target.top + (y0 - sy) / sh * target.height
        return MapRect(left, top, max(x1 - x0, 0f) / sw * target.width, max(y1 - y0, 0f) / sh * target.height)
    }

    companion object {
        fun root(map: MapRect) = Frame(listOf(0f, 0f, MapGeometry.BASIS, MapGeometry.BASIS), map)

        /** A child frame: [bounds] (basis points) now fill [target]. A zero-size source falls back to the root scale. */
        fun of(bounds: List<Long>, target: MapRect): Frame {
            val w = max(bounds[2].toFloat(), 1f)
            val h = max(bounds[3].toFloat(), 1f)
            return Frame(listOf(clampBasis(bounds[0]), clampBasis(bounds[1]), w, h), target)
        }

        private fun clampBasis(value: Long): Float = value.coerceIn(0L, MapGeometry.BASIS.toLong()).toFloat()
    }
}

/** What the size classes need to know about text: the font scale and the measured width of a short title. */
private class TextFit(val fontScale: Float, val titleWidth: Float)

private class Placer(val windows: Map<Long, Window>, val text: TextFit, val map: MapRect) {
    val tiles = ArrayList<PlacedTile>()
    val groups = ArrayList<PlacedGroup>()

    /** Window id to tree order, for every window of the layout, hidden tab children included. */
    val order = LinkedHashMap<Long, Int>()

    fun place(tile: Tile, frame: Frame, layer: TileLayer) {
        when (tile) {
            is WindowTile -> placeWindow(tile.windowId, frame.map(tile.bounds), layer)
            is ContainerTile -> if (tile.kind.showsOneChild) placeGroup(tile, frame, layer) else tile.children.forEach { place(it, frame, layer) }
        }
    }

    fun tile(windowId: Long, outer: MapRect, layer: TileLayer, treeOrder: Int): PlacedTile {
        val rect = outer.inset(MapGeometry.INSET)
        val sizeClass = MapGeometry.sizeClassOf(rect, text.fontScale, text.titleWidth)
        val fragment = sizeClass == SizeClass.Medium &&
            rect.width >= MapGeometry.FRAGMENT_MIN_WIDTH && rect.height >= MapGeometry.FRAGMENT_MIN_HEIGHT
        return PlacedTile(
            windowId = windowId,
            rect = rect,
            sizeClass = sizeClass,
            layer = layer,
            treeOrder = treeOrder,
            position = MapGeometry.positionOf(rect, map),
            dimmed = false,
            showsTitleFragment = fragment,
        )
    }

    private fun placeWindow(windowId: Long, outer: MapRect, layer: TileLayer) {
        if (windowId !in windows || windowId in order) return
        val treeOrder = order.size
        order[windowId] = treeOrder
        tiles += tile(windowId, outer, layer, treeOrder)
    }

    private fun placeGroup(group: ContainerTile, frame: Frame, layer: TileLayer) {
        val outer = frame.map(group.bounds)
        val shown = group.active?.toInt()?.takeIf { it in group.children.indices } ?: 0
        val tabs = group.children.map { child -> windowIdsOf(child) }
        val kind = if (group.kind == ContainerKind.TABBED) GroupKind.Tabbed else GroupKind.Stacked
        val inner = outer.inset(MapGeometry.INSET)
        val headerHeight = if (kind == GroupKind.Tabbed) {
            MapGeometry.TAB_HEADER
        } else {
            MapGeometry.STACKED_ROW * min(tabs.size, MapGeometry.MAX_STACKED_ROWS)
        }
        // The strip needs room above a body that is still tappable; else the group is just its shown child.
        val hasHeader = inner.width >= MapGeometry.OWN_TARGET && inner.height - headerHeight >= MapGeometry.OWN_TARGET
        val body = if (hasHeader) {
            val top = inner.top + headerHeight
            MapRect(outer.left, top, outer.width, outer.bottom - top)
        } else {
            outer
        }
        val treeOrder = order.size
        if (hasHeader) {
            groups += PlacedGroup(MapRect(inner.left, inner.top, inner.width, headerHeight), kind, tabs, shown, layer, treeOrder, dimmed = false)
        }
        group.children.forEachIndexed { index, child ->
            if (index == shown) {
                val shownChild = group.children[index]
                place(shownChild, Frame.of(shownChild.bounds, body), layer)
            } else {
                // Hidden children keep their tree order (same-app indexes), but are not drawn.
                windowIdsOf(child).forEach { id -> if (id in windows && id !in order) order[id] = order.size }
            }
        }
    }

    private fun windowIdsOf(tile: Tile): List<Long> = when (tile) {
        is WindowTile -> listOf(tile.windowId).filter { it in windows }
        is ContainerTile -> tile.children.flatMap { windowIdsOf(it) }
    }
}
