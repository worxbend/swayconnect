package com.swayconnect.android.ui.state

import com.swayconnect.protocol.AudioState
import com.swayconnect.protocol.Device
import com.swayconnect.protocol.OpAvailability

/** Scope that a source write other than muting needs as well (docs/PROTOCOL.md 17.9 capture rule). */
const val SCOPE_AUDIO_CAPTURE = "audio.capture"

/**
 * Topic `audio` as the phone shows it (docs/UX.md 7). [drafts] are the sliders' own values: a slider shows its draft
 * while the finger moves and after release until the server's next state, so it never jumps back to an older value.
 */
data class AudioView(
    val state: AudioState,
    /** False once the connection ended; the UI dims the area and sends nothing. */
    val live: Boolean = true,
    val drafts: Map<Long, VolumeDraft> = emptyMap(),
) {
    /** The value a slider shows for [device]: its draft, or the server's volume cut to the slider's range. */
    fun shownVolume(device: Device): Int = drafts[device.id]?.pct ?: device.volumePct.coerceAtMost(state.maxVolumePct).toInt()

    /** The default output device; null when the server names none or names one it does not list. */
    val defaultSink: Device?
        get() = state.sinks.firstOrNull { it.id == state.defaultSink }
}

/** A slider value of the phone, 0 to `max_volume_pct`. */
data class VolumeDraft(val pct: Int, val phase: Phase) {
    enum class Phase {
        /** The finger is on the slider. */
        Dragging,

        /** The finger lifted; the final value waits for its answer. */
        Released,

        /** The final value was answered: the next `audio` state replaces the draft. */
        Answered,
    }
}

/**
 * Applies an `audio` state. Drafts of devices that left the state go, and so do answered ones: the server value is the
 * truth again. A device action's spinner ends here or with its answer, whichever is first.
 */
fun UiState.withAudio(next: AudioState): UiState {
    val shown = next.copy(sinks = next.sinks.distinctBy { it.id }, sources = next.sources.distinctBy { it.id })
    val deviceIds = (shown.sinks + shown.sources).map { it.id }.toSet()
    val drafts = audio?.drafts.orEmpty().filter { (id, draft) -> id in deviceIds && draft.phase != VolumeDraft.Phase.Answered }
    return copy(
        audio = AudioView(shown, live = true, drafts = drafts),
        pendingControl = pendingControl.takeUnless { it is PendingControl.Device },
    )
}

/** The connection ended: the last state stays dimmed, and no slider waits for an answer any more. */
fun UiState.withAudioStale(): UiState = copy(audio = audio?.copy(live = false, drafts = emptyMap()))

/** The finger moved a slider to [pct]; it is cut to `0..max_volume_pct`. Unchanged when the state lists no such device. */
fun UiState.withVolumeDragged(deviceId: Long, pct: Int): UiState = withDraft(deviceId, pct, VolumeDraft.Phase.Dragging)

/** The finger lifted at [pct], or a volume key stepped to it. */
fun UiState.withVolumeReleased(deviceId: Long, pct: Int): UiState = withDraft(deviceId, pct, VolumeDraft.Phase.Released)

/** `audio.set_volume` of [pct] was answered with `ok`. Only the final value of a released slider counts. */
fun UiState.withVolumeAnswered(deviceId: Long, pct: Int): UiState {
    val view = audio ?: return this
    if (!view.isFinalValue(deviceId, pct)) return this
    return copy(audio = view.copy(drafts = view.drafts + (deviceId to VolumeDraft(pct, VolumeDraft.Phase.Answered))))
}

/**
 * `audio.set_volume` of [pct] failed. When it was the final value, the slider shows the server's volume again at once:
 * no new state may follow a value that never changed anything.
 */
fun UiState.withVolumeFailed(deviceId: Long, pct: Int): UiState {
    val view = audio ?: return this
    if (!view.isFinalValue(deviceId, pct)) return this
    return copy(audio = view.copy(drafts = view.drafts - deviceId))
}

private fun AudioView.isFinalValue(deviceId: Long, pct: Int): Boolean =
    drafts[deviceId]?.let { it.phase == VolumeDraft.Phase.Released && it.pct == pct } == true

private fun UiState.withDraft(deviceId: Long, pct: Int, phase: VolumeDraft.Phase): UiState {
    val view = audio ?: return this
    if ((view.state.sinks + view.state.sources).none { it.id == deviceId }) return this
    val value = pct.coerceIn(0, view.state.maxVolumePct.toInt())
    return copy(audio = view.copy(drafts = view.drafts + (deviceId to VolumeDraft(value, phase))))
}

/**
 * How a write to a source shows (docs/UX.md 7.2): [op] by the table of 17.3, and locked with `audio.capture` when the
 * op is available but that scope is missing. Muting a source needs no `audio.capture`; use [Granted.availabilityOf].
 */
fun Granted.sourceAvailability(op: String): OpAvailability {
    val base = availabilityOf(op)
    return if (base == OpAvailability.Available && SCOPE_AUDIO_CAPTURE !in scopes) OpAvailability.Locked(SCOPE_AUDIO_CAPTURE) else base
}
