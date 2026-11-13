package com.swayconnect.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp

/** Screen 1 of docs/DESIGN.md 8.3. A link only fills the form; the Pair button starts the connection. */
@Composable
fun PairScreen(state: UiState, viewModel: MainViewModel) {
    val form = state.pair
    // With a stored pairing this screen was opened over the desktop screen: back returns there.
    BackHandler(enabled = state.server != null && form.phase == PairPhase.Editing) { viewModel.showDesktop() }
    // safeDrawing covers the system bars, a display cutout and the keyboard.
    Scaffold(contentWindowInsets = WindowInsets.safeDrawing) { innerPadding ->
        Column(
            modifier =
                Modifier.padding(innerPadding)
                    .verticalScroll(rememberScrollState())
                    .fillMaxWidth()
                    .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Pair with a computer", style = MaterialTheme.typography.headlineSmall)
            if (form.phase == PairPhase.Editing) {
                PairFormFields(state, viewModel)
            } else {
                PairProgress(form.confirmCode, onCancel = viewModel::cancelPairing)
            }
        }
    }
    val server = state.server
    if (form.confirmingReplace && server != null) {
        ReplaceDialog(server.name, onConfirm = viewModel::confirmReplace, onDismiss = viewModel::dismissReplace)
    }
}

@Composable
private fun PairFormFields(state: UiState, viewModel: MainViewModel) {
    val form = state.pair
    val focusManager = LocalFocusManager.current
    Text(
        "On the computer, run: swayconnectd pair. Scan the QR code it prints and open the link, " +
            "or copy the link and paste it here."
    )
    OutlinedTextField(
        value = form.link,
        onValueChange = viewModel::onLinkEdited,
        label = { Text("Pairing link") },
        modifier = Modifier.fillMaxWidth(),
        maxLines = 4,
        // The link holds the pairing secret: the keyboard must neither learn it nor "correct" it.
        // The text stays readable, since the user checks host and fingerprint.
        keyboardOptions =
            KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Done),
        // Done closes the keyboard, which covers the Pair button.
        keyboardActions = KeyboardActions(onDone = { focusManager.clearFocus() }),
    )
    form.target?.let { PairTargetSummary(it) }
    DeviceNameField(form.deviceName, viewModel::onDeviceNameEdited)
    state.server?.let { Text("This replaces ${it.name}.") }
    form.error?.let {
        Text(
            it,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        )
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = viewModel::requestPairing, enabled = form.target != null) { Text("Pair") }
        if (state.server != null) {
            TextButton(onClick = viewModel::showDesktop) { Text("Cancel") }
        }
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
        label = { Text("Device name") },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
    )
}

@Composable
private fun PairTargetSummary(target: PairTarget) {
    Column {
        target.name?.let { Text("Computer: $it") }
        Text("Host: ${target.host}")
        Text("Port: ${target.port}")
        Text("Fingerprint: ${target.shortFingerprint} …", fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun PairProgress(confirmCode: String?, onCancel: () -> Unit) {
    if (confirmCode == null) {
        CircularProgressIndicator()
        Text("Connecting to the computer")
    } else {
        Text(
            confirmCode,
            style = MaterialTheme.typography.displayLarge,
            fontFamily = FontFamily.Monospace,
            // Digit by digit: a screen reader would read one number of six digits.
            modifier = Modifier.semantics { contentDescription = confirmCode.toList().joinToString(" ") },
        )
        Text(
            "The terminal on the computer shows a code. If it is the same, answer yes there. " +
                "If not, answer no and tap Cancel. Keep this app open until the computer answers."
        )
    }
    TextButton(onClick = onCancel) { Text("Cancel") }
}

@Composable
private fun ReplaceDialog(serverName: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onConfirm) { Text("Replace") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        title = { Text("Replace $serverName?") },
        text = { Text("The pairing with $serverName is deleted once the new computer has confirmed this device.") },
    )
}
