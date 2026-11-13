package com.swayconnect.android.ui.pair

import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.state.PairPhase
import com.swayconnect.android.ui.state.UiState

/**
 * The pair flow of docs/UX.md 11: welcome, target, confirm code, what this phone can do.
 * A link only fills the form; the Pair button starts the connection.
 */
@Composable
fun PairScreen(state: UiState, viewModel: MainViewModel) {
    SecureWindow()
    val form = state.pair
    val target = form.target
    val server = state.server
    PairBackHandler(state, viewModel)
    // safeDrawing covers the system bars, a display cutout and the keyboard.
    Scaffold(contentWindowInsets = WindowInsets.safeDrawing) { innerPadding ->
        Column(
            modifier =
                Modifier.padding(innerPadding)
                    .verticalScroll(rememberScrollState())
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                stringResource(R.string.pair_title),
                style = MaterialTheme.typography.headlineLarge,
                modifier = Modifier.semantics { heading() },
            )
            when (form.phase) {
                PairPhase.Editing ->
                    if (target == null) {
                        WelcomeCard(
                            error = form.error,
                            serverName = server?.name,
                            onLink = viewModel::onLinkEdited,
                            onBack = viewModel::showDesktop,
                        )
                    } else {
                        TargetCard(
                            target = target,
                            deviceName = form.deviceName,
                            replaces = server?.name,
                            error = form.error,
                            onDeviceName = viewModel::onDeviceNameEdited,
                            onPair = viewModel::requestPairing,
                            onOtherLink = { viewModel.onLinkEdited("") },
                        )
                    }
                PairPhase.Connecting,
                PairPhase.AwaitingConfirmation -> ConfirmCodeCard(form.confirmCode, onCancel = viewModel::cancelPairing)
                PairPhase.Confirmed ->
                    CapabilitiesCard(
                        serverName = server?.name,
                        granted = state.granted,
                        problem = state.connectionProblem,
                        onDone = viewModel::finishPairing,
                    )
            }
        }
    }
    if (form.confirmingReplace && server != null) {
        ReplaceDialog(server.name, onConfirm = viewModel::confirmReplace, onDismiss = viewModel::dismissReplace)
    }
}

/**
 * Back steps out of the flow: from the target to the welcome card, from the welcome card to the stored
 * computer, from step 4 to the desktop. While the computer is asked, Back is the system's, as in v1:
 * the attempt outlives the activity and Cancel is on screen.
 */
@Composable
private fun PairBackHandler(state: UiState, viewModel: MainViewModel) {
    val form = state.pair
    val editing = form.phase == PairPhase.Editing
    BackHandler(enabled = editing && form.target != null) { viewModel.onLinkEdited("") }
    BackHandler(enabled = editing && form.target == null && state.server != null) { viewModel.showDesktop() }
    BackHandler(enabled = form.phase == PairPhase.Confirmed) { viewModel.finishPairing() }
}

/**
 * FLAG_SECURE while any pair step is shown (UX 11 rule 5, DESIGN 5.6 item 12): the link and the code stay out
 * of screenshots, screen shares and the recents thumbnail.
 */
@Composable
private fun SecureWindow() {
    val window = checkNotNull(LocalActivity.current) { "the pair screen needs an activity" }.window
    DisposableEffect(window) {
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}

@Composable
private fun ReplaceDialog(serverName: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.pair_replace_button)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.pair_cancel)) } },
        title = { Text(stringResource(R.string.pair_replace_title, serverName)) },
        text = { Text(stringResource(R.string.pair_replace_text, serverName)) },
    )
}
