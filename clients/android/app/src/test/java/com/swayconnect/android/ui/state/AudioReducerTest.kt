package com.swayconnect.android.ui.state

import com.swayconnect.protocol.AudioState
import com.swayconnect.protocol.Device
import com.swayconnect.protocol.DeviceKind
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.Welcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The audio reducer and the slider drafts of docs/UX.md 7.2. */
class AudioReducerTest {
    private val base = UiState(pair = PairForm(deviceName = "Pixel"))

    @Test
    fun `a slider shows its draft while dragging, released and answered, then the server value`() {
        val shown = base.withAudio(audio(sink(1, volume = 40)))
        val dragged = shown.withVolumeDragged(1, 55)
        assertEquals(55, dragged.volume(1))

        // A state from an earlier send must not move a slider under the finger or a final value that waits.
        val midDrag = dragged.withAudio(audio(sink(1, volume = 48)))
        assertEquals(55, midDrag.volume(1))
        val released = midDrag.withVolumeReleased(1, 60)
        assertEquals(60, released.withAudio(audio(sink(1, volume = 55))).volume(1))

        // An answer for another value than the final one changes nothing.
        assertEquals(VolumeDraft.Phase.Released, released.withVolumeAnswered(1, 55).audio?.drafts?.get(1)?.phase)
        val answered = released.withVolumeAnswered(1, 60)
        assertEquals(60, answered.volume(1))
        assertEquals(61, answered.withAudio(audio(sink(1, volume = 61))).volume(1))
    }

    @Test
    fun `a failed final value shows the server value at once, a failed middle value does not`() {
        val dragged = base.withAudio(audio(sink(1, volume = 40))).withVolumeDragged(1, 50)
        assertEquals(50, dragged.withVolumeFailed(1, 50).volume(1))

        val released = dragged.withVolumeReleased(1, 70)
        assertEquals(70, released.withVolumeFailed(1, 50).volume(1))
        assertEquals(40, released.withVolumeFailed(1, 70).volume(1))
    }

    @Test
    fun `slider values stay within 0 and max_volume_pct`() {
        val shown = base.withAudio(audio(sink(1, volume = 130), maxVolumePct = 100))
        assertEquals("a device above the maximum shows the top", 100, shown.volume(1))
        assertEquals(100, shown.withVolumeDragged(1, 140).volume(1))
        assertEquals(0, shown.withVolumeDragged(1, -5).volume(1))
        assertEquals(150, base.withAudio(audio(sink(1, volume = 40), maxVolumePct = 150)).withVolumeDragged(1, 400).volume(1))
        assertEquals("no audio state, no draft", base, base.withVolumeDragged(1, 40))
        assertEquals("an unlisted device gets no draft", shown, shown.withVolumeDragged(9, 40))
    }

    @Test
    fun `drafts of gone devices go and duplicate ids are dropped`() {
        val dragged = base.withAudio(audio(sink(1), sink(2))).withVolumeDragged(2, 70)
        val next = dragged.withAudio(audio(sink(1), sink(1, volume = 99)))
        assertEquals(listOf(1L), next.audio?.state?.sinks?.map { it.id })
        assertEquals(emptyMap<Long, VolumeDraft>(), next.audio?.drafts)
    }

    @Test
    fun `an audio state ends a device spinner and leaves the others`() {
        val device = base.copy(pendingControl = PendingControl.Device(1)).withAudio(audio(sink(1)))
        assertNull(device.pendingControl)
        val player = base.copy(pendingControl = PendingControl.Player(1)).withAudio(audio(sink(1)))
        assertEquals(PendingControl.Player(1), player.pendingControl)
    }

    @Test
    fun `the default sink is the listed device the server names`() {
        assertEquals(2L, base.withAudio(audio(sink(1), sink(2), defaultSink = 2)).audio?.defaultSink?.id)
        assertNull(base.withAudio(audio(sink(1), defaultSink = 9)).audio?.defaultSink)
    }

    @Test
    fun `stale keeps the state dimmed and drops drafts`() {
        val stale = base.withAudio(audio(sink(1))).withVolumeDragged(1, 30).withAudioStale()
        assertEquals(false, stale.audio?.live)
        assertEquals(emptyMap<Long, VolumeDraft>(), stale.audio?.drafts)
    }

    @Test
    fun `source writes other than muting are locked without audio capture`() {
        val ops = listOf(Op.AUDIO_SET_VOLUME, Op.AUDIO_SET_MUTE, Op.AUDIO_SET_DEFAULT)
        val without = granted(listOf("state.read", "audio"), ops)
        assertEquals(OpAvailability.Locked(SCOPE_AUDIO_CAPTURE), without.sourceAvailability(Op.AUDIO_SET_VOLUME))
        assertEquals(OpAvailability.Locked(SCOPE_AUDIO_CAPTURE), without.sourceAvailability(Op.AUDIO_SET_DEFAULT))
        assertEquals("muting needs audio only", OpAvailability.Available, without.availabilityOf(Op.AUDIO_SET_MUTE))

        val with = granted(listOf("state.read", "audio", SCOPE_AUDIO_CAPTURE), ops)
        assertEquals(OpAvailability.Available, with.sourceAvailability(Op.AUDIO_SET_VOLUME))

        val readOnly = granted(listOf("state.read", "audio"), listOf(Op.PING), readOnly = true, knownOps = ops)
        assertEquals("read-only wins over the lock", OpAvailability.ReadOnly, readOnly.sourceAvailability(Op.AUDIO_SET_VOLUME))
    }

    private fun UiState.volume(deviceId: Long): Int {
        val view = checkNotNull(audio)
        return view.shownVolume(view.state.sinks.first { it.id == deviceId })
    }

    private companion object {
        fun sink(id: Long, volume: Long = 40) = Device(id, "Line out $id", DeviceKind.SPEAKER, volume, muted = false, available = true)

        fun audio(vararg sinks: Device, maxVolumePct: Long = 100, defaultSink: Long? = null) =
            AudioState(true, maxVolumePct, defaultSink, null, sinks.toList(), emptyList(), truncated = false)

        fun granted(scopes: List<String>, ops: List<String>, readOnly: Boolean = false, knownOps: List<String>? = null) =
            Granted.of(
                Welcome(
                    protocol = 1,
                    deviceId = "device",
                    serverProof = "proof",
                    scopes = scopes,
                    ops = ops,
                    topics = listOf("desktop", "audio"),
                    readOnly = readOnly,
                    server = ServerInfo("desk", "0.2.0"),
                    limits = FrameLimits(16384, 1048576, 90000),
                    knownOps = knownOps,
                ),
            )
    }
}
