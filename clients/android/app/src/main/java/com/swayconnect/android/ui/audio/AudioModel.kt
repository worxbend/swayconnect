package com.swayconnect.android.ui.audio

import com.swayconnect.android.ui.state.AudioView
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.sourceAvailability
import com.swayconnect.protocol.Device
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.Player
import com.swayconnect.protocol.PlayerStatus

// Pure rules of the Audio area (docs/UX.md 7). No Compose import, so the JVM tests cover them.

enum class DeviceRole { Output, Input }

/** What a tap on a control does. [Send] calls the server; [ShowLocked] opens the Locked sheet. */
sealed interface AudioTap {
    data object Send : AudioTap

    /** Unmuting a microphone: a 600 ms hold, or a confirm dialog under TalkBack (UX 7.2). */
    data object Hold : AudioTap

    data class ShowLocked(val scope: String) : AudioTap

    /** Read-only, stale, waiting for an answer, or nothing to do: the control takes no tap. */
    data object None : AudioTap
}

/** One device card of UX 7.1. Only the default device shows its mute button and slider. */
data class DeviceCard(
    val device: Device,
    val role: DeviceRole,
    val isDefault: Boolean,
    /** The slider value: the phone's draft or the server's volume, 0 to [maxVolume]. */
    val volume: Int,
    val maxVolume: Int,
    val setDefault: OpAvailability,
    val setVolume: OpAvailability,
    /** The op state of the mute button's next step: muting, or unmuting (a source then needs `audio.capture`). */
    val toggleMute: OpAvailability,
    /** False while the state is not confirmed by a live connection: dimmed, no taps. */
    val live: Boolean,
    /** A mute or default change of this device waits for its answer. */
    val pending: Boolean,
) {
    /** UX 7.1: an unplugged device is a disabled row. The default one keeps its controls. */
    val unplugged: Boolean get() = !device.available

    /** A tap on the card makes it the default device. */
    fun tap(): AudioTap =
        when {
            isDefault || unplugged -> AudioTap.None
            else -> gate(setDefault) { AudioTap.Send }
        }

    /** A tap on the mute button. Unmuting a microphone needs the hold (UX 7.2). */
    fun muteTap(): AudioTap = gate(toggleMute) { if (role == DeviceRole.Input && device.muted) AudioTap.Hold else AudioTap.Send }

    /** The slider moves only while the server would take the value. */
    val sliderEnabled: Boolean get() = setVolume == OpAvailability.Available && live

    /** The scope the slider needs, when it is locked. */
    val sliderLockedScope: String? get() = (setVolume as? OpAvailability.Locked)?.scope

    private inline fun gate(access: OpAvailability, onAvailable: () -> AudioTap): AudioTap =
        when {
            access is OpAvailability.Locked -> AudioTap.ShowLocked(access.scope)
            access != OpAvailability.Available || !live || pending -> AudioTap.None
            else -> onAvailable()
        }
}

/** The devices part of the area. */
sealed interface DevicesPart {
    /** The server offers no audio: the part is left out. */
    data object Hidden : DevicesPart

    data class Locked(val scope: String) : DevicesPart

    /** The server offers audio but sent no `audio` state yet. */
    data object Waiting : DevicesPart

    /** `available: false`: "Sound server not reachable on worxbox." */
    data object Unreachable : DevicesPart

    /** "No sound devices". */
    data object Empty : DevicesPart

    data class Shown(val outputs: List<DeviceCard>, val inputs: List<DeviceCard>, val truncated: Boolean) : DevicesPart
}

/** The devices part of [state]; before `welcome` it waits. */
fun devicesPart(state: UiState): DevicesPart {
    val granted = state.granted ?: return DevicesPart.Waiting
    when (val access = granted.availabilityOf(Op.AUDIO_SET_VOLUME)) {
        OpAvailability.Unsupported -> return DevicesPart.Hidden
        is OpAvailability.Locked -> return DevicesPart.Locked(access.scope)
        else -> Unit
    }
    val view = state.audio ?: return DevicesPart.Waiting
    val audio = view.state
    return when {
        !audio.available -> DevicesPart.Unreachable
        audio.sinks.isEmpty() && audio.sources.isEmpty() -> DevicesPart.Empty
        else -> DevicesPart.Shown(
            outputs = audio.sinks.map { cardOf(it, DeviceRole.Output, view, granted, state.pendingControl) },
            inputs = audio.sources.map { cardOf(it, DeviceRole.Input, view, granted, state.pendingControl) },
            truncated = audio.truncated,
        )
    }
}

private fun cardOf(device: Device, role: DeviceRole, view: AudioView, granted: Granted, pending: PendingControl?): DeviceCard {
    val output = role == DeviceRole.Output
    val defaultId = if (output) view.state.defaultSink else view.state.defaultSource
    val write: (String) -> OpAvailability = if (output) granted::availabilityOf else granted::sourceAvailability
    return DeviceCard(
        device = device,
        role = role,
        isDefault = device.id == defaultId,
        volume = view.shownVolume(device),
        maxVolume = view.state.maxVolumePct.toInt(),
        setDefault = write(Op.AUDIO_SET_DEFAULT),
        setVolume = write(Op.AUDIO_SET_VOLUME),
        // Muting a source needs `audio` only (PROTOCOL 17.9 capture rule).
        toggleMute = if (device.muted) write(Op.AUDIO_SET_MUTE) else granted.availabilityOf(Op.AUDIO_SET_MUTE),
        live = view.live,
        pending = pending == PendingControl.Device(device.id),
    )
}

/** The now-playing card of UX 7.1. */
data class NowPlaying(
    val player: Player,
    /** The op state of the `media.*` ops; they share one scope. */
    val access: OpAvailability,
    val live: Boolean,
    val pending: Boolean,
) {
    val playing: Boolean get() = player.status == PlayerStatus.PLAYING

    /** No title: the server withholds it without `state.titles`, or the player has none ("Playing · Chromium"). */
    val titleWithheld: Boolean get() = player.title == null

    /** True when [command] may be sent now: the player allows it (`can_*`) and the phone may act. */
    fun canSend(command: MediaCommand): Boolean {
        val allowed = when (command) {
            MediaCommand.PLAY_PAUSE -> if (playing) player.canPause else player.canPlay
            MediaCommand.NEXT -> player.canNext
            MediaCommand.PREVIOUS -> player.canPrevious
        }
        return allowed && access == OpAvailability.Available && live && !pending
    }
}

/** The media part of the area. */
sealed interface MediaPart {
    data object Hidden : MediaPart

    data class Locked(val scope: String) : MediaPart

    data object Waiting : MediaPart

    /** "Nothing playing". */
    data object Nothing : MediaPart

    data class Shown(val card: NowPlaying) : MediaPart
}

/** The media part of [state]; before `welcome` it waits. */
fun mediaPart(state: UiState): MediaPart {
    val granted = state.granted ?: return MediaPart.Waiting
    val access = granted.availabilityOf(Op.MEDIA_PLAY_PAUSE)
    if (access == OpAvailability.Unsupported) return MediaPart.Hidden
    if (access is OpAvailability.Locked) return MediaPart.Locked(access.scope)
    val view = state.media ?: return MediaPart.Waiting
    val player = view.activePlayer ?: return MediaPart.Nothing
    return MediaPart.Shown(NowPlaying(player, access, view.live, state.pendingControl == PendingControl.Player(player.id)))
}

private const val TICK_STEP_PCT = 5

enum class SliderHaptic { Threshold, Tick }

/** The haptic of a slider step from [from] to [to] (UX 7.2, 14): a threshold at 0 and at [max], a tick every 5 %. */
fun sliderHapticOf(from: Int, to: Int, max: Int): SliderHaptic? =
    when {
        from == to -> null
        to == 0 || to == max -> SliderHaptic.Threshold
        from / TICK_STEP_PCT != to / TICK_STEP_PCT -> SliderHaptic.Tick
        else -> null
    }
