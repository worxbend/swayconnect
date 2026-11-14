package com.swayconnect.android.ui.more

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import com.swayconnect.android.R
import com.swayconnect.android.ui.audio.AudioTap
import com.swayconnect.android.ui.audio.DeviceCard
import com.swayconnect.android.ui.state.Countdown
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.replaceUnsafe

/** The keep-awake tile of docs/UX.md 10.2. [onAction] carries out what a tap or a long press asks for. */
@Composable
internal fun KeepAwakeQuickTile(tile: KeepAwakeTile, onAction: (KeepAwakeAction) -> Unit, modifier: Modifier = Modifier) {
    val tap = tile.tap()
    val longPress = tile.longPress()
    val face = tile.face
    val extras = listOfNotNull(
        stringResource(R.string.keep_awake_computer_locked).takeIf { tile.computerLocked },
        stringResource(R.string.keep_awake_window_too).takeIf { tile.windowKeepsAwake },
    )
    val locked = tile.access is OpAvailability.Locked
    val look = TileLook(
        icon = SwayIcons.Coffee,
        title = stringResource(if (face is KeepAwakeFace.On) R.string.keep_awake_on else R.string.keep_awake),
        lines = (if (locked) listOf(stringResource(R.string.more_off_for_phone)) else faceLines(face)) + extras,
        on = face is KeepAwakeFace.On && !locked,
        locked = locked,
        pending = tile.pending,
    )
    val state = if (locked) stringResource(R.string.more_off_for_phone) else spokenState(face)
    val speech = TileSpeech(
        state = (listOf(state) + extras).joinToString(", "),
        clickLabel = actionLabel(tap),
        longClickLabel = actionLabel(longPress),
        toggled = face is KeepAwakeFace.On,
    )
    val input = TileInput(
        onClick = { onAction(tap) }.takeIf { tap != KeepAwakeAction.None },
        onLongClick = { onAction(longPress) }.takeIf { longPress != KeepAwakeAction.None },
        enabled = tile.access != OpAvailability.ReadOnly,
        stale = !tile.live,
    )
    QuickTile(look, speech, input, modifier)
}

@Composable
private fun faceLines(face: KeepAwakeFace): List<String> =
    when (face) {
        KeepAwakeFace.Waiting -> listOf(stringResource(R.string.more_waiting))
        KeepAwakeFace.Off -> listOf(stringResource(R.string.keep_awake_off))
        is KeepAwakeFace.On -> {
            val count = face.leftMs?.let { countdownText(it) }
            when {
                face.fromThisPhone -> listOf(count ?: stringResource(R.string.keep_awake_on_untimed))
                else -> listOfNotNull(count, stringResource(R.string.keep_awake_other_device))
            }
        }
        is KeepAwakeFace.Cooldown -> listOf(
            when (val shown = Countdown.of(face.leftMs)) {
                is Countdown.Minutes -> stringResource(R.string.keep_awake_available_in_minutes, shown.minutes.toInt())
                is Countdown.Seconds -> stringResource(R.string.keep_awake_available_in_seconds, shown.seconds.toInt())
            },
        )
    }

/** "52 min left", "45 s left": per minute, per second in the last minute. Not spoken (see [spokenState]). */
@Composable
private fun countdownText(leftMs: Long): String =
    when (val shown = Countdown.of(leftMs)) {
        is Countdown.Minutes -> stringResource(R.string.keep_awake_minutes_left, shown.minutes.toInt())
        is Countdown.Seconds -> stringResource(R.string.keep_awake_seconds_left, shown.seconds.toInt())
    }

/** The spoken state names its minutes in steps of 5, so it changes at most every 5 minutes (UX 10.2). */
@Composable
private fun spokenState(face: KeepAwakeFace): String =
    when (face) {
        KeepAwakeFace.Waiting -> stringResource(R.string.keep_awake_spoken_waiting)
        KeepAwakeFace.Off -> stringResource(R.string.keep_awake_spoken_off)
        is KeepAwakeFace.On -> {
            val minutes = face.leftMs?.let { Countdown.announcedMinutes(it).toInt() }
            when {
                face.fromThisPhone && minutes != null -> pluralStringResource(R.plurals.keep_awake_spoken_left, minutes, minutes)
                face.fromThisPhone -> stringResource(R.string.keep_awake_spoken_on)
                minutes != null -> pluralStringResource(R.plurals.keep_awake_spoken_other_left, minutes, minutes)
                else -> stringResource(R.string.keep_awake_spoken_other)
            }
        }
        is KeepAwakeFace.Cooldown -> {
            val minutes = Countdown.announcedMinutes(face.leftMs).toInt()
            pluralStringResource(R.plurals.keep_awake_spoken_cooldown, minutes, minutes)
        }
    }

@Composable
private fun actionLabel(action: KeepAwakeAction): String? =
    when (action) {
        KeepAwakeAction.TurnOn -> stringResource(R.string.keep_awake_turn_on)
        KeepAwakeAction.TurnOff -> stringResource(R.string.keep_awake_turn_off)
        KeepAwakeAction.ChooseDuration -> stringResource(R.string.keep_awake_choose)
        is KeepAwakeAction.ShowLocked -> stringResource(R.string.more_show_how)
        is KeepAwakeAction.Refuse, KeepAwakeAction.None -> null
    }

/** A tile for one op (UX 10.3): hidden when unsupported, a lock that opens the Locked sheet when locked. */
internal class GatedTile(
    val access: OpAvailability,
    /** What a tap does while the op is available. */
    val onAvailable: () -> Unit,
    val onLocked: (String) -> Unit,
    /** Read-only still taps through when the tile only opens a sheet ([onAvailable] sends nothing). */
    val opensSheet: Boolean = false,
)

@Composable
internal fun OpQuickTile(gate: GatedTile, look: TileLook, speech: TileSpeech, stale: Boolean, modifier: Modifier = Modifier) {
    val access = gate.access
    val locked = access is OpAvailability.Locked
    val shown = if (locked) TileLook(look.icon, look.title, listOf(stringResource(R.string.more_off_for_phone)), locked = true) else look
    val spoken = if (locked) TileSpeech(stringResource(R.string.more_off_for_phone), stringResource(R.string.more_show_how)) else speech
    val onClick: () -> Unit = if (access is OpAvailability.Locked) ({ gate.onLocked(access.scope) }) else gate.onAvailable
    val enabled = access != OpAvailability.ReadOnly || gate.opensSheet
    QuickTile(shown, spoken, TileInput(onClick, enabled = enabled, stale = stale && !locked), modifier)
}

/** Drive: the same switch as the Desktop context pill (UX 10.3). It changes the phone only, so it acts at once. */
@Composable
internal fun DriveQuickTile(gate: GatedTile, driving: Boolean, modifier: Modifier = Modifier) {
    // As the context pill: without the op, swipes only browse, whatever the choice.
    val shownOn = driving && gate.access == OpAvailability.Available
    val word = stringResource(if (shownOn) R.string.drive_tile_on else R.string.drive_tile_off)
    val look = TileLook(SwayIcons.Swap, stringResource(R.string.drive_tile), listOf(word), on = shownOn)
    val haptics = LocalHaptics.current
    val toggle = GatedTile(gate.access, {
        haptics.perform(if (driving) SwayHaptic.ToggleOff else SwayHaptic.ToggleOn)
        gate.onAvailable()
    }, gate.onLocked)
    OpQuickTile(toggle, look, TileSpeech(word, clickLabel = null, toggled = shownOn), stale = false, modifier)
}

@Composable
internal fun ScratchpadQuickTile(gate: GatedTile, hidden: Int?, stale: Boolean, modifier: Modifier = Modifier) {
    val line = hidden?.let { if (it == 0) stringResource(R.string.scratchpad_empty) else pluralStringResource(R.plurals.scratchpad_hidden, it, it) }
    val look = TileLook(SwayIcons.Scratchpad, stringResource(R.string.scratchpad), listOfNotNull(line))
    OpQuickTile(gate, look, TileSpeech(line, clickLabel = null), stale, modifier)
}

/** Lock: `session.lock`, no confirm (UX 10.3). The answer `already_locked` becomes the line "Already locked.". */
@Composable
internal fun LockQuickTile(gate: GatedTile, pending: Boolean, stale: Boolean, modifier: Modifier = Modifier) {
    val look = TileLook(SwayIcons.Lock, stringResource(R.string.lock_tile), listOf(stringResource(R.string.lock_tile_now)), pending = pending)
    val idle = GatedTile(gate.access, { if (!pending && !stale) gate.onAvailable() }, gate.onLocked)
    OpQuickTile(idle, look, TileSpeech(state = null, clickLabel = stringResource(R.string.lock_tile_spoken)), stale, modifier)
}

/**
 * Mic (UX 10.1, 10.3): ink while live. A tap mutes; unmuting is a 600 ms hold with `audio.capture` (a dialog under
 * TalkBack), else the lock opens the Locked sheet. [onSetMuted] takes the device id and the new mute state.
 */
@Composable
internal fun MicQuickTile(tile: MicTile, onSetMuted: (Long, Boolean) -> Unit, onLocked: (String) -> Unit, modifier: Modifier = Modifier) {
    val haptics = LocalHaptics.current
    var asking by remember { mutableStateOf(false) }
    val tap = tile.tap()
    val card = (tile as? MicTile.Shown)?.card
    val live = card?.device?.muted == false
    val line = micLine(tile)
    val look = TileLook(
        icon = if (live) SwayIcons.Mic else SwayIcons.MicOff,
        title = stringResource(R.string.mic_tile),
        lines = listOf(line),
        on = live,
        locked = tap is AudioTap.ShowLocked,
        pending = card?.pending == true,
    )
    val clickLabel = when (tap) {
        AudioTap.Send -> stringResource(R.string.mic_tile_mute)
        AudioTap.Hold -> stringResource(R.string.mic_tile_unmute)
        is AudioTap.ShowLocked -> stringResource(R.string.more_show_how)
        AudioTap.None -> null
    }
    val onClick: (() -> Unit)? = when (tap) {
        // A default input that is live: Send is muting (UX 7.2).
        AudioTap.Send -> card?.let { shown -> { haptics.perform(SwayHaptic.ToggleOff); onSetMuted(shown.device.id, true) } }
        is AudioTap.ShowLocked -> { { onLocked(tap.scope) } }
        AudioTap.Hold, AudioTap.None -> null
    }
    val hold = card?.takeIf { tap == AudioTap.Hold }?.let { shown -> TileHold(onHeld = { onSetMuted(shown.device.id, false) }, onConfirmRequest = { asking = true }) }
    val input = TileInput(onClick, enabled = card?.toggleMute != OpAvailability.ReadOnly, stale = card?.live == false, hold = hold)
    QuickTile(look, TileSpeech(line, clickLabel, toggled = live.takeIf { card != null }), input, modifier)
    if (asking) {
        MicUnmuteDialog(card?.takeIf { tap == AudioTap.Hold }, onDismiss = { asking = false }) { id ->
            asking = false
            onSetMuted(id, false)
        }
    }
}

@Composable
private fun micLine(tile: MicTile): String =
    when (tile) {
        is MicTile.Locked -> stringResource(R.string.more_off_for_phone)
        MicTile.Waiting -> stringResource(R.string.more_waiting)
        MicTile.Unreachable -> stringResource(R.string.mic_tile_unreachable)
        MicTile.NoMic -> stringResource(R.string.mic_tile_none)
        is MicTile.Shown -> stringResource(if (tile.card.device.muted) R.string.mic_tile_muted else R.string.mic_tile_live)
    }

/** The TalkBack way of the unmute hold (UX 7.2, 15). A null [card] (no longer unmutable) closes it. */
@Composable
private fun MicUnmuteDialog(card: DeviceCard?, onDismiss: () -> Unit, onConfirm: (Long) -> Unit) {
    if (card == null) {
        LaunchedEffect(Unit) { onDismiss() }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.audio_unmute_title)) },
        text = { Text(stringResource(R.string.audio_unmute_text, replaceUnsafe(card.device.description))) },
        confirmButton = { TextButton(onClick = { onConfirm(card.device.id) }) { Text(stringResource(R.string.audio_unmute_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** Launcher on computer (UX 10.3): `launcher.open`. The view model then goes to Keys or shows the line (UX 8.2). */
@Composable
internal fun LauncherQuickTile(tile: LauncherTile, onOpen: () -> Unit, onLocked: (String) -> Unit, modifier: Modifier = Modifier) {
    val look = TileLook(SwayIcons.Search, stringResource(R.string.launcher_tile), listOf(stringResource(R.string.launcher_tile_where)), pending = tile.pending)
    val gate = GatedTile(tile.access, { if (tile.tap() == LauncherTap.Open) onOpen() }, onLocked)
    OpQuickTile(gate, look, TileSpeech(state = null, clickLabel = stringResource(R.string.launcher_tile_spoken)), stale = !tile.live, modifier)
}
