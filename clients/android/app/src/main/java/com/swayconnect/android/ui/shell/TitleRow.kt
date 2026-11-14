package com.swayconnect.android.ui.shell

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.layout.IntrinsicMeasurable
import androidx.compose.ui.layout.IntrinsicMeasureScope
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.layout.Placeable
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayIcons

private enum class TitleSlot { Column, Pill, CompactPill, Round }

private val TITLE_GAP = 8.dp

/**
 * The title row of docs/UX.md 4.1 (reference 2): the title with the status line tight under it, and the context
 * pill and the round button centred against that pair. Grows with the font scale; never shorter than 72dp.
 * [gutter] holds the start and end edges shared with the page below, so everything hangs on one line.
 * The title and the status line have priority: when either would not fit beside the full context pill, the pill
 * is drawn icon-only ([contextPill] gets `compact = true`). TalkBack order (UX 15): title, status line, context pill, round button.
 */
@Composable
internal fun TitleRow(
    title: String,
    status: StatusKind,
    pills: StatusPills,
    gutter: PaddingValues,
    onStatusClick: () -> Unit,
    contextPill: @Composable (compact: Boolean) -> Unit,
    roundButton: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    SubcomposeLayout(modifier.fillMaxWidth().heightIn(min = 72.dp).padding(gutter)) { constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        // Composed first, so the title column comes first among the row's children.
        val column = subcompose(TitleSlot.Column) {
            Column {
                TitleText(title, Modifier.semantics { heading() })
                StatusLine(status, pills, onStatusClick)
            }
        }
        var pill = subcompose(TitleSlot.Pill) { contextPill(false) }.map { it.measure(loose) }
        val round = subcompose(TitleSlot.Round, roundButton).map { it.measure(loose) }
        // The widest of the title and the status line: a status pill cut to "Awake …" loses its minutes too.
        val columnNeeds = column.maxOfOrNull { it.maxIntrinsicWidth(Constraints.Infinity) } ?: 0
        val gap = TITLE_GAP.roundToPx()
        fun titleRoom(trailing: List<Placeable>): Int =
            constraints.maxWidth - trailing.sumOf { it.width + gap }
        if (pill.isNotEmpty() && columnNeeds > titleRoom(pill + round)) {
            pill = subcompose(TitleSlot.CompactPill) { contextPill(true) }.map { it.measure(loose) }
        }
        val trailing = pill + round
        val texts = column.map { it.measure(loose.copy(maxWidth = titleRoom(trailing).coerceAtLeast(0))) }
        val height = maxOf(constraints.minHeight, (texts + trailing).maxOfOrNull { it.height } ?: 0)
        layout(constraints.maxWidth, height) {
            texts.forEach { it.placeRelative(0, (height - it.height) / 2) }
            var x = constraints.maxWidth
            for (placeable in trailing.asReversed()) {
                x -= placeable.width
                placeable.placeRelative(x, (height - placeable.height) / 2)
                x -= gap
            }
        }
    }
}

@Composable
private fun TitleText(title: String, modifier: Modifier = Modifier) {
    Text(
        text = title,
        style = MaterialTheme.typography.headlineLarge,
        color = MaterialTheme.colorScheme.onBackground,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier,
    )
}

/** The optional pills after the status word (UX 4.1). [onAwakeClick] opens the duration sheet. */
internal data class StatusPills(
    val awake: AwakePill?,
    val typing: Boolean,
    val drivePaused: Boolean,
    val onAwakeClick: () -> Unit,
)

/**
 * Dot + word, then the state pills. Only the dot + word node is a polite live region (UX 15): it changes only
 * on a transition, so only transitions are announced. Pills sit in their own nodes, outside it. The row is about
 * 24dp tall; Compose widens the touch area of a clickable that small to the 48dp minimum. One row on every
 * area, so the header keeps its height across tabs: the word keeps its width, a pill that does not fit ends in
 * an ellipsis and keeps its full text in semantics.
 */
@Composable
private fun StatusLine(status: StatusKind, pills: StatusPills, onClick: () -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val squeeze = Modifier.weight(1f, fill = false)
        Row(
            modifier = Modifier
                .clickable(onClickLabel = stringResource(R.string.status_open_computer), onClick = onClick)
                .semantics { liveRegion = LiveRegionMode.Polite }
                .padding(vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            StatusDot(status)
            Text(
                text = stringResource(status.word),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
        pills.awake?.let { AwakeStatus(it, pills.onAwakeClick, squeeze) }
        if (pills.typing) StatePill(text = stringResource(R.string.status_typing), tone = PillTone.Neutral, modifier = squeeze, icon = SwayIcons.Keyboard.filled)
        if (pills.drivePaused) StatePill(text = stringResource(R.string.status_drive_paused), tone = PillTone.Neutral, modifier = squeeze)
    }
}

/**
 * The Awake pill: "Awake 54 min", or "54 min" beside the coffee glyph when the whole text does not fit its room
 * ([AwakePill.dropsWord]); only then an ellipsis. Both forms are composed and the layout places one. TalkBack
 * always hears the whole text.
 */
@Composable
private fun AwakeStatus(awake: AwakePill, onClick: () -> Unit, modifier: Modifier) {
    val full = awake.minutesLeft?.let { stringResource(R.string.status_awake_minutes, it) } ?: stringResource(R.string.status_awake)
    val short = awake.minutesLeft?.let { stringResource(R.string.status_awake_minutes_short, it) }
    val unspoken = Modifier.clearAndSetSemantics {}
    Layout(
        content = {
            StatePill(full, PillTone.Neutral, unspoken, icon = SwayIcons.Coffee.filled)
            if (short != null) StatePill(short, PillTone.Neutral, unspoken, icon = SwayIcons.Coffee.filled)
        },
        modifier = modifier
            .clickable(onClickLabel = stringResource(R.string.keep_awake_choose), onClick = onClick)
            .semantics { contentDescription = full },
        measurePolicy = remember(awake) { AwakeMeasurePolicy(awake) },
    )
}

/** Places the whole pill (first child) or the minutes-only one (last child). Intrinsics ask for the whole one. */
private class AwakeMeasurePolicy(private val awake: AwakePill) : MeasurePolicy {
    override fun MeasureScope.measure(measurables: List<Measurable>, constraints: Constraints): MeasureResult {
        val full = measurables.first()
        val dropsWord = awake.dropsWord(full.maxIntrinsicWidth(constraints.maxHeight), constraints.maxWidth)
        val placeable = (if (dropsWord) measurables.last() else full).measure(constraints)
        return layout(placeable.width, placeable.height) { placeable.placeRelative(0, 0) }
    }

    override fun IntrinsicMeasureScope.maxIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int): Int =
        measurables.first().maxIntrinsicWidth(height)

    override fun IntrinsicMeasureScope.minIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int): Int =
        measurables.last().minIntrinsicWidth(height)

    override fun IntrinsicMeasureScope.maxIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int): Int =
        measurables.first().maxIntrinsicHeight(width)

    override fun IntrinsicMeasureScope.minIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int): Int =
        measurables.first().minIntrinsicHeight(width)
}

private val StatusKind.word: Int
    get() = when (this) {
        StatusKind.Ready -> R.string.status_ready
        StatusKind.Connecting -> R.string.status_connecting
        StatusKind.CheckingKey -> R.string.status_checking_key
        StatusKind.Offline -> R.string.status_offline
        StatusKind.SwayOffline -> R.string.status_sway_offline
        StatusKind.Locked -> R.string.status_locked
        StatusKind.ScreensOff -> R.string.status_screens_off
        StatusKind.ReadOnly -> R.string.status_read_only
        StatusKind.NotPaired -> R.string.status_not_paired
        StatusKind.WrongKey -> R.string.status_wrong_key
        StatusKind.Refused -> R.string.status_refused
    }

private const val ARC_TURN_MS = 1000

/** The shape of each state (UX 4.1), so colour is never the only sign. Decorative: the word carries it. */
@Composable
private fun StatusDot(status: StatusKind) {
    val scheme = MaterialTheme.colorScheme
    val muted = scheme.onSurfaceVariant
    val ok = LocalSwayTokens.current.ok
    val modifier = Modifier.size(16.dp)
    when (status) {
        StatusKind.Ready -> Canvas(modifier) { drawCircle(ok, radius = 5.dp.toPx()) }
        StatusKind.Connecting, StatusKind.CheckingKey -> SpinningArc(muted, modifier)
        StatusKind.Offline -> Canvas(modifier) { drawCircle(muted, radius = 4.5.dp.toPx(), style = Stroke(1.5.dp.toPx())) }
        StatusKind.SwayOffline -> Canvas(modifier) {
            val inset = 3.dp.toPx()
            val stroke = 1.5.dp.toPx()
            val far = size.width - inset
            drawRect(muted, topLeft = Offset(inset, inset), size = Size(far - inset, far - inset), style = Stroke(stroke))
            drawLine(muted, Offset(inset, inset), Offset(far, far), stroke)
            drawLine(muted, Offset(far, inset), Offset(inset, far), stroke)
        }
        StatusKind.Locked -> Icon(SwayIcons.Lock.filled, contentDescription = null, tint = muted, modifier = modifier)
        StatusKind.ScreensOff -> Icon(SwayIcons.Moon.filled, contentDescription = null, tint = muted, modifier = modifier)
        StatusKind.ReadOnly -> Icon(SwayIcons.Visibility.outline, contentDescription = null, tint = muted, modifier = modifier)
        StatusKind.NotPaired, StatusKind.WrongKey, StatusKind.Refused ->
            Icon(SwayIcons.Warning.filled, contentDescription = null, tint = scheme.error, modifier = modifier)
    }
}

/** A quarter-open arc that turns once a second; it stands still under reduce motion. */
@Composable
private fun SpinningArc(color: Color, modifier: Modifier) {
    val reduceMotion = LocalReduceMotion.current
    val angle = if (reduceMotion) {
        0f
    } else {
        val transition = rememberInfiniteTransition(label = "statusArc")
        val turning by transition.animateFloat(
            initialValue = 0f,
            targetValue = 360f,
            animationSpec = infiniteRepeatable(tween(ARC_TURN_MS, easing = LinearEasing), RepeatMode.Restart),
            label = "statusArcAngle",
        )
        turning
    }
    Canvas(modifier) {
        val stroke = 1.5.dp.toPx()
        val inset = 2.5.dp.toPx()
        rotate(angle) {
            drawArc(
                color = color,
                startAngle = 0f,
                sweepAngle = 270f,
                useCenter = false,
                topLeft = Offset(inset, inset),
                size = Size(size.width - 2 * inset, size.height - 2 * inset),
                style = Stroke(stroke),
            )
        }
    }
}
