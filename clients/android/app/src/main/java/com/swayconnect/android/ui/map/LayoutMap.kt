package com.swayconnect.android.ui.map

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.traversalIndex
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.protocol.Layout as SwayLayout
import com.swayconnect.protocol.Window
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

private const val PRESSED_SCALE = 0.96f
private const val DIMMED_ALPHA = 0.3f
private const val PORTRAIT_HEIGHT_SHARE = 0.55f
private const val LANDSCAPE_HEIGHT_SHARE = 0.8f
private const val DEFAULT_ASPECT = 9f / 16f
private val SLAB_PADDING = 8.dp

/**
 * What the map does on a gesture. The map renders server state only: every action goes to the caller.
 *
 * @param onFocus a tap on a tile (UX 6.10: `window.focus`, in Drive and Browse mode).
 * @param onLongPress opens the Window sheet; the map already fired the long-press haptic.
 * @param onOpenGroup a narrow tab or stacked strip: the Tab group sheet with these windows.
 * @param onShowSmallWindows the "+N small windows" chip: the Small windows sheet.
 * @param onShowList the dense-workspace chip: list view for this page only.
 * @param tileActions the TalkBack custom actions of a tile (only those the phone may do).
 */
data class MapActions(
    val onFocus: (windowId: Long) -> Unit,
    val onLongPress: (windowId: Long) -> Unit,
    val onOpenGroup: (windowIds: List<Long>) -> Unit,
    val onShowSmallWindows: (windowIds: List<Long>) -> Unit,
    val onShowList: () -> Unit,
    val tileActions: (Window) -> List<CustomAccessibilityAction> = { emptyList() },
)

/**
 * The map slab of a workspace page (UX 5.1, 6) with its chips below it: one item of the page's `LazyColumn`.
 *
 * @param windows the desktop's windows; tiles whose window is missing are skipped.
 * @param workspaceLabel the workspace's name for "Nothing open on 5".
 * @param pendingWindowId a window whose action waits: spinner, taps ignored.
 * @param enabled false while the state is stale (UX 5.9); the caller also dims the page.
 * @param appName the name an `app_id` shows as (its desktop entry's name, else the id).
 */
@Composable
fun MapSlab(
    layout: SwayLayout,
    windows: List<Window>,
    workspaceLabel: String,
    actions: MapActions,
    modifier: Modifier = Modifier,
    pendingWindowId: Long? = null,
    enabled: Boolean = true,
    appName: (String) -> String,
) {
    // UX 6.6: the floating layer hides and shows per page, not stored.
    var showFloating by rememberSaveable { mutableStateOf(true) }
    val fontScale = LocalDensity.current.fontScale
    val titleWidth = shortTitleWidth()
    val maxHeight = slabMaxHeight()
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val innerWidth = max(maxWidth.value - 2 * SLAB_PADDING.value, 0f)
        val innerHeight = min(innerWidth * aspectOf(layout), maxHeight - 2 * SLAB_PADDING.value)
        val byId = remember(windows) { windows.associateBy { it.id } }
        val plan = remember(layout, windows, innerWidth, innerHeight, fontScale, showFloating, titleWidth) {
            MapGeometry.place(layout, windows, innerWidth, innerHeight, fontScale, showFloating, titleWidth)
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surface) {
                LayoutMap(
                    plan = plan,
                    windows = byId,
                    workspaceLabel = workspaceLabel,
                    actions = actions,
                    pendingWindowId = pendingWindowId,
                    enabled = enabled,
                    appName = appName,
                    modifier = Modifier.padding(SLAB_PADDING).fillMaxWidth().height(innerHeight.dp),
                )
            }
            MapChips(plan, showFloating, enabled, actions, onToggleFloating = { showFloating = !showFloating })
        }
    }
}

/** A short title the large class must fit beside its icon (UX 6.2): about 8 characters. */
private const val SHORT_TITLE_SAMPLE = "Abcdefgh"

/** The width in dp of [SHORT_TITLE_SAMPLE] in the tile title style at the current font scale. */
@Composable
private fun shortTitleWidth(): Float {
    val measurer = rememberTextMeasurer()
    val style = MaterialTheme.typography.labelLarge
    val density = LocalDensity.current
    return remember(measurer, style, density) {
        measurer.measure(SHORT_TITLE_SAMPLE, style, maxLines = 1).size.width / density.density
    }
}

/** height / width of the workspace, or 16:9 when the layout's size is unusable. */
private fun aspectOf(layout: SwayLayout): Float {
    val w = layout.size[0]
    val h = layout.size[1]
    return if (w > 0 && h > 0) h.toFloat() / w else DEFAULT_ASPECT
}

/** A tall (portrait monitor) map is capped to a share of the window height and letterboxed. */
@Composable
private fun slabMaxHeight(): Float {
    val size = LocalWindowInfo.current.containerSize
    val density = LocalDensity.current.density
    val heightDp = size.height / density
    val share = if (size.width > size.height) LANDSCAPE_HEIGHT_SHARE else PORTRAIT_HEIGHT_SHARE
    return heightDp * share
}

/** A child of the map layout: where it goes (read in the layout phase, so animation only relayouts) and its z. */
private class Slot(val rect: () -> Rect, val z: Float)

@Composable
private fun LayoutMap(
    plan: MapPlan,
    windows: Map<Long, Window>,
    workspaceLabel: String,
    actions: MapActions,
    pendingWindowId: Long?,
    enabled: Boolean,
    appName: (String) -> String,
    modifier: Modifier,
) {
    val reduceMotion = LocalReduceMotion.current
    // UX 6.9: up to 25 tiles animate their bounds; more snap. The focus node always slides (unless reduce motion).
    val animateTiles = !reduceMotion && plan.tiles.size <= MapGeometry.ANIMATED_TILE_LIMIT
    var settled by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { settled = true }
    // The tap detector restarts only when the plan or the pending id changes; it reads these through State, so
    // it never acts on callbacks of an older composition (a changed availability, for example).
    val currentActions by rememberUpdatedState(actions)
    val currentHaptics by rememberUpdatedState(LocalHaptics.current)
    val description = mapDescription(plan)
    val focusedId = windows.values.firstOrNull { it.focused }?.id
    val tileZ = plan.tiles.withIndex().associate { (index, tile) -> tile.windowId to (2f * index + 2f) }

    val tapModifier = if (enabled) {
        Modifier.pointerInput(plan, pendingWindowId) {
            fun sharedHit(offset: Offset): Long? {
                val x = offset.x / density
                val y = offset.y / density
                if (plan.groups.any { !it.dimmed && it.header.contains(x, y) }) return null
                val tile = plan.hit(x, y) ?: return null
                // Large and medium tiles take their own taps; the shared rule covers small tiles and the gaps.
                val ownTarget = tile.sizeClass != SizeClass.Small && tile.rect.contains(x, y)
                return tile.windowId.takeIf { !ownTarget && it != pendingWindowId }
            }
            detectTapGestures(
                onTap = { offset -> sharedHit(offset)?.let(currentActions.onFocus) },
                onLongPress = { offset ->
                    sharedHit(offset)?.let { id ->
                        currentHaptics.perform(SwayHaptic.LongPress)
                        currentActions.onLongPress(id)
                    }
                },
            )
        }
    } else {
        Modifier
    }

    Layout(
        modifier = modifier
            .semantics {
                isTraversalGroup = true
                contentDescription = description
            }
            .then(tapModifier),
        content = {
            if (plan.tiles.isEmpty()) EmptyMap(plan.map, workspaceLabel)
            // A focused sliver is ink itself; a 3dp ring would swallow it.
            val focusedTile = plan.tiles.firstOrNull { it.windowId == focusedId && !it.dimmed && it.sizeClass != SizeClass.Sliver }
            // Over its tile and under the next one (z steps by 2), so the ring is never hidden by the tile's wash.
            if (focusedTile != null) FocusNode(focusedTile.rect, z = tileZ.getValue(focusedTile.windowId) + 1f)
            for (group in plan.groups) {
                val z = plan.tiles.firstOrNull { it.treeOrder >= group.treeOrder }?.let { tileZ[it.windowId] } ?: 1f
                key(group.layer, group.treeOrder) { GroupHeader(group, windows, actions, enabled, animateTiles, z, appName) }
            }
            for (tile in plan.tiles) {
                val window = windows[tile.windowId] ?: continue
                key(tile.windowId) {
                    val look = TileLook(
                        tile = tile,
                        window = window,
                        focused = tile.windowId == focusedId,
                        pending = tile.windowId == pendingWindowId,
                        sameAppIndex = plan.sameAppIndex[tile.windowId],
                        pulseNewMarks = settled,
                        appName = window.appId?.let(appName),
                    )
                    MapTile(look, actions, enabled, animate = animateTiles, grow = settled && animateTiles, z = tileZ.getValue(tile.windowId))
                }
            }
        },
    ) { measurables, constraints ->
        val placed = measurables.map { measurable ->
            val slot = measurable.layoutId as Slot
            val rect = slot.rect()
            val width = (rect.width * density).roundToInt().coerceAtLeast(0)
            val height = (rect.height * density).roundToInt().coerceAtLeast(0)
            Triple(measurable.measure(Constraints.fixed(width, height)), rect, slot.z)
        }
        layout(constraints.maxWidth, constraints.maxHeight) {
            for ((placeable, rect, z) in placed) {
                placeable.placeRelative((rect.left * density).roundToInt(), (rect.top * density).roundToInt(), z)
            }
        }
    }
}

private fun MapRect.toRect() = Rect(left, top, right, bottom)

/** New windows grow from the centre of their rect (UX 6.9). */
private fun MapRect.seed() = Rect(Offset(left + width / 2, top + height / 2), 0f)

/** The rect of a child, animated to each new target with the default spatial spring, or snapped. */
@Composable
private fun animatedRect(target: MapRect, animate: Boolean, grow: Boolean): () -> Rect {
    val spec = LocalSwayMotion.current.defaultSpatialSpec<Rect>()
    val rect = remember { Animatable(if (grow) target.seed() else target.toRect(), Rect.VectorConverter) }
    LaunchedEffect(target, animate) {
        if (animate) rect.animateTo(target.toRect(), spec) else rect.snapTo(target.toRect())
    }
    return { rect.value }
}

/**
 * UX 6.4: one ink ring that slides from tile to tile when focus moves. A ring, not a fill: the focused tile keeps
 * its app wash, so it still reads as that app's window. It draws nothing else and takes no touch.
 */
@Composable
private fun FocusNode(target: MapRect, z: Float) {
    val rect = animatedRect(target, animate = true, grow = false)
    Box(
        Modifier
            .layoutId(Slot(rect, z))
            .border(FOCUS_RING, MaterialTheme.colorScheme.primary, TILE_SHAPE),
    )
}

private val FOCUS_RING = 3.dp

@Composable
private fun MapTile(look: TileLook, actions: MapActions, enabled: Boolean, animate: Boolean, grow: Boolean, z: Float) {
    val tile = look.tile
    val rect = animatedRect(tile.rect, animate, grow)
    val haptics = LocalHaptics.current
    val focusLabel = stringResource(R.string.map_action_focus)
    val moreLabel = stringResource(R.string.window_more_actions)
    val spoken = tileDescription(look)
    val state = tileState(look)
    val active = enabled && !look.pending && tile.tappable
    val longPress = {
        haptics.perform(SwayHaptic.LongPress)
        actions.onLongPress(tile.windowId)
    }
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) PRESSED_SCALE else 1f, LocalSwayMotion.current.fastSpatialSpec(), label = "press")

    // Large and medium tiles take their own taps; small ones get theirs from the map's shared hit rule (UX 6.2).
    val touch = if (tile.tappable && tile.sizeClass != SizeClass.Small) {
        Modifier.combinedClickable(
            interactionSource = interaction,
            indication = ripple(),
            enabled = active,
            role = Role.Button,
            onClickLabel = focusLabel,
            onLongClickLabel = moreLabel,
            onLongClick = longPress,
            // The app's LongPress above follows the haptics setting; the built-in one would not.
            hapticFeedbackEnabled = false,
            onClick = { actions.onFocus(tile.windowId) },
        )
    } else {
        Modifier
    }
    // One node per tile, its inner texts cleared: TalkBack reads the description once, not the title twice.
    val described = Modifier.clearAndSetSemantics {
        if (!tile.tappable) return@clearAndSetSemantics
        role = Role.Button
        contentDescription = spoken
        if (state.isNotEmpty()) stateDescription = state
        traversalIndex = tile.treeOrder.toFloat()
        if (!active) {
            disabled()
            return@clearAndSetSemantics
        }
        customActions = actions.tileActions(look.window)
        if (tile.sizeClass == SizeClass.Small) {
            onClick(focusLabel) { actions.onFocus(tile.windowId); true }
            onLongClick(moreLabel) { longPress(); true }
        }
    }
    TileBody(
        look = look,
        modifier = Modifier
            .layoutId(Slot(rect, z))
            .graphicsLayer {
                alpha = if (tile.dimmed) DIMMED_ALPHA else 1f
                scaleX = scale
                scaleY = scale
            }
            .then(touch)
            .then(described),
    )
}

@Composable
private fun GroupHeader(
    group: PlacedGroup,
    windows: Map<Long, Window>,
    actions: MapActions,
    enabled: Boolean,
    animate: Boolean,
    z: Float,
    appName: (String) -> String,
) {
    val rect = animatedRect(group.header, animate, grow = false)
    val count = pluralStringResource(R.plurals.map_window_count, group.windowCount, group.windowCount)
    val shownTitle = group.tabs.getOrNull(group.shown)?.firstOrNull()?.let { windowName(windows[it], appName) }.orEmpty()
    val groupText = stringResource(
        if (group.kind == GroupKind.Tabbed) R.string.map_group_tabbed else R.string.map_group_stacked,
        count,
        shownTitle,
    )
    val focusLabel = stringResource(R.string.map_action_focus)
    val openLabel = stringResource(R.string.map_action_open_group)
    val tabTexts = group.tabs.mapIndexed { index, ids ->
        stringResource(R.string.map_tab, index + 1, group.tabs.size, windowName(windows[ids.firstOrNull()], appName))
    }
    val active = enabled && !group.dimmed
    val base = Modifier
        .layoutId(Slot(rect, z))
        .graphicsLayer { alpha = if (group.dimmed) DIMMED_ALPHA else 1f }
    val strip = when {
        group.dimmed -> base.clearAndSetSemantics { }
        group.segmentsTappable -> base.semantics {
            isTraversalGroup = true
            contentDescription = groupText
            traversalIndex = group.treeOrder.toFloat()
        }
        else -> base
            .clickable(enabled = active, onClickLabel = openLabel, role = Role.Button) { actions.onOpenGroup(group.tabs.flatten()) }
            .clearAndSetSemantics {
                role = Role.Button
                contentDescription = groupText
                traversalIndex = group.treeOrder.toFloat()
                if (active) onClick(openLabel) { actions.onOpenGroup(group.tabs.flatten()); true } else disabled()
            }
    }
    GroupStrip(group, windows, strip) { index ->
        val first = group.tabs[index].firstOrNull()
        if (!group.segmentsTappable || first == null) {
            Modifier
        } else {
            Modifier
                .clickable(enabled = active, onClickLabel = focusLabel, role = Role.Button) { actions.onFocus(first) }
                .clearAndSetSemantics {
                    role = Role.Button
                    contentDescription = tabTexts[index]
                    if (!active) disabled()
                }
        }
    }
}

@Composable
private fun EmptyMap(map: MapRect, workspaceLabel: String) {
    val outline = MaterialTheme.colorScheme.outline
    val radius = with(LocalDensity.current) { 8.dp.toPx() }
    val stroke = with(LocalDensity.current) { Stroke(1.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(12f, 8f))) }
    Box(
        Modifier
            .layoutId(Slot({ map.toRect() }, 0f))
            .drawBehind { drawRoundRect(outline, cornerRadius = CornerRadius(radius), style = stroke) }
            .padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = stringResource(R.string.map_empty, workspaceLabel),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun MapChips(plan: MapPlan, showFloating: Boolean, enabled: Boolean, actions: MapActions, onToggleFloating: () -> Unit) {
    val small = plan.smallWindowIds
    val floatingChip = plan.showsFloatingChip
    if (small.isEmpty() && !floatingChip && !plan.isDense) return
    // UX 15: chip edges at 3:1 on the background, not the default `outlineVariant`.
    val outline = MaterialTheme.colorScheme.outline
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (small.isNotEmpty()) {
            AssistChip(
                onClick = { actions.onShowSmallWindows(small) },
                enabled = enabled,
                border = AssistChipDefaults.assistChipBorder(enabled, borderColor = outline),
                label = { Text(pluralStringResource(R.plurals.map_chip_small, small.size, small.size)) },
            )
        }
        if (floatingChip) {
            val state = stringResource(if (showFloating) R.string.map_chip_floating_shown else R.string.map_chip_floating_hidden)
            FilterChip(
                selected = showFloating,
                onClick = onToggleFloating,
                enabled = enabled,
                border = FilterChipDefaults.filterChipBorder(enabled, showFloating, borderColor = outline, selectedBorderColor = outline),
                label = { Text(stringResource(R.string.map_chip_floating, plan.floatingCount)) },
                modifier = Modifier.semantics { stateDescription = state },
            )
        }
        if (plan.isDense) {
            AssistChip(
                onClick = actions.onShowList,
                enabled = enabled,
                border = AssistChipDefaults.assistChipBorder(enabled, borderColor = outline),
                label = { Text(stringResource(R.string.map_chip_dense)) },
            )
        }
    }
}

@Composable
private fun mapDescription(plan: MapPlan): String {
    val kind = when (plan.kind) {
        MapKind.SplitH -> R.string.map_kind_split_h
        MapKind.SplitV -> R.string.map_kind_split_v
        MapKind.Tabbed -> R.string.map_kind_tabbed
        MapKind.Stacked -> R.string.map_kind_stacked
        MapKind.OneWindow -> R.string.map_kind_one_window
        MapKind.FloatingOnly -> R.string.map_kind_floating_only
        MapKind.Empty -> R.string.map_kind_empty
    }
    val count = pluralStringResource(R.plurals.map_window_count, plan.windowCount, plan.windowCount)
    return stringResource(R.string.map_description, stringResource(kind), count)
}

/** The spoken name of a tab: the app name the screen shows, else the title, else "Untitled". */
@Composable
private fun windowName(window: Window?, appName: (String) -> String): String {
    val untitled = stringResource(R.string.window_untitled)
    return window?.appId?.let(appName) ?: window?.title ?: untitled
}

/** "Chrome, Docs, left": the app name the tile shows, title, place on the map (label in name, WCAG 2.5.3). */
@Composable
private fun tileDescription(look: TileLook): String {
    val window = look.window
    val position = when (look.tile.position) {
        MapPosition.Whole -> R.string.map_pos_whole
        MapPosition.TopLeft -> R.string.map_pos_top_left
        MapPosition.Top -> R.string.map_pos_top
        MapPosition.TopRight -> R.string.map_pos_top_right
        MapPosition.Left -> R.string.map_pos_left
        MapPosition.Centre -> R.string.map_pos_centre
        MapPosition.Right -> R.string.map_pos_right
        MapPosition.BottomLeft -> R.string.map_pos_bottom_left
        MapPosition.Bottom -> R.string.map_pos_bottom
        MapPosition.BottomRight -> R.string.map_pos_bottom_right
    }
    val untitled = stringResource(R.string.window_untitled)
    val name = listOfNotNull(look.appName, window.title).distinct().ifEmpty { listOf(untitled) }
    return (name + stringResource(position)).joinToString(", ")
}

/** "focused on computer, needs attention, floating, window 2 of this app": the words behind every mark. */
@Composable
private fun tileState(look: TileLook): String {
    val window = look.window
    return listOfNotNull(
        stringResource(R.string.window_mark_focused_spoken).takeIf { look.focused },
        stringResource(R.string.window_mark_urgent_spoken).takeIf { window.urgent },
        stringResource(R.string.window_mark_fullscreen).takeIf { window.fullscreen },
        stringResource(R.string.window_mark_floating).takeIf { window.floating },
        look.sameAppIndex?.let { stringResource(R.string.window_same_app_index_spoken, it) },
        stringResource(R.string.window_waiting).takeIf { look.pending },
    ).joinToString(", ")
}
