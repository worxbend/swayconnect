package com.swayconnect.android.ui.audio

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.common.StateLine
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.replaceUnsafe

/**
 * The Audio area (docs/UX.md 7): the now-playing card, then the outputs and the inputs as cards. It renders the
 * server's `audio` and `media` states; only a slider under the finger shows the phone's own value.
 */
@Composable
fun AudioArea(state: UiState, viewModel: MainViewModel, contentPadding: PaddingValues) {
    val onLocked = { scope: String -> viewModel.openSheet(Sheet.Locked(scope)) }
    var unmuteAsked by remember { mutableStateOf<Long?>(null) }
    var confirmAnswer by remember { mutableStateOf(false) }
    val actions = DeviceActions(
        onSetDefault = { id ->
            confirmAnswer = true
            viewModel.setDefaultDevice(id)
        },
        onSetMuted = viewModel::setMuted,
        onConfirmUnmute = { id -> unmuteAsked = id },
        onDrag = viewModel::dragVolume,
        onFinish = viewModel::finishVolume,
        onLocked = onLocked,
    )
    val devices = devicesPart(state)
    val media = mediaPart(state)
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(contentPadding).padding(top = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Above the cards, as in Launch: a refusal stays in view however many devices follow.
        for (line in stateLines(state, devices, media)) StateLine(line, announce = line == state.notice)
        MediaSection(media, onLocked, viewModel::controlMedia)
        DevicesSection(devices, actions, onLocked)
    }
    UnmuteDialog(devices, unmuteAsked, onDismiss = { unmuteAsked = null }) { id ->
        unmuteAsked = null
        viewModel.setMuted(id, false)
    }
    AudioHaptics(state, confirmAnswer) { confirmAnswer = false }
}

@Composable
private fun MediaSection(media: MediaPart, onLocked: (String) -> Unit, onCommand: (playerId: Long, MediaCommand) -> Unit) {
    when (media) {
        MediaPart.Hidden, MediaPart.Waiting -> Unit
        is MediaPart.Locked -> DashedCard(stringResource(R.string.media_locked), icon = SwayIcons.Lock.outline, onClick = { onLocked(media.scope) })
        MediaPart.Nothing -> DashedCard(stringResource(R.string.media_nothing), icon = SwayIcons.Play.outline)
        is MediaPart.Shown -> NowPlayingCard(media.card, onCommand = { command -> onCommand(media.card.player.id, command) })
    }
}

@Composable
private fun DevicesSection(devices: DevicesPart, actions: DeviceActions, onLocked: (String) -> Unit) {
    when (devices) {
        is DevicesPart.Locked -> DashedCard(stringResource(R.string.audio_locked), icon = SwayIcons.Lock.outline, onClick = { onLocked(devices.scope) })
        DevicesPart.Empty -> DashedCard(stringResource(R.string.audio_no_devices), icon = SwayIcons.Volume.outline)
        is DevicesPart.Shown -> {
            DeviceGroup(R.string.audio_output, devices.outputs, actions)
            DeviceGroup(R.string.audio_input, devices.inputs, actions)
        }
        DevicesPart.Hidden, DevicesPart.Waiting, DevicesPart.Unreachable -> Unit
    }
}

@Composable
private fun DeviceGroup(heading: Int, cards: List<DeviceCard>, actions: DeviceActions) {
    if (cards.isEmpty()) return
    Text(
        stringResource(heading),
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(top = 4.dp).semantics { heading() },
    )
    for (card in cards) DeviceCardView(card, actions)
}

/** "Waiting", the unreachable sound server, the last refusal, a cut list and read-only (UX 7.2, 16). */
@Composable
private fun stateLines(state: UiState, devices: DevicesPart, media: MediaPart): List<String> {
    val waiting = devices == DevicesPart.Waiting && (media == MediaPart.Waiting || media == MediaPart.Hidden)
    val server = state.server?.name
    return listOfNotNull(
        stringResource(R.string.audio_waiting).takeIf { waiting },
        stringResource(R.string.audio_none).takeIf { devices == DevicesPart.Hidden && media == MediaPart.Hidden },
        (if (server != null) stringResource(R.string.audio_unreachable, server) else stringResource(R.string.audio_unreachable_unnamed))
            .takeIf { devices == DevicesPart.Unreachable },
        state.notice,
        stringResource(R.string.audio_truncated).takeIf { (devices as? DevicesPart.Shown)?.truncated == true },
        stringResource(R.string.audio_read_only).takeIf { state.granted?.readOnly == true },
    )
}

/**
 * The TalkBack way of the microphone hold (UX 7.2, 15). It closes by itself when the device left the state or
 * may no longer be unmuted.
 */
@Composable
private fun UnmuteDialog(devices: DevicesPart, deviceId: Long?, onDismiss: () -> Unit, onConfirm: (Long) -> Unit) {
    if (deviceId == null) return
    val card = (devices as? DevicesPart.Shown)?.inputs?.firstOrNull { it.device.id == deviceId }
    if (card == null || card.muteTap() != AudioTap.Hold) {
        LaunchedEffect(Unit) { onDismiss() }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.audio_unmute_title)) },
        text = { Text(stringResource(R.string.audio_unmute_text, replaceUnsafe(card.label.name))) },
        confirmButton = { TextButton(onClick = { onConfirm(deviceId) }) { Text(stringResource(R.string.audio_unmute_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/**
 * Haptics of UX 14 for this area that follow an answer: `Confirm` when a default-device change ends without a
 * refusal ([confirmAnswer], set by the tap), `Reject` when a refusal line appears. Toggles and slider ticks fire
 * at the touch, in the cards.
 */
@Composable
private fun AudioHaptics(state: UiState, confirmAnswer: Boolean, onAnswered: () -> Unit) {
    val haptics = LocalHaptics.current
    var waited by remember { mutableStateOf<PendingControl?>(null) }
    LaunchedEffect(state.pendingControl) {
        val ended = waited is PendingControl.Device && state.pendingControl == null
        if (ended && confirmAnswer) {
            if (state.notice == null) haptics.perform(SwayHaptic.Confirm)
            onAnswered()
        }
        waited = state.pendingControl
    }
    var shownNotice by remember { mutableStateOf(state.notice) }
    LaunchedEffect(state.notice) {
        if (state.notice != null && state.notice != shownNotice) haptics.perform(SwayHaptic.Reject)
        shownNotice = state.notice
    }
}
