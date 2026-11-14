package com.swayconnect.android.ui.more

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.ui.common.HOLD_MS
import com.swayconnect.android.ui.theme.Haptics
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayIcon
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

private val TILE_MIN_HEIGHT = 88.dp
private val BADGE = 40.dp
private const val DISABLED_ALPHA = 0.38f
private const val STALE_ALPHA = 0.6f
private const val BADGE_ON_INK_ALPHA = 0.16f
private const val FULL_TURN = 360f
private const val TOP = -90f

/** What a tile shows. [on] fills it with ink (UX 10.1); the first line then takes `accentOnInk`. */
internal class TileLook(
    val icon: SwayIcon,
    val title: String,
    val lines: List<String>,
    val on: Boolean = false,
    /** The scope is missing: a lock replaces the icon (UX 10.2 last row). */
    val locked: Boolean = false,
    val pending: Boolean = false,
)

/**
 * What TalkBack says. The tile is one node: [title] and [state] are spoken, the visible lines are not, so a
 * per-second countdown is never read out (UX 10.2, 15). [toggled] makes it a switch.
 */
internal class TileSpeech(
    val state: String?,
    val clickLabel: String?,
    val longClickLabel: String? = null,
    val toggled: Boolean? = null,
)

/**
 * How the tile takes input. [enabled] false shows it disabled; [stale] dims it as a state no connection
 * confirms (UX 13). A null [onClick] takes no tap. A [hold] replaces the tap and the long press.
 */
internal class TileInput(
    val onClick: (() -> Unit)?,
    val onLongClick: (() -> Unit)? = null,
    val enabled: Boolean = true,
    val stale: Boolean = false,
    val hold: TileHold? = null,
)

/**
 * A 600 ms hold instead of a tap (UX 7.2, 12): the badge ring fills while the finger stays down. TalkBack, Switch
 * Access and a keyboard cannot hold; their activation calls [onConfirmRequest], which asks in a dialog (UX 15).
 */
internal class TileHold(val onHeld: () -> Unit, val onConfirmRequest: () -> Unit)

/** One quick-settings tile of docs/UX.md 10.1: 88dp or taller, an icon badge, a title and its lines. */
@Composable
internal fun QuickTile(look: TileLook, speech: TileSpeech, input: TileInput, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val effects = LocalSwayMotion.current.defaultEffectsSpec<Color>()
    val container by animateColorAsState(if (look.on) scheme.primary else scheme.surface, effects, label = "tile")
    val content by animateColorAsState(if (look.on) scheme.onPrimary else scheme.onSurface, effects, label = "tileContent")
    val alpha = when {
        !input.enabled -> DISABLED_ALPHA
        input.stale -> STALE_ALPHA
        else -> 1f
    }
    val clickable = input.onClick
    val progress = remember { Animatable(0f) }
    val hold = input.hold
    val holding = rememberHolding(progress, hold)
    val interactive = if (hold != null && input.enabled) {
        Modifier.holdToAct(holding, speech.clickLabel)
    } else if (clickable != null) {
        Modifier.combinedClickable(
            enabled = input.enabled,
            role = if (speech.toggled != null) Role.Switch else Role.Button,
            onClickLabel = speech.clickLabel,
            onLongClickLabel = speech.longClickLabel,
            onLongClick = input.onLongClick,
            indication = ripple(),
            interactionSource = null,
            onClick = clickable,
        )
    } else {
        Modifier
    }
    Row(
        modifier = modifier
            .heightIn(min = TILE_MIN_HEIGHT)
            .alpha(alpha)
            .clip(MaterialTheme.shapes.medium)
            .background(container)
            .then(interactive)
            .semantics {
                contentDescription = look.title
                speech.state?.let { stateDescription = it }
                speech.toggled?.let { toggleableState = ToggleableState(it) }
            }
            .clearAndSetSemantics {}
            .padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TileBadge(look, content) { progress.value }
        Column(Modifier.weight(1f).fillMaxHeight(), verticalArrangement = Arrangement.Center) {
            Text(look.title, style = MaterialTheme.typography.titleMedium, color = content, maxLines = 2, overflow = TextOverflow.Ellipsis)
            look.lines.forEachIndexed { index, line ->
                val accent = look.on && index == 0
                Text(
                    text = line,
                    style = MaterialTheme.typography.bodyMedium,
                    color = when {
                        accent -> LocalSwayTokens.current.accentOnInk
                        look.on -> content
                        else -> scheme.onSurfaceVariant
                    },
                )
            }
        }
    }
}

/** The round badge; the hold ring reads [holdProgress] in the draw phase, so a hold redraws without recomposing. */
@Composable
private fun TileBadge(look: TileLook, content: Color, holdProgress: () -> Float) {
    val badge = if (look.on) content.copy(alpha = BADGE_ON_INK_ALPHA) else MaterialTheme.colorScheme.surfaceContainer
    val ring = Modifier.drawBehind {
        val stroke = 2.dp.toPx()
        drawArc(
            color = content,
            startAngle = TOP,
            sweepAngle = FULL_TURN * holdProgress(),
            useCenter = false,
            topLeft = Offset(stroke / 2, stroke / 2),
            size = Size(size.width - stroke, size.height - stroke),
            style = Stroke(stroke),
        )
    }
    Box(Modifier.size(BADGE).background(badge, CircleShape).then(ring), contentAlignment = Alignment.Center) {
        when {
            look.pending -> CircularProgressIndicator(modifier = Modifier.size(20.dp), color = content, strokeWidth = 2.dp)
            look.locked -> Icon(SwayIcons.Lock.outline, contentDescription = null, tint = content, modifier = Modifier.size(22.dp))
            else -> Icon(if (look.on) look.icon.filled else look.icon.outline, contentDescription = null, tint = content, modifier = Modifier.size(22.dp))
        }
    }
}

/** What a running hold needs, kept across recompositions so a new state does not cancel it. */
private class Holding(
    val progress: Animatable<Float, AnimationVector1D>,
    val scope: CoroutineScope,
    val haptics: Haptics,
    val reduceMotion: Boolean,
    val onHeld: State<() -> Unit>,
    val onConfirmRequest: State<() -> Unit>,
)

@Composable
private fun rememberHolding(progress: Animatable<Float, AnimationVector1D>, hold: TileHold?): Holding {
    val haptics = LocalHaptics.current
    val reduceMotion = LocalReduceMotion.current
    val scope = rememberCoroutineScope()
    val held = rememberUpdatedState(hold?.onHeld ?: {})
    val confirm = rememberUpdatedState(hold?.onConfirmRequest ?: {})
    return remember(progress, scope, haptics, reduceMotion) { Holding(progress, scope, haptics, reduceMotion, held, confirm) }
}

/**
 * The hold gesture of `HoldButton` on a whole tile (UX 12, 14): an early release or a scroll cancels with
 * `Reject`, a full hold fires `GestureThresholdActivate` and acts. Semantics and keyboard ask for the dialog.
 */
private fun Modifier.holdToAct(holding: Holding, label: String?): Modifier =
    semantics {
        role = Role.Button
        onClick(label) { holding.onConfirmRequest.value(); true }
    }
        .focusable()
        .onKeyEvent { event ->
            val activates = event.key == Key.Enter || event.key == Key.NumPadEnter || event.key == Key.Spacebar
            if (activates && event.type == KeyEventType.KeyUp) holding.onConfirmRequest.value()
            activates
        }
        .pointerInput(holding) {
            val progress = holding.progress
            awaitEachGesture {
                awaitFirstDown().consume()
                val filling = holding.scope.launch { progress.animateTo(1f, tween(HOLD_MS, easing = LinearEasing)) }
                // Null: the hold time passed with the finger still down.
                val endedEarly = withTimeoutOrNull(HOLD_MS.toLong()) {
                    waitForUpOrCancellation()?.consume()
                    true
                }
                filling.cancel()
                if (endedEarly == null) {
                    // Cleared before acting: the badge spinner takes over while the answer is awaited.
                    holding.scope.launch { progress.snapTo(0f) }
                    holding.haptics.perform(SwayHaptic.GestureThresholdActivate)
                    holding.onHeld.value()
                    waitForUpOrCancellation()
                } else {
                    holding.haptics.perform(SwayHaptic.Reject)
                    holding.scope.launch { if (holding.reduceMotion) progress.snapTo(0f) else progress.animateTo(0f, tween(HOLD_MS / 3)) }
                }
            }
        }
