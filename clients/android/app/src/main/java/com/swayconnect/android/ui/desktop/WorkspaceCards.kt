package com.swayconnect.android.ui.desktop

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.gestures.snapping.SnapPosition
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.ui.unit.max
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.util.lerp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.tabular
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Workspace
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin

private val CARD_MIN_WIDTH = 72.dp
private val CARD_MIN_HEIGHT = 88.dp
private val COMPACT_MIN_WIDTH = 56.dp
private val COMPACT_HEIGHT = 40.dp
private val CARD_GAP = 8.dp
private val RING_GAP = 3.dp
private val RING_WIDTH = 2.dp
private val RING_STRETCH = 16.dp
private val URGENT_DOT = 14.dp
private val URGENT_KEYLINE = 2.dp
private const val PRESSED_SCALE = 0.96f
private const val URGENT_PULSE_LOW = 0.4f
private const val URGENT_PULSE_STEP_MS = 150

/** What a card can ask for. Taps and long presses are off while the desktop is stale. */
internal class CardActions(
    val onTap: (Long) -> Unit,
    val onLongPress: (Long) -> Unit,
    /** "Show on computer" as a TalkBack action; null when it is not offered (Drive mode, or the op is not available). */
    val onShowOnComputer: ((Long) -> Unit)?,
)

/**
 * The workspace card carousel (docs/UX.md 5.4, 17.3). When every card fits, the row starts on the page gutter
 * ([edges]) like reference 2 and does not scroll. When they overflow, it snaps the card nearest the centre, and
 * every card can reach the centre because the side padding is half the free width. The ring follows the pager's
 * offset in the draw phase. The fill shows only the focus that the computer confirmed.
 *
 * @param rowGesture a user drag of the row, from its start until the row settled.
 * @param onRowDragStart a user drag of the row began: user-initiated, like a pager drag.
 * @param onRowSettled a user drag of the row stopped on this card index.
 * @param onStep TalkBack's "Next workspace" / "Previous workspace": +1 or -1. Not user-initiated.
 */
@Composable
internal fun WorkspaceRow(
    desktop: Desktop,
    pagerState: PagerState,
    rowState: LazyListState,
    rowGesture: RowGesture,
    pendingId: Long?,
    enabled: Boolean,
    compact: Boolean,
    actions: CardActions,
    onRowDragStart: () -> Unit,
    onRowSettled: (Int) -> Unit,
    onStep: (Int) -> Unit,
    edges: PaddingValues,
    modifier: Modifier = Modifier,
) {
    val reduceMotion = LocalReduceMotion.current
    val ringColor = MaterialTheme.colorScheme.primary
    val radius = if (compact) COMPACT_HEIGHT / 2 else CARD_RADIUS
    val nextLabel = stringResource(R.string.desktop_next)
    val previousLabel = stringResource(R.string.desktop_previous)
    val rowLabel = stringResource(R.string.desktop_cards)
    RowSettleEffects(rowState, rowGesture, onRowDragStart, onRowSettled)
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val direction = LocalLayoutDirection.current
        val start = edges.calculateStartPadding(direction)
        val end = edges.calculateEndPadding(direction)
        val fits = cardsFit(desktop, compact, maxWidth - start - end)
        val vertical = RING_GAP + RING_WIDTH + 2.dp
        val minWidth = if (compact) COMPACT_MIN_WIDTH else CARD_MIN_WIDTH
        val side = ((maxWidth - minWidth) / 2).coerceAtLeast(start)
        LazyRow(
            state = rowState,
            flingBehavior = rememberSnapFlingBehavior(rowState, SnapPosition.Center),
            // A row that fits has nothing to scroll; a drag on it must not pick the card nearest the centre.
            userScrollEnabled = !fits,
            contentPadding = if (fits) {
                PaddingValues(start = start, end = end, top = vertical, bottom = vertical)
            } else {
                PaddingValues(horizontal = side, vertical = vertical)
            },
            horizontalArrangement = Arrangement.spacedBy(CARD_GAP),
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .ring(pagerState, rowState, ringColor, radius, reduceMotion)
                .semantics {
                    contentDescription = rowLabel
                    customActions = listOf(
                        CustomAccessibilityAction(previousLabel) {
                            onStep(-1)
                            true
                        },
                        CustomAccessibilityAction(nextLabel) {
                            onStep(1)
                            true
                        },
                    )
                }
                .selectableGroup(),
        ) {
            itemsIndexed(desktop.workspaces, key = { _, workspace -> workspace.id }) { index, workspace ->
                WorkspaceCard(
                    workspace = workspace,
                    windows = windowCount(desktop, workspace),
                    shown = index == pagerState.currentPage,
                    pending = pendingId == workspace.id,
                    enabled = enabled,
                    compact = compact,
                    actions = actions,
                    modifier = Modifier.animateItem(
                        fadeInSpec = if (reduceMotion) null else LocalSwayMotion.current.defaultEffectsSpec(),
                        placementSpec = if (reduceMotion) null else LocalSwayMotion.current.defaultSpatialSpec(),
                        fadeOutSpec = if (reduceMotion) null else tween(REMOVE_MS),
                    ),
                )
            }
        }
    }
}

private val CARD_RADIUS = 24.dp
private const val REMOVE_MS = 150
private val CARD_PADDING = 10.dp
private val COMPACT_PADDING = 14.dp
private val COMPACT_SPACING = 6.dp

/**
 * True when every card, at the width its texts measure to, fits in [available] with the gaps and one gap to
 * spare. The card faces below use the same styles and paddings.
 */
@Composable
private fun cardsFit(desktop: Desktop, compact: Boolean, available: Dp): Boolean {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val typography = MaterialTheme.typography
    val largeFont = density.fontScale > LARGE_FONT_SCALE
    fun width(text: String, style: TextStyle): Dp =
        if (text.isEmpty()) 0.dp else with(density) { measurer.measure(text, style, maxLines = 1).size.width.toDp() }
    val widths = desktop.workspaces.map { workspace ->
        val text = cardText(workspace)
        if (compact) {
            val name = width(text.name, typography.labelMedium)
            val spacing = if (name > 0.dp) COMPACT_SPACING else 0.dp
            max(COMPACT_MIN_WIDTH, COMPACT_PADDING * 2 + name + spacing + width(text.number, typography.titleMedium.tabular()))
        } else {
            val count = windowCount(desktop, workspace)
            val countText = if (count == 0) {
                stringResource(R.string.desktop_count_empty)
            } else {
                pluralStringResource(R.plurals.desktop_window_count, count, count)
            }
            val name = if (largeFont) 0.dp else width(text.name, typography.labelMedium)
            val widest = maxOf(name, width(text.number, typography.titleLarge), width(countText, typography.labelSmall))
            max(CARD_MIN_WIDTH, widest + CARD_PADDING * 2)
        }
    }
    val total = widths.fold(0.dp) { sum, card -> sum + card } + CARD_GAP * widths.size
    return total <= available
}

/**
 * A user drag of the card row, from its `DragInteraction.Start` until the row stopped after it. The keep-in-view
 * scroll reads [active] and never fights the user.
 */
internal class RowGesture {
    /** The finger drags the row. */
    var fingerDown by mutableStateOf(false)

    /** A user drag happened and its settle is not reported yet. */
    var active by mutableStateOf(false)
}

/**
 * Reports the centred card once the finger is up and the row stopped (the snap fling included). A drag that did
 * not move the row still ends with a report, so the user flag of the view model is always consumed.
 */
@Composable
private fun RowSettleEffects(rowState: LazyListState, rowGesture: RowGesture, onDragStart: () -> Unit, onRowSettled: (Int) -> Unit) {
    LaunchedEffect(rowState) {
        rowState.interactionSource.interactions.collect { interaction ->
            when (interaction) {
                is DragInteraction.Start -> {
                    rowGesture.fingerDown = true
                    rowGesture.active = true
                    onDragStart()
                }
                is DragInteraction.Stop, is DragInteraction.Cancel -> rowGesture.fingerDown = false
            }
        }
    }
    LaunchedEffect(rowState) {
        snapshotFlow { rowGesture.active && !rowGesture.fingerDown && !rowState.isScrollInProgress }.collect { stopped ->
            // Read again: the gap between the drag and its fling must not count as a stop.
            if (!stopped || rowState.isScrollInProgress) return@collect
            rowGesture.active = false
            centredIndex(rowState)?.let(onRowSettled)
        }
    }
}

/** The index of the card whose centre is nearest the row's centre. */
internal fun centredIndex(rowState: LazyListState): Int? {
    val info = rowState.layoutInfo
    val centre = (info.viewportStartOffset + info.viewportEndOffset) / 2
    return info.visibleItemsInfo.minByOrNull { abs(it.offset + it.size / 2 - centre) }?.index
}

/** Scrolls the row so card [index] sits in the centre: the keep-in-view scroll. It is not user-initiated. */
internal suspend fun centreCard(rowState: LazyListState, index: Int, animate: Boolean, spec: AnimationSpec<Float>) {
    if (index < 0 || index >= rowState.layoutInfo.totalItemsCount) return
    if (rowState.layoutInfo.visibleItemsInfo.none { it.index == index }) {
        if (animate) rowState.animateScrollToItem(index) else rowState.scrollToItem(index)
    }
    val info = rowState.layoutInfo
    val item = info.visibleItemsInfo.firstOrNull { it.index == index } ?: return
    val delta = (item.offset + item.size / 2 - (info.viewportStartOffset + info.viewportEndOffset) / 2).toFloat()
    if (delta == 0f) return
    if (animate) rowState.animateScrollBy(delta, spec) else rowState.scrollBy(delta)
}

/**
 * The "shown on the phone" ring (UX 5.4, 13): 2dp, 3dp outside the card. It reads the pager and the row in the
 * draw phase only, lerps between the two cards by the pager's offset and stretches by 16dp × sin(π·|f|).
 * Under reduce motion it jumps from card to card.
 */
private fun Modifier.ring(pagerState: PagerState, rowState: LazyListState, color: Color, radius: Dp, reduceMotion: Boolean): Modifier =
    drawWithContent {
        drawContent()
        val info = rowState.layoutInfo
        val start = info.viewportStartOffset
        fun span(index: Int): Pair<Float, Float>? =
            info.visibleItemsInfo.firstOrNull { it.index == index }?.let { (it.offset - start).toFloat() to it.size.toFloat() }
        val page = pagerState.currentPage
        val fraction = if (reduceMotion) 0f else pagerState.currentPageOffsetFraction
        val here = span(page) ?: return@drawWithContent
        val t = abs(fraction)
        val next = if (fraction == 0f) null else span(page + if (fraction > 0f) 1 else -1)
        var left = if (next != null) lerp(here.first, next.first, t) else here.first
        var width = if (next != null) lerp(here.second, next.second, t) else here.second
        val stretch = if (reduceMotion || next == null) 0f else RING_STRETCH.toPx() * sin(PI.toFloat() * t)
        left -= stretch / 2
        width += stretch
        if (layoutDirection == LayoutDirection.Rtl) left = size.width - left - width
        val gap = (RING_GAP + RING_WIDTH / 2).toPx()
        val vertical = (RING_GAP + RING_WIDTH + 2.dp).toPx()
        drawRoundRect(
            color = color,
            topLeft = Offset(left - gap, vertical - gap),
            size = Size(width + 2 * gap, size.height - 2 * vertical + 2 * gap),
            cornerRadius = CornerRadius(radius.toPx() + gap),
            style = Stroke(RING_WIDTH.toPx()),
        )
    }

/**
 * One workspace card (UX 5.4): name line, number, count line. Fill = focused on the computer (a colour fade,
 * never a slide); the ring around it is drawn by the row. An urgent dot pulses once when it appears.
 */
@Composable
private fun WorkspaceCard(
    workspace: Workspace,
    windows: Int,
    shown: Boolean,
    pending: Boolean,
    enabled: Boolean,
    compact: Boolean,
    actions: CardActions,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    val motion = LocalSwayMotion.current
    val haptics = LocalHaptics.current
    val focused = workspace.focused
    val container by animateColorAsState(if (focused) scheme.primary else scheme.surface, motion.defaultEffectsSpec(), label = "card fill")
    val content by animateColorAsState(if (focused) scheme.onPrimary else scheme.onSurface, motion.defaultEffectsSpec(), label = "card text")
    val countColor by animateColorAsState(
        if (focused) LocalSwayTokens.current.accentOnInk else scheme.onSurfaceVariant,
        motion.defaultEffectsSpec(),
        label = "card count",
    )
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) PRESSED_SCALE else 1f, motion.fastSpatialSpec(), label = "press")
    val text = cardText(workspace)
    val count = if (windows == 0) stringResource(R.string.desktop_count_empty) else pluralStringResource(R.plurals.desktop_window_count, windows, windows)
    val spoken = cardSpoken(text, count, focused, workspace.urgent, pending)
    val showLabel = stringResource(R.string.desktop_show_on_computer)
    val shape = if (compact) CircleShape else MaterialTheme.shapes.large
    val largeFont = LocalDensity.current.fontScale > LARGE_FONT_SCALE
    Box(
        modifier = modifier
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
            }
            .then(if (compact) Modifier.minimumInteractiveComponentSize() else Modifier),
    ) {
        // One TalkBack node per card (UX 15): the clickable tab carries the words, the state and the action;
        // the texts inside are cleared after it.
        val face = Modifier
            .clip(shape)
            .background(container)
            .combinedClickable(
                interactionSource = interaction,
                indication = ripple(),
                enabled = enabled,
                role = Role.Tab,
                onClickLabel = stringResource(R.string.desktop_card_open),
                onLongClickLabel = stringResource(R.string.desktop_card_more),
                onLongClick = {
                    haptics.perform(SwayHaptic.LongPress)
                    actions.onLongPress(workspace.id)
                },
                hapticFeedbackEnabled = false,
                onClick = { actions.onTap(workspace.id) },
            )
            .semantics {
                contentDescription = spoken
                selected = shown
                actions.onShowOnComputer?.let { show ->
                    customActions = listOf(
                        CustomAccessibilityAction(showLabel) {
                            show(workspace.id)
                            true
                        },
                    )
                }
            }
        if (compact) {
            Row(
                modifier = face.heightIn(min = COMPACT_HEIGHT).widthIn(min = COMPACT_MIN_WIDTH).padding(horizontal = COMPACT_PADDING).clearAndSetSemantics {},
                horizontalArrangement = Arrangement.spacedBy(COMPACT_SPACING, Alignment.CenterHorizontally),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (text.name.isNotEmpty()) CardLine(text.name, MaterialTheme.typography.labelMedium, content)
                CardLine(text.number, MaterialTheme.typography.titleMedium.tabular(), content)
                if (pending) CircularProgressIndicator(Modifier.size(14.dp), color = countColor, strokeWidth = 2.dp)
            }
        } else {
            Column(
                modifier = face
                    .widthIn(min = CARD_MIN_WIDTH)
                    .heightIn(min = CARD_MIN_HEIGHT)
                    .padding(horizontal = CARD_PADDING, vertical = 10.dp)
                    .clearAndSetSemantics {},
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
            ) {
                // Every card keeps the line, so all cards have one height and the ring fits each (UX 15: dropped above 1.5).
                if (!largeFont) CardLine(text.name, MaterialTheme.typography.labelMedium, content.copy(alpha = NAME_ALPHA))
                CardLine(text.number, MaterialTheme.typography.titleLarge, content)
                if (pending) {
                    CircularProgressIndicator(Modifier.size(16.dp), color = countColor, strokeWidth = 2.dp)
                } else {
                    CardLine(count, MaterialTheme.typography.labelSmall, countColor)
                }
            }
        }
        if (workspace.urgent) UrgentDot(Modifier.align(Alignment.TopEnd).offset(x = 2.dp, y = (-2).dp))
    }
}

private const val NAME_ALPHA = 0.78f

@Composable
private fun CardLine(text: String, style: TextStyle, color: Color) {
    Text(text = text, style = style, color = color, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

@Composable
private fun cardSpoken(text: CardText, count: String, focused: Boolean, urgent: Boolean, pending: Boolean): String =
    listOfNotNull(
        stringResource(R.string.desktop_card_spoken, text.number),
        text.name.takeIf { it.isNotEmpty() },
        count,
        stringResource(R.string.desktop_card_focused).takeIf { focused },
        stringResource(R.string.desktop_card_urgent).takeIf { urgent },
        stringResource(R.string.desktop_card_switching).takeIf { pending },
    ).joinToString(", ")

/**
 * 14dp `error` dot with "!" (UX 5.4). It sits in a 2dp `surface` keyline: dark `error` on the white focused
 * fill is only 2.06:1, the keyline keeps the mark at 3:1 or more on every fill (A1, ThemeContrastTest).
 * Pulses twice (alpha 1 → 0.4 → 1, 600ms) when it appears, not under reduce motion.
 */
@Composable
private fun UrgentDot(modifier: Modifier) {
    val reduceMotion = LocalReduceMotion.current
    val alpha = remember { Animatable(1f) }
    LaunchedEffect(Unit) {
        if (reduceMotion) return@LaunchedEffect
        repeat(2) {
            alpha.animateTo(URGENT_PULSE_LOW, tween(URGENT_PULSE_STEP_MS))
            alpha.animateTo(1f, tween(URGENT_PULSE_STEP_MS))
        }
    }
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier = modifier
            .size(URGENT_DOT + URGENT_KEYLINE * 2)
            .graphicsLayer { this.alpha = alpha.value }
            .border(URGENT_KEYLINE, scheme.surface, CircleShape)
            .padding(URGENT_KEYLINE)
            .background(scheme.error, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Text("!", color = scheme.onError, fontSize = 10.sp, lineHeight = 10.sp, fontWeight = FontWeight.Bold)
    }
}
