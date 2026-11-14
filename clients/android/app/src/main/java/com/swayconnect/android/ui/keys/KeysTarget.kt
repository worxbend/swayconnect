package com.swayconnect.android.ui.keys

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.state.KeysRefusal
import com.swayconnect.android.ui.state.Typed
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.Window

/** "Type there" and "Back to …" after `focus_changed` (UX 9.2). */
internal class FocusMovedActions(val onTypeThere: () -> Unit, val onBackToTarget: () -> Unit)

/**
 * The target card (UX 9.1): the window captured when Keys opened, or the launcher. After `focus_changed` it gets a
 * 2dp `error` border, the warning, "Sent X of Y" and the two ways on (UX 9.2). [desktop] is already cleaned for display.
 */
@Composable
internal fun TargetCard(state: UiState, desktop: Desktop?, canFocus: Boolean, actions: FocusMovedActions) {
    val keys = state.keys ?: return
    val moved = keys.refusal as? KeysRefusal.FocusChanged
    val scheme = MaterialTheme.colorScheme
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = scheme.surface,
        border = moved?.let { BorderStroke(2.dp, scheme.error) },
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            TargetHeadline(keys.target, desktop)
            if (moved != null) FocusMoved(moved, keys.target, keys.typed, desktop, canFocus, actions)
        }
    }
}

@Composable
private fun TargetHeadline(target: InputTarget?, desktop: Desktop?) {
    when (target) {
        null -> Text(stringResource(R.string.keys_no_target), style = MaterialTheme.typography.bodyLarge)
        InputTarget.Launcher -> Headline(icon = SwayIcons.Search.outline, window = null, title = stringResource(R.string.keys_launcher), workspace = null)
        is InputTarget.Window -> {
            val window = desktop?.windows?.firstOrNull { it.id == target.windowId }
            if (window == null) {
                Text(stringResource(R.string.keys_target_gone), style = MaterialTheme.typography.bodyLarge)
            } else {
                val workspace = desktop?.workspaces?.firstOrNull { it.id == window.workspaceId }?.let { it.number?.toString() ?: it.name }
                Headline(icon = null, window = window, title = window.title ?: windowName(window), workspace = workspace)
                if (isKnownTerminal(window.appId)) StatePill(stringResource(R.string.keys_terminal), PillTone.Coral, icon = SwayIcons.Terminal.outline)
            }
        }
    }
}

@Composable
private fun Headline(icon: ImageVector?, window: Window?, title: String, workspace: String?) {
    Row(
        modifier = Modifier.semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
            if (window != null) AppIcon(appId = window.appId, iconHash = window.icon, size = 28.dp) else if (icon != null) Icon(icon, contentDescription = null)
        }
        Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        if (workspace != null) {
            val spoken = stringResource(R.string.keys_target_workspace_spoken, workspace)
            Text(
                stringResource(R.string.keys_target_workspace, workspace),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.semantics { contentDescription = spoken },
            )
        }
    }
}

@Composable
private fun FocusMoved(moved: KeysRefusal.FocusChanged, target: InputTarget?, typed: Typed?, desktop: Desktop?, canFocus: Boolean, actions: FocusMovedActions) {
    val now = moved.focusedWindowId?.let { id -> desktop?.windows?.firstOrNull { it.id == id } }
    val message = if (moved.focusedWindowId == null) {
        stringResource(R.string.keys_focus_lost)
    } else {
        stringResource(R.string.keys_focus_moved, now?.let { windowName(it) } ?: stringResource(R.string.window_untitled))
    }
    RefusalMessage(message, SwayIcons.Warning.outline, MaterialTheme.colorScheme.error, LiveRegionMode.Assertive)
    SentLine(typed)
    val back = (target as? InputTarget.Window)?.let { t -> desktop?.windows?.firstOrNull { it.id == t.windowId } }
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (moved.focusedWindowId != null) OutlinedButton(onClick = actions.onTypeThere) { Text(stringResource(R.string.keys_type_there)) }
        if (back != null && canFocus) OutlinedButton(onClick = actions.onBackToTarget) { Text(stringResource(R.string.keys_back_to, windowName(back))) }
    }
}

/**
 * The other refusals of UX 9.2 as one full-width card: locked (also while `session` says so, which disables the
 * composer), a keyboard grab, a closed launcher.
 */
@Composable
internal fun RefusalCard(state: UiState, onDismiss: () -> Unit) {
    val keys = state.keys ?: return
    val refusal = keys.refusal
    val sessionLocked = state.session?.state?.locked == true
    val (text, icon) = when {
        keysLocked(state) -> R.string.keys_locked to SwayIcons.Lock.outline
        refusal == KeysRefusal.KeyboardGrabbed -> R.string.keys_grabbed to SwayIcons.Warning.outline
        refusal == KeysRefusal.LauncherClosed -> R.string.keys_launcher_closed to SwayIcons.Warning.outline
        else -> return
    }
    Surface(shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            RefusalMessage(stringResource(text), icon, MaterialTheme.colorScheme.onSurface, LiveRegionMode.Polite)
            SentLine(keys.typed)
            // A known lock clears itself with the next `session` state; a refusal alone needs a way back.
            if (refusal != null && !sessionLocked) {
                val label = if (refusal == KeysRefusal.Locked) R.string.keys_try_again else R.string.keys_dismiss
                TextButton(onClick = onDismiss) { Text(stringResource(label)) }
            }
        }
    }
}

@Composable
private fun RefusalMessage(text: String, icon: ImageVector, tint: Color, live: LiveRegionMode) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, tint = tint)
        Text(text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.semantics { liveRegion = live })
    }
}

/** "Sent 16 of 40 characters." when a send stopped early; the unsent tail stays in the field. */
@Composable
internal fun SentLine(typed: Typed?) {
    if (typed == null || typed.deliveredChars >= typed.totalChars) return
    val total = typed.totalChars.toInt()
    Text(
        pluralStringResource(R.plurals.keys_sent_of, total, typed.deliveredChars, typed.totalChars),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

/** The picker of UX 9.2: one chip per other target. */
@Composable
internal fun TargetPicker(state: UiState, desktop: Desktop?, onPick: (InputTarget) -> Unit) {
    val choices = targetChoices(state)
    if (choices.isEmpty()) return
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (choice in choices) {
            val name = when (choice) {
                InputTarget.Launcher -> stringResource(R.string.keys_launcher)
                is InputTarget.Window -> desktop?.windows?.firstOrNull { it.id == choice.windowId }?.let { windowName(it) } ?: stringResource(R.string.window_untitled)
            }
            OutlinedButton(onClick = { onPick(choice) }) { Text(stringResource(R.string.keys_type_into, name)) }
        }
    }
}

/** The app's name as UX 9.2 says it ("Focus moved to Firefox."): the app id, else the title. */
@Composable
private fun windowName(window: Window): String = window.appId ?: window.title ?: stringResource(R.string.window_untitled)
