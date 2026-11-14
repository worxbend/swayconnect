package com.swayconnect.android.ui.keys

import android.view.WindowManager
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.common.StateLine
import com.swayconnect.android.ui.desktop.forDisplay
import com.swayconnect.android.ui.shell.RoundButton
import com.swayconnect.android.ui.state.KeysRefusal
import com.swayconnect.android.ui.state.KeysView
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.Typed
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.InputKey
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/**
 * The Keys area (docs/UX.md 9): the target card, the composer, the special keys and shortcuts, and the refusals.
 * `FLAG_SECURE` stays on while it is shown, in every mode.
 */
@Composable
fun KeysArea(state: UiState, viewModel: MainViewModel, contentPadding: PaddingValues) {
    SecureWindow()
    val keys = state.keys ?: return
    val plain = rememberTextFieldState()
    // Not saveable on purpose: Private text is dropped on rotation (UX 9.2).
    val secret = remember { TextFieldState() }
    LaunchedEffect(keys.privateMode) { if (!keys.privateMode) secret.clearText() }
    DropDelivered(keys, plain, secret)
    RejectOnRefusal(keys.refusal, state.notice)
    val desktop = remember(state.desktop) { state.desktop?.forDisplay() }
    val access = keysAccess(state)
    val offered = access != null && !(access.text == OpAvailability.Unsupported && access.keys == OpAvailability.Unsupported)
    Column(
        modifier = Modifier.fillMaxSize().padding(contentPadding).padding(top = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // The card, the refusals and the composer scroll; the key rows below them never do, so with the
        // keyboard up they stay directly above it (UX 9.1).
        Column(
            modifier = Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when {
                access == null -> Line(stringResource(R.string.keys_waiting))
                !offered -> Line(stringResource(R.string.keys_none))
                else -> KeysBody(state, viewModel, desktop, access, if (keys.privateMode) secret else plain)
            }
        }
        if (offered && access != null) KeyChips(state, viewModel, access)
    }
}

/** The round button of the title row (UX 9.1): Private mode on or off. */
@Composable
fun KeysPrivateButton(state: UiState, viewModel: MainViewModel, onLocked: (String) -> Unit) {
    val on = state.keys?.privateMode == true
    RoundButton(
        icon = if (on) SwayIcons.Lock.filled else SwayIcons.LockOpen.outline,
        label = stringResource(if (on) R.string.keys_private_on else R.string.keys_private_off),
        availability = state.granted?.availabilityOf(Op.INPUT_TEXT),
        onClick = { viewModel.setPrivateMode(!on) },
        onLocked = onLocked,
    )
}

@Composable
private fun KeysBody(state: UiState, viewModel: MainViewModel, desktop: Desktop?, access: KeysAccess, field: TextFieldState) {
    val keys = state.keys ?: return
    val terminal = targetIsTerminal(state)
    val canFocus = state.granted?.availabilityOf(Op.WINDOW_FOCUS) == OpAvailability.Available
    TargetCard(state, desktop, canFocus, FocusMovedActions(viewModel::typeThere, viewModel::focusKeysTarget))
    TargetPicker(state, desktop, viewModel::selectKeysTarget)
    RefusalCard(state, viewModel::dismissKeysRefusal)
    if (keys.refusal == null) SentLine(keys.typed)
    state.notice?.let { StateLine(it, announce = true) }
    if (state.granted?.readOnly == true) Line(stringResource(R.string.keys_read_only))
    val showLocked = { scope: String -> viewModel.openSheet(Sheet.Locked(scope)) }
    when (val text = access.text) {
        is OpAvailability.Locked -> DashedCard(stringResource(R.string.keys_text_locked), icon = SwayIcons.Lock.outline, onClick = { showLocked(text.scope) })
        OpAvailability.Unsupported -> Unit
        else -> {
            val look = ComposerLook(
                canType = canSend(state, Op.INPUT_TEXT),
                canPressReturn = canSend(state, Op.INPUT_KEY),
                busy = typingBusy(state),
                terminal = terminal,
                privateMode = keys.privateMode,
            )
            Composer(field, look) { thenReturn -> send(viewModel, field, thenReturn) }
        }
    }
}

/** The special keys and the shortcuts (UX 9.1), or their Locked card. */
@Composable
private fun KeyChips(state: UiState, viewModel: MainViewModel, access: KeysAccess) {
    val keys = state.keys ?: return
    when (val press = access.keys) {
        is OpAvailability.Locked ->
            DashedCard(stringResource(R.string.keys_keys_locked), icon = SwayIcons.Lock.outline, onClick = { viewModel.openSheet(Sheet.Locked(press.scope)) })
        OpAvailability.Unsupported -> Unit
        else -> {
            val enabled = canSend(state, Op.INPUT_KEY)
            val onPress = { keyPress: KeyPress -> viewModel.pressKey(keyPress.key, keyPress.modifiers) }
            SpecialKeysRow(keys.sticky, enabled, onPress, viewModel::toggleModifier)
            ShortcutsRow(targetIsTerminal(state), enabled, onPress)
        }
    }
}

/** Send cuts the text with [TextSplit]; "Send + ⏎" on an empty field presses Enter alone. */
private fun send(viewModel: MainViewModel, field: TextFieldState, thenReturn: Boolean) {
    when (val split = TextSplit.split(field.text.toString())) {
        is TextSplit.Result.Parts -> viewModel.sendText(split.parts, thenReturn)
        TextSplit.Result.Empty -> if (thenReturn) viewModel.pressKey(InputKey.RETURN)
        TextSplit.Result.Unsendable -> Unit
    }
}

/**
 * Once per send (`keys.typed.serial`): the delivered part leaves the field and the unsent tail stays (UX 9.2).
 * Private text goes whole. The field is read-only while a send waits, so the cut matches what was sent.
 */
@Composable
private fun DropDelivered(keys: KeysView, plain: TextFieldState, secret: TextFieldState) {
    val typed: Typed? = keys.typed
    var handled by rememberSaveable { mutableLongStateOf(typed?.serial ?: 0L) }
    LaunchedEffect(typed?.serial) {
        if (typed == null || typed.serial == handled) return@LaunchedEffect
        handled = typed.serial
        if (keys.privateMode) {
            secret.clearText()
        } else {
            plain.edit { replace(0, deliveredEnd(asCharSequence().toString(), typed.deliveredChars), "") }
        }
    }
}

/** `Reject` when a refusal or a refusal line appears (UX 14). */
@Composable
private fun RejectOnRefusal(refusal: KeysRefusal?, notice: String?) {
    val haptics = LocalHaptics.current
    var shown by remember { mutableStateOf(refusal to notice) }
    LaunchedEffect(refusal, notice) {
        val (shownRefusal, shownNotice) = shown
        if ((refusal != null && refusal != shownRefusal) || (notice != null && notice != shownNotice)) haptics.perform(SwayHaptic.Reject)
        shown = refusal to notice
    }
}

/** UX 9.2 and 17.8: what is typed for the computer stays out of screenshots, screen shares and recents. */
@Composable
private fun SecureWindow() {
    val window = checkNotNull(LocalActivity.current) { "the Keys area needs an activity" }.window
    DisposableEffect(window) {
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}

@Composable
private fun Line(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
