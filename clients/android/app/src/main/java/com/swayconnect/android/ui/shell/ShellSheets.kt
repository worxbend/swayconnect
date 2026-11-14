package com.swayconnect.android.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.clickable
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.LockedFeatureContent
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.more.idleInhibitSentence
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.KeepAwake

/** What the shell's own sheets can ask for. The area sheets (window, workspace, ...) belong to their areas. */
internal class ShellSheetActions(
    val onClose: () -> Unit,
    val onShowLocked: (String) -> Unit,
    val onRetry: () -> Unit,
    val onForget: () -> Unit,
)

/** Renders [UiState.sheet] when it is one of the shell's kinds: Computer (UX 10.6) or Locked (UX 10.4). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ShellSheet(state: UiState, actions: ShellSheetActions) {
    val sheet = state.sheet
    if (sheet !is Sheet.Computer && sheet !is Sheet.Locked) return
    ModalBottomSheet(onDismissRequest = actions.onClose, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(
            modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when (sheet) {
                is Sheet.Locked -> LockedContent(sheet.scope, state.server?.devicePrefix, state.session?.state?.keepAwake)
                else -> ComputerContent(state, actions)
            }
        }
    }
}

@Composable
private fun LockedContent(scope: String, devicePrefix: String?, keepAwake: KeepAwake?) {
    val sentence = if (scope == IDLE_INHIBIT) idleInhibitSentence(keepAwake) else stringResource(scopeSentence(scope))
    LockedFeatureContent(scope, devicePrefix, sentence)
}

/** Its sentence names the server's keep-awake limits from topic `session` (UX 10.5). */
private const val IDLE_INHIBIT = "idle.inhibit"

/** One honest sentence per scope (UX 10.5). */
private fun scopeSentence(scope: String): Int =
    when (scope) {
        "control" -> R.string.scope_control
        "audio" -> R.string.scope_audio
        "audio.capture" -> R.string.scope_audio_capture
        "media" -> R.string.scope_media
        IDLE_INHIBIT -> R.string.scope_idle_inhibit
        "launch" -> R.string.scope_launch
        "launch.risky" -> R.string.scope_launch_risky
        "input.text", "input.keys" -> R.string.scope_input
        "destructive" -> R.string.scope_destructive
        else -> R.string.scope_other
    }

@Composable
private fun ComputerContent(state: UiState, actions: ShellSheetActions) {
    var confirmingForget by rememberSaveable { mutableStateOf(false) }
    val server = state.server
    val granted = state.granted
    server?.let {
        Text(it.name, style = MaterialTheme.typography.titleMedium)
        Text(stringResource(R.string.computer_fingerprint, it.shortFingerprint), style = MaterialTheme.typography.bodyMedium)
    }
    if (granted == null) {
        Text(stringResource(R.string.computer_not_connected), style = MaterialTheme.typography.bodyMedium)
    } else {
        Text(stringResource(R.string.computer_granted), style = MaterialTheme.typography.labelLarge)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (granted.scopes.isEmpty()) Text(stringResource(R.string.computer_none))
            for (scope in granted.scopes) StatePill(text = scope, tone = PillTone.Ink)
        }
        val missing = granted.knownScopes.orEmpty().filter { it !in granted.scopes }
        if (missing.isNotEmpty()) {
            Text(stringResource(R.string.computer_missing), style = MaterialTheme.typography.labelLarge)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (scope in missing) {
                    val spoken = stringResource(R.string.computer_locked_scope, scope)
                    StatePill(
                        text = scope,
                        tone = PillTone.Neutral,
                        icon = SwayIcons.Lock.outline,
                        modifier = Modifier
                            .heightIn(min = 48.dp)
                            .clickable { actions.onShowLocked(scope) }
                            .semantics {
                                role = Role.Button
                                contentDescription = spoken
                            },
                    )
                }
            }
        }
        if (granted.readOnly) Text(stringResource(R.string.computer_read_only), style = MaterialTheme.typography.bodyMedium)
    }
    if (state.connection == ConnectionStatus.Backoff) {
        TextButton(onClick = actions.onRetry) { Text(stringResource(R.string.computer_retry)) }
    }
    if (server != null) {
        TextButton(onClick = { confirmingForget = true }) {
            Text(stringResource(R.string.computer_forget), color = MaterialTheme.colorScheme.error)
        }
    }
    if (confirmingForget) {
        AlertDialog(
            onDismissRequest = { confirmingForget = false },
            confirmButton = {
                TextButton(onClick = {
                    confirmingForget = false
                    actions.onClose()
                    actions.onForget()
                }) { Text(stringResource(R.string.forget_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmingForget = false }) { Text(stringResource(R.string.cancel)) } },
            title = { Text(stringResource(R.string.forget_title)) },
            text = { Text(stringResource(R.string.forget_text)) },
        )
    }
}
