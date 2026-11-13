package com.swayconnect.android.ui.pair

import android.content.ClipData
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.PairTarget
import com.swayconnect.android.ui.theme.tabular
import kotlinx.coroutines.launch

/** A white card on the greige background (UX 3.1, shape `large`). */
@Composable
internal fun PairCard(content: @Composable ColumnScope.() -> Unit) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surface) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
internal fun CardHeading(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

/** A refusal or problem line. Polite live region: it appears after a tap. */
@Composable
internal fun ProblemText(text: String) {
    Text(
        text,
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
}

/** Full-width ink button (UX 11 step 2). */
@Composable
internal fun InkButton(text: String, onClick: () -> Unit, enabled: Boolean = true) {
    Button(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
        Text(text, style = MaterialTheme.typography.labelLarge)
    }
}

/** Step 1 (UX 11): what to run on the computer, and "Paste link". */
@Composable
internal fun WelcomeCard(error: String?, serverName: String?, onLink: (String) -> Unit, onBack: () -> Unit) {
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    var clipboardEmpty by remember { mutableStateOf(false) }
    PairCard {
        Text(stringResource(R.string.pair_welcome), style = MaterialTheme.typography.titleMedium)
        Text(stringResource(R.string.pair_run_on_computer), style = MaterialTheme.typography.bodyMedium)
        CommandPill(command = stringResource(R.string.pair_command), onCopy = { command ->
            scope.launch { clipboard.setClipEntry(ClipEntry(ClipData.newPlainText(command, command))) }
        })
        Text(
            stringResource(R.string.pair_scan_hint),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        InkButton(stringResource(R.string.pair_paste_link), onClick = {
            scope.launch {
                // Plain text only: a link copied from a terminal or a chat is text. Nothing connects here.
                val text = clipboard.getClipEntry()?.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
                clipboardEmpty = text.isNullOrBlank()
                if (!clipboardEmpty) onLink(text.toString())
            }
        })
        if (clipboardEmpty) ProblemText(stringResource(R.string.pair_clipboard_empty)) else error?.let { ProblemText(it) }
        serverName?.let { name ->
            TextButton(onClick = onBack, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(R.string.pair_back_to_desktop, name))
            }
        }
    }
}

/** The command in a mono pill with Copy (UX 11 step 1). Selectable, so it can also be copied by hand. */
@Composable
private fun CommandPill(command: String, onCopy: (String) -> Unit) {
    val copyLabel = stringResource(R.string.pair_copy_command)
    Surface(shape = CircleShape, color = MaterialTheme.colorScheme.surfaceContainer) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 20.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SelectionContainer(modifier = Modifier.weight(1f)) {
                Text(
                    command,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
            TextButton(
                onClick = { onCopy(command) },
                modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = copyLabel },
            ) {
                Text(stringResource(R.string.pair_copy))
            }
        }
    }
}

/** Step 2 (UX 11): the computer named by the link, the device name and "Pair". */
@Composable
internal fun TargetCard(
    target: PairTarget,
    deviceName: String,
    replaces: String?,
    error: String?,
    onDeviceName: (String) -> Unit,
    onPair: () -> Unit,
    onOtherLink: () -> Unit,
) {
    PairCard {
        CardHeading(stringResource(R.string.pair_target_heading))
        target.name?.let { TargetLine(stringResource(R.string.pair_target_name), it) }
        TargetLine(stringResource(R.string.pair_target_host), target.host)
        TargetLine(stringResource(R.string.pair_target_port), target.port.toString())
        FingerprintLine(target.shortFingerprint)
        DeviceNameField(deviceName, onDeviceName)
        replaces?.let { Text(stringResource(R.string.pair_replaces, it), style = MaterialTheme.typography.bodyMedium) }
        error?.let { ProblemText(it) }
        InkButton(stringResource(R.string.pair_button), onClick = onPair)
        TextButton(onClick = onOtherLink, modifier = Modifier.heightIn(min = 48.dp)) {
            Text(stringResource(R.string.pair_other_link))
        }
    }
}

@Composable
private fun TargetLine(label: String, value: String) {
    // One node: TalkBack reads "Host, 192.0.2.1".
    Column(modifier = Modifier.semantics(mergeDescendants = true) {}) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyLarge)
    }
}

/** Four groups of four hex digits (the first 8 bytes), mono and tabular, as the computer prints them. */
@Composable
private fun FingerprintLine(shortFingerprint: String) {
    Column(modifier = Modifier.semantics(mergeDescendants = true) {}) {
        Text(
            stringResource(R.string.pair_target_fingerprint),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            shortFingerprint,
            style = MaterialTheme.typography.titleMedium.tabular(),
            fontFamily = FontFamily.Monospace,
        )
        Text(
            stringResource(R.string.pair_target_fingerprint_hint),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * The text is kept in the composition while the user types: a field that reads its value back from
 * a StateFlow can lose characters. The view model only receives the edits.
 */
@Composable
private fun DeviceNameField(initialName: String, onEdited: (String) -> Unit) {
    var name by remember { mutableStateOf(initialName) }
    OutlinedTextField(
        value = name,
        onValueChange = {
            name = it
            onEdited(it)
        },
        label = { Text(stringResource(R.string.pair_device_name)) },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
        shape = MaterialTheme.shapes.small,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
    )
}
