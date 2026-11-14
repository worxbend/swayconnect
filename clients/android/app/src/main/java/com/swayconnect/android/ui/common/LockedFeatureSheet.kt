package com.swayconnect.android.ui.common

import android.content.ClipData
import android.content.Intent
import androidx.annotation.StringRes
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.SwayIcons
import kotlinx.coroutines.launch

/**
 * The body of the Locked feature sheet (docs/UX.md 10.4): "<feature> is off for this phone", the scope's one
 * honest sentence ([sentence], from the table of UX 10.5), the command the owner runs at the computer (mono,
 * selectable), Copy and Share, and that the phone reconnects by itself. [scope] is a plain scope name
 * (`Refusal` checks the server's); [devicePrefix] null prints `<id>` and says where to find it.
 */
@Composable
fun LockedFeatureContent(scope: String, devicePrefix: String?, sentence: String) {
    val command = if (devicePrefix != null) {
        stringResource(R.string.locked_command, devicePrefix, scope)
    } else {
        stringResource(R.string.locked_command_unknown_id, scope)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(SwayIcons.Lock.outline, contentDescription = null, modifier = Modifier.size(20.dp))
        Text(
            text = stringResource(lockedTitle(scope)),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.semantics { heading() },
        )
    }
    Text(sentence, style = MaterialTheme.typography.bodyMedium)
    Text(stringResource(R.string.locked_run), style = MaterialTheme.typography.bodyMedium)
    SelectionContainer {
        Text(
            text = command,
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(12.dp))
                .padding(12.dp),
        )
    }
    CommandButtons(command)
    if (devicePrefix == null) {
        Text(stringResource(R.string.locked_device_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    Text(stringResource(R.string.locked_reconnects), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/** Copy puts the command on the clipboard; Share hands it to another app (a chat with the computer's owner). */
@Composable
private fun CommandButtons(command: String) {
    val clipboard = LocalClipboard.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var copied by rememberSaveable(command) { mutableStateOf(false) }
    val shareTitle = stringResource(R.string.locked_share_title)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        CommandButton(if (copied) R.string.locked_copied else R.string.locked_copy) {
            scope.launch {
                clipboard.setClipEntry(ClipEntry(ClipData.newPlainText(shareTitle, command)))
                copied = true
            }
        }
        CommandButton(R.string.locked_share) {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, command)
            context.startActivity(Intent.createChooser(send, shareTitle))
        }
    }
}

@Composable
private fun CommandButton(@StringRes label: Int, onClick: () -> Unit) {
    FilledTonalButton(
        onClick = onClick,
        // The tonal fill alone is 1.1:1 on the sheet; the outline keeps the edge at 3:1 (UX 15).
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        modifier = Modifier.heightIn(min = 48.dp),
    ) {
        Text(stringResource(label), maxLines = 1)
    }
}

/** "<feature> is off for this phone" (UX 10.4). A scope this app does not know gets the plain title. */
@StringRes
internal fun lockedTitle(scope: String): Int =
    when (scope) {
        "state.titles" -> R.string.locked_title_titles
        "control" -> R.string.locked_title_control
        "destructive" -> R.string.locked_title_destructive
        "idle.inhibit" -> R.string.locked_title_idle_inhibit
        "audio" -> R.string.locked_title_audio
        "audio.capture" -> R.string.locked_title_audio_capture
        "media" -> R.string.locked_title_media
        "launch" -> R.string.locked_title_launch
        "launch.risky" -> R.string.locked_title_launch_risky
        "input.text", "input.keys" -> R.string.locked_title_input
        else -> R.string.locked_title
    }
