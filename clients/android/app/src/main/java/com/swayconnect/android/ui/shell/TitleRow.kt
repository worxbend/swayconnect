package com.swayconnect.android.ui.shell

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.clickable
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayIcons

/**
 * The title row of docs/UX.md 4.1 (reference 2): the title with the status line tight under it, and the context
 * pill and the round button centred against that pair. Grows with the font scale; never shorter than 72dp.
 * [gutter] holds the start and end edges shared with the page below, so everything hangs on one line.
 * TalkBack order (UX 15): title, status line, context pill, round button.
 */
@Composable
internal fun TitleRow(
    title: String,
    status: StatusKind,
    drivePaused: Boolean,
    gutter: PaddingValues,
    onStatusClick: () -> Unit,
    contextPill: @Composable () -> Unit,
    roundButton: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth().heightIn(min = 72.dp).padding(gutter),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                text = title,
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.onBackground,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading() },
            )
            StatusLine(status, drivePaused, onStatusClick)
        }
        contextPill()
        roundButton()
    }
}

/**
 * Dot + word, then the state pills. Only the dot + word node is a polite live region (UX 15): it changes only
 * on a transition, so only transitions are announced. Pills sit in their own nodes, outside it. The row is about
 * 24dp tall; Compose widens the touch area of a clickable that small to the 48dp minimum.
 */
@Composable
private fun StatusLine(status: StatusKind, drivePaused: Boolean, onClick: () -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
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
        if (drivePaused) StatePill(text = stringResource(R.string.status_drive_paused), tone = PillTone.Neutral)
    }
}

private val StatusKind.word: Int
    get() = when (this) {
        StatusKind.Ready -> R.string.status_ready
        StatusKind.Connecting -> R.string.status_connecting
        StatusKind.CheckingKey -> R.string.status_checking_key
        StatusKind.Offline -> R.string.status_offline
        StatusKind.SwayOffline -> R.string.status_sway_offline
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
