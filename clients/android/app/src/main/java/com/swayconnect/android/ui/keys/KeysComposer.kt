package com.swayconnect.android.ui.keys

import androidx.annotation.StringRes
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicSecureTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldDecorator
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.InterceptPlatformTextInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.HoldButton
import com.swayconnect.android.ui.theme.SwayIcons

/** The terminal hold of "Send + ⏎" (UX 9.2). */
private const val RETURN_HOLD_MS = 400

/** What the composer may do now; the area computes it from the state. */
internal data class ComposerLook(
    /** `input.text` may go: the field is editable and Send can be enabled. */
    val canType: Boolean,
    /** `input.key` may go: "Send + ⏎" can be enabled. */
    val canPressReturn: Boolean,
    /** A send waits for its answer: the field is read-only, so the delivered part can be cut exactly. */
    val busy: Boolean,
    /** The target is a known terminal: "Send + ⏎" needs a hold. */
    val terminal: Boolean,
    val privateMode: Boolean,
    /** The computer is set to look only: the field stays disabled (no IME) and its placeholder says why. */
    val readOnly: Boolean,
) {
    /** The field's placeholder: the reason typing is off, else what the field is for. */
    @get:StringRes
    val hint: Int
        get() = when {
            readOnly && !canType -> R.string.keys_typing_off
            privateMode -> R.string.keys_private_field
            else -> R.string.keys_field
        }
}

/**
 * The composer (UX 9.1, 9.2, 17.8): a one-line field inside [NoLearningInterceptor], Send and "Send + ⏎". The IME's
 * Send key is Send. [onSend] gets true for "Send + ⏎". Enter is never put in the text: a pasted line break makes the
 * text unsendable until it is removed.
 */
@Composable
internal fun Composer(field: TextFieldState, look: ComposerLook, onSend: (thenReturn: Boolean) -> Unit) {
    val split = TextSplit.split(field.text.toString())
    val sendable = look.canType && !look.busy && split is TextSplit.Result.Parts
    // An empty field still sends Enter alone: the special keys row has no Return.
    val returnable = look.canPressReturn && !look.busy && (split == TextSplit.Result.Empty || (look.canType && split is TextSplit.Result.Parts))
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        InterceptPlatformTextInput(NoLearningInterceptor) {
            if (look.privateMode) PrivateField(field, look, onSend) else PlainField(field, look, onSend)
        }
        if (split == TextSplit.Result.Unsendable) {
            Text(stringResource(R.string.keys_unsendable), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
            SendButton(sendable, look.busy) { onSend(false) }
            SendReturnButton(returnable, look.terminal) { onSend(true) }
        }
    }
}

@Composable
private fun PlainField(field: TextFieldState, look: ComposerLook, onSend: (Boolean) -> Unit) {
    TextField(
        state = field,
        modifier = Modifier.fillMaxWidth(),
        enabled = look.canType,
        readOnly = look.busy,
        placeholder = { Text(stringResource(look.hint)) },
        lineLimits = TextFieldLineLimits.SingleLine,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
        onKeyboardAction = { if (!look.busy) onSend(false) },
        shape = MaterialTheme.shapes.medium,
    )
}

/** Private mode (UX 9.2): hidden text, a password keyboard. The area drops its text on rotation and after Send. */
@Composable
private fun PrivateField(field: TextFieldState, look: ComposerLook, onSend: (Boolean) -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val hint = stringResource(look.hint)
    val decorator = TextFieldDecorator { inner ->
        Surface(shape = MaterialTheme.shapes.medium, color = scheme.surfaceContainer, border = BorderStroke(1.dp, scheme.outline)) {
            Box(Modifier.heightIn(min = 56.dp).padding(16.dp)) {
                if (field.text.isEmpty()) Text(hint, style = MaterialTheme.typography.bodyLarge, color = scheme.onSurfaceVariant)
                inner()
            }
        }
    }
    BasicSecureTextField(
        state = field,
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = hint },
        enabled = look.canType,
        readOnly = look.busy,
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onSurface),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Send),
        onKeyboardAction = { if (!look.busy) onSend(false) },
        cursorBrush = SolidColor(scheme.primary),
        decorator = decorator,
    )
}

@Composable
private fun SendButton(enabled: Boolean, busy: Boolean, onClick: () -> Unit) {
    val waiting = stringResource(R.string.keys_sending)
    Button(onClick = onClick, enabled = enabled, modifier = Modifier.heightIn(min = 48.dp).semantics { if (busy) stateDescription = waiting }) {
        if (busy) CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp, color = LocalContentColor.current)
        else Text(stringResource(R.string.keys_send))
    }
}

/** "Send + ⏎": a 400 ms hold for a known terminal, with a confirm dialog for TalkBack (UX 9.2, 15). */
@Composable
private fun SendReturnButton(enabled: Boolean, terminal: Boolean, onSend: () -> Unit) {
    val label = stringResource(R.string.keys_send_return)
    if (!terminal) {
        val spoken = stringResource(R.string.keys_send_return_spoken)
        OutlinedButton(onClick = onSend, enabled = enabled, modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = spoken }) { Text(label) }
        return
    }
    var confirming by remember { mutableStateOf(false) }
    HoldButton(
        label = label,
        icon = SwayIcons.Terminal.outline,
        onHeld = onSend,
        onConfirmRequest = { confirming = true },
        spoken = stringResource(R.string.keys_send_return_hold_spoken),
        enabled = enabled,
        holdMs = RETURN_HOLD_MS,
    )
    if (confirming && enabled) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(stringResource(R.string.keys_confirm_title)) },
            text = { Text(stringResource(R.string.keys_confirm_text)) },
            confirmButton = {
                TextButton(onClick = { confirming = false; onSend() }) { Text(stringResource(R.string.keys_confirm_run)) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
