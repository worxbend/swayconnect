package com.swayconnect.android.ui.launch

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
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
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.common.HOLD_MS
import com.swayconnect.android.ui.theme.Haptics
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.App
import com.swayconnect.protocol.replaceUnsafe
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

private val BADGE = 56.dp
private val ICON = 36.dp
private val MARK = 14.dp
private const val FULL_TURN = 360f
private const val TOP = -90f
private const val DISABLED_ALPHA = 0.38f

/** What a cell can ask for. The area decides the target workspace of a plain start. */
internal class CellActions(
    val onStart: (App) -> Unit,
    val onStartOn: (App, number: Int) -> Unit,
    /** TalkBack, Switch Access or a keyboard on a risky app: they cannot hold, so a dialog asks (UX 15). */
    val onConfirmRequest: (App) -> Unit,
    val onLocked: (String) -> Unit,
)

/**
 * One app of the grid (docs/UX.md 8.2): a 56dp round badge and a two-line label. A risky app carries a 14dp
 * shield and starts after a 600 ms hold; without `launch.risky` it shows a lock and opens the Locked sheet. A long
 * press on any other app offers "Open on workspace" with [targets].
 */
@Composable
internal fun AppCellView(cell: AppCell, targets: List<Int>, actions: CellActions, modifier: Modifier = Modifier) {
    val name = replaceUnsafe(cell.app.name)
    val progress = remember { Animatable(0f) }
    val hold = rememberCellHold(progress, onHeld = { actions.onStart(cell.app) }, onConfirmRequest = { actions.onConfirmRequest(cell.app) })
    val startLabel = stringResource(R.string.launch_start)
    var menu by remember { mutableStateOf(false) }
    val gesture = when (val tap = cell.tap()) {
        LaunchTap.Send -> Modifier.combinedClickable(
            onClickLabel = startLabel,
            onLongClickLabel = stringResource(R.string.launch_open_on).takeIf { targets.isNotEmpty() },
            onLongClick = if (targets.isNotEmpty()) ({ menu = true }) else null,
            role = Role.Button,
        ) { actions.onStart(cell.app) }
        LaunchTap.Hold -> Modifier.holdToStart(hold, startLabel)
        is LaunchTap.ShowLocked -> Modifier.clickable(role = Role.Button) { actions.onLocked(tap.scope) }
        LaunchTap.None -> Modifier.semantics { disabled() }
    }
    val state = cellState(cell)
    // Read-only: the cell looks disabled, as the More tiles do (UX 4.1).
    val dimmed = cell.looksDisabled()
    Box(modifier) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .alpha(if (dimmed) DISABLED_ALPHA else 1f)
                .clip(MaterialTheme.shapes.medium)
                .then(gesture)
                // The label is the spoken name; the state adds risky, locked or starting.
                .semantics(mergeDescendants = true) { if (state != null) stateDescription = state }
                .padding(vertical = 8.dp, horizontal = 4.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Badge(cell, name) { progress.value }
            Text(
                text = name,
                style = MaterialTheme.typography.labelMedium,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        OpenOnMenu(menu, targets, onDismiss = { menu = false }) { number ->
            menu = false
            actions.onStartOn(cell.app, number)
        }
    }
}

@Composable
private fun cellState(cell: AppCell): String? =
    when {
        cell.pending -> stringResource(R.string.launch_state_starting)
        cell.tap() is LaunchTap.ShowLocked -> stringResource(R.string.launch_state_locked)
        cell.tap() == LaunchTap.Hold -> stringResource(R.string.launch_state_risky)
        else -> null
    }

/**
 * The round badge with the app icon; the hold ring, the spinner, and the shield or lock mark sit on it. The ring
 * reads [holdProgress] in the draw phase, so a hold redraws without recomposing.
 */
@Composable
private fun Badge(cell: AppCell, name: String, holdProgress: () -> Float) {
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier = Modifier
            .size(BADGE)
            .background(scheme.surfaceContainer, CircleShape)
            .drawBehind {
                val turned = holdProgress()
                if (turned <= 0f) return@drawBehind
                val stroke = 3.dp.toPx()
                drawArc(
                    color = scheme.onSurface,
                    startAngle = TOP,
                    sweepAngle = FULL_TURN * turned,
                    useCenter = false,
                    topLeft = Offset(stroke / 2, stroke / 2),
                    size = Size(size.width - stroke, size.height - stroke),
                    style = Stroke(stroke),
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        AppIcon(appId = name, iconHash = cell.app.icon, size = ICON)
        if (cell.pending) CircularProgressIndicator(Modifier.size(BADGE), strokeWidth = 2.dp)
        val mark = markOf(cell)
        if (mark != null) {
            Box(
                Modifier.align(Alignment.BottomEnd).offset(x = 2.dp, y = 2.dp).size(MARK + 4.dp).background(scheme.surface, CircleShape),
                contentAlignment = Alignment.Center,
            ) {
                Icon(imageVector = mark, contentDescription = null, tint = scheme.onSurface, modifier = Modifier.size(MARK))
            }
        }
    }
}

/** A lock without `launch.risky`, else the shield of a risky app (UX 8.2). */
private fun markOf(cell: AppCell): ImageVector? =
    when {
        cell.tap() is LaunchTap.ShowLocked -> SwayIcons.Lock.filled
        cell.app.risky -> SwayIcons.Shield.outline
        else -> null
    }

/** The inputs of one cell's hold, kept across recompositions so a new state does not cancel a running hold. */
private class CellHold(
    val progress: Animatable<Float, AnimationVector1D>,
    val scope: CoroutineScope,
    val haptics: Haptics,
    val reduceMotion: Boolean,
    val onHeld: State<() -> Unit>,
    val onConfirmRequest: State<() -> Unit>,
)

@Composable
private fun rememberCellHold(progress: Animatable<Float, AnimationVector1D>, onHeld: () -> Unit, onConfirmRequest: () -> Unit): CellHold {
    val haptics = LocalHaptics.current
    val reduceMotion = LocalReduceMotion.current
    val scope = rememberCoroutineScope()
    val held = rememberUpdatedState(onHeld)
    val confirm = rememberUpdatedState(onConfirmRequest)
    return remember(progress, scope, haptics, reduceMotion) { CellHold(progress, scope, haptics, reduceMotion, held, confirm) }
}

/**
 * The 600 ms hold of a risky app (UX 8.2, 12, 14), drawn as the badge ring by its progress. An early release
 * cancels with `Reject`; a full hold fires `GestureThresholdActivate`. Semantics and keyboard activation ([label])
 * ask for the confirm dialog instead (UX 15). The same gesture as `HoldButton`, on a grid cell.
 */
private fun Modifier.holdToStart(hold: CellHold, label: String): Modifier =
    semantics {
        role = Role.Button
        onClick(label) { hold.onConfirmRequest.value(); true }
    }
        .focusable()
        .onKeyEvent { event ->
            val activates = event.key == Key.Enter || event.key == Key.NumPadEnter || event.key == Key.Spacebar
            if (activates && event.type == KeyEventType.KeyUp) hold.onConfirmRequest.value()
            activates
        }
        .pointerInput(hold) {
            val progress = hold.progress
            awaitEachGesture {
                awaitFirstDown().consume()
                val filling = hold.scope.launch { progress.animateTo(1f, tween(HOLD_MS, easing = LinearEasing)) }
                // Null: the hold time passed with the finger still down.
                val endedEarly = withTimeoutOrNull(HOLD_MS.toLong()) {
                    waitForUpOrCancellation()?.consume()
                    true
                }
                filling.cancel()
                if (endedEarly == null) {
                    // The ring clears before the start: the cell's spinner takes over, and the gesture may end
                    // with the cell disabled before this coroutine could clear it.
                    hold.scope.launch { progress.snapTo(0f) }
                    hold.haptics.perform(SwayHaptic.GestureThresholdActivate)
                    hold.onHeld.value()
                    waitForUpOrCancellation()
                } else {
                    hold.haptics.perform(SwayHaptic.Reject)
                    hold.scope.launch { if (hold.reduceMotion) progress.snapTo(0f) else progress.animateTo(0f, tween(HOLD_MS / 3)) }
                }
            }
        }

/** The long-press menu: "Open on workspace", then one row per workspace number. */
@Composable
private fun OpenOnMenu(expanded: Boolean, targets: List<Int>, onDismiss: () -> Unit, onPick: (Int) -> Unit) {
    DropdownMenu(expanded = expanded && targets.isNotEmpty(), onDismissRequest = onDismiss) {
        DropdownMenuItem(text = { Text(stringResource(R.string.launch_open_on)) }, onClick = {}, enabled = false)
        for (number in targets) {
            DropdownMenuItem(text = { Text(stringResource(R.string.launch_on, number)) }, onClick = { onPick(number) })
        }
    }
}
