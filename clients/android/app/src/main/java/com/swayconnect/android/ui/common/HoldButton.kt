package com.swayconnect.android.ui.common

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import kotlinx.coroutines.launch

/** The hold of a destructive or armed control (docs/UX.md 12: "press 400 or 600ms"). */
const val HOLD_MS = 600

private const val DISABLED_ALPHA = 0.38f
private const val FULL_TURN = 360f
private const val TOP = -90f

/**
 * A button that acts only after a press held for [holdMs] (docs/UX.md 12, 14, 15, 17.8). A ring around the icon
 * fills while the finger stays down. An early release, or a scroll that takes the gesture, cancels with `Reject`;
 * a full hold fires `GestureThresholdActivate` and [onHeld].
 *
 * TalkBack, Switch Access and a hardware keyboard cannot hold: their activation calls [onConfirmRequest], which
 * opens a confirm dialog instead (UX 15). So the caller always offers a second, explicit way to say yes.
 *
 * @param spoken what TalkBack says for the button; [label] is the visible word.
 * @param color text, icon, ring and 1dp edge (`error` for a destructive control: 3:1 on card and sheet, UX 15).
 */
@Composable
fun HoldButton(
    label: String,
    icon: ImageVector,
    onHeld: () -> Unit,
    onConfirmRequest: () -> Unit,
    modifier: Modifier = Modifier,
    spoken: String = label,
    iconOnly: Boolean = false,
    enabled: Boolean = true,
    color: Color = MaterialTheme.colorScheme.onSurface,
    holdMs: Int = HOLD_MS,
) {
    val haptics = LocalHaptics.current
    val reduceMotion = LocalReduceMotion.current
    val scope = rememberCoroutineScope()
    val progress = remember { Animatable(0f) }
    val held = rememberUpdatedState(onHeld)
    val confirm = rememberUpdatedState(onConfirmRequest)
    val confirmLabel = stringResource(R.string.hold_confirm_action)
    val shown = if (enabled) color else color.copy(alpha = DISABLED_ALPHA)
    Surface(
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, shown),
        modifier = modifier
            .heightIn(min = 48.dp)
            .semantics(mergeDescendants = true) {
                role = Role.Button
                contentDescription = spoken
                if (enabled) onClick(confirmLabel) { confirm.value(); true } else disabled()
            }
            .focusable(enabled)
            .onKeyEvent { event ->
                val activates = event.key == Key.Enter || event.key == Key.NumPadEnter || event.key == Key.Spacebar
                if (enabled && activates && event.type == KeyEventType.KeyUp) confirm.value()
                enabled && activates
            }
            .pointerInput(enabled, holdMs, haptics, reduceMotion) {
                if (!enabled) return@pointerInput
                awaitEachGesture {
                    awaitFirstDown().consume()
                    val filling = scope.launch { progress.animateTo(1f, tween(holdMs, easing = LinearEasing)) }
                    // Null: the hold time passed with the finger still down. True: the finger left, or a scroll
                    // consumed the gesture, before that.
                    val endedEarly = withTimeoutOrNull(holdMs.toLong()) {
                        waitForUpOrCancellation()?.consume()
                        true
                    }
                    filling.cancel()
                    if (endedEarly == null) {
                        scope.launch { progress.snapTo(1f) }
                        haptics.perform(SwayHaptic.GestureThresholdActivate)
                        held.value()
                        waitForUpOrCancellation()
                        scope.launch { progress.snapTo(0f) }
                    } else {
                        haptics.perform(SwayHaptic.Reject)
                        scope.launch { if (reduceMotion) progress.snapTo(0f) else progress.animateTo(0f, tween(holdMs / 3)) }
                    }
                }
            },
    ) {
        Row(
            modifier = Modifier.padding(horizontal = if (iconOnly) 12.dp else 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier.size(24.dp).drawBehind {
                    val stroke = 2.dp.toPx()
                    drawArc(
                        color = shown,
                        startAngle = TOP,
                        sweepAngle = FULL_TURN * progress.value,
                        useCenter = false,
                        topLeft = Offset(stroke / 2, stroke / 2),
                        size = Size(size.width - stroke, size.height - stroke),
                        style = Stroke(stroke),
                    )
                },
                contentAlignment = Alignment.Center,
            ) {
                Icon(imageVector = icon, contentDescription = null, tint = shown, modifier = Modifier.size(16.dp))
            }
            if (!iconOnly) Text(label, color = shown, style = MaterialTheme.typography.labelLarge, maxLines = 1)
        }
    }
}
