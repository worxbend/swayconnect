package com.swayconnect.android.ui.audio

import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.SCOPE_AUDIO_CAPTURE
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.withAudio
import com.swayconnect.android.ui.state.withAudioStale
import com.swayconnect.android.ui.state.withMedia
import com.swayconnect.android.ui.state.withMediaStale
import com.swayconnect.android.ui.state.withVolumeDragged
import com.swayconnect.protocol.AudioState
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.MediaState
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.PlayerStatus
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.Welcome
import com.swayconnect.protocol.Wire
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The Audio area rules of docs/UX.md 7, from protocol/vectors/v1/ext/audio.json and media.json. */
class AudioModelTest {
    private val withCapture = granted(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE)
    private val withoutCapture = granted(DEFAULT_SCOPES)

    @Test
    fun `outputs and inputs as cards - only the default device has the slider and mute`() {
        val part = shown(audioState("01-two-sinks-two-sources"), withCapture)
        assertEquals(listOf(1L, 3L, 5L), part.outputs.map { it.device.id })
        assertEquals(listOf(2L, 4L), part.inputs.map { it.device.id })
        assertEquals(listOf(true, false, false), part.outputs.map { it.isDefault })
        assertEquals(listOf(true, false), part.inputs.map { it.isDefault })
        assertEquals(40, part.outputs[0].volume)
        assertEquals(100, part.outputs[0].maxVolume)
        assertTrue(part.outputs[0].sliderEnabled)
        assertFalse(part.truncated)
    }

    @Test
    fun `a tap makes a plugged device the default, an unplugged or the default one takes none`() {
        val outputs = shown(audioState("01-two-sinks-two-sources"), withCapture).outputs
        assertEquals("already the default", AudioTap.None, outputs[0].tap())
        assertEquals(AudioTap.Send, outputs[1].tap())
        assertTrue(outputs[2].unplugged)
        assertEquals("unplugged: a disabled row", AudioTap.None, outputs[2].tap())
    }

    @Test
    fun `without audio capture the mic unmute, level and input choice show the lock, muting does not`() {
        val inputs = shown(audioState("01-two-sinks-two-sources"), withoutCapture).inputs
        val muted = inputs[0]
        assertTrue(muted.device.muted)
        assertEquals(AudioTap.ShowLocked(SCOPE_AUDIO_CAPTURE), muted.muteTap())
        assertEquals(SCOPE_AUDIO_CAPTURE, muted.sliderLockedScope)
        assertFalse(muted.sliderEnabled)
        assertEquals(AudioTap.ShowLocked(SCOPE_AUDIO_CAPTURE), inputs[1].tap())

        val live = inputs[1].copy(isDefault = true)
        assertEquals("muting a microphone needs audio only", AudioTap.Send, live.muteTap())
    }

    @Test
    fun `with audio capture unmuting the mic is a hold, the rest a tap`() {
        val part = shown(audioState("01-two-sinks-two-sources"), withCapture)
        assertEquals(AudioTap.Hold, part.inputs[0].muteTap())
        assertTrue(part.inputs[0].sliderEnabled)
        assertEquals(AudioTap.Send, part.inputs[1].tap())
        assertEquals("an output mutes and unmutes with a tap", AudioTap.Send, part.outputs[0].muteTap())
        val mutedOutput = part.outputs[0].copy(device = part.outputs[0].device.copy(muted = true))
        assertEquals(AudioTap.Send, mutedOutput.muteTap())
    }

    @Test
    fun `read-only, a stale state and a waiting answer take no tap`() {
        val readOnly = shown(audioState("01-two-sinks-two-sources"), granted(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE, readOnly = true))
        assertEquals(AudioTap.None, readOnly.outputs[1].tap())
        assertEquals(AudioTap.None, readOnly.outputs[0].muteTap())
        assertEquals(AudioTap.None, readOnly.inputs[0].muteTap())
        assertFalse(readOnly.outputs[0].sliderEnabled)

        val stale = devicesPart(stateWith(withCapture).withAudio(audioState("01-two-sinks-two-sources")).withAudioStale()) as DevicesPart.Shown
        assertEquals(AudioTap.None, stale.outputs[1].tap())
        assertFalse(stale.outputs[0].sliderEnabled)

        val waiting = stateWith(withCapture).withAudio(audioState("01-two-sinks-two-sources")).copy(pendingControl = PendingControl.Device(3))
        val cards = (devicesPart(waiting) as DevicesPart.Shown).outputs
        assertTrue(cards[1].pending)
        assertEquals(AudioTap.None, cards[1].tap())
        assertFalse("another device's spinner leaves this one", cards[0].pending)
    }

    @Test
    fun `the slider range is 0 to max_volume_pct and a draft shows while dragging`() {
        val state = stateWith(withCapture).withAudio(audioState("04-volume-bounds-and-max-150"))
        val part = devicesPart(state) as DevicesPart.Shown
        assertEquals(150, part.outputs[0].maxVolume)
        assertEquals(0, part.outputs[0].volume)
        assertEquals("a device above the maximum shows the top", 150, part.outputs[1].volume)
        val dragged = devicesPart(state.withVolumeDragged(1, 70)) as DevicesPart.Shown
        assertEquals(70, dragged.outputs[0].volume)
    }

    @Test
    fun `no default device, a cut list, an unreachable server and no devices`() {
        val noDefault = shown(audioState("05-no-default-devices"), withCapture)
        assertTrue((noDefault.outputs + noDefault.inputs).none { it.isDefault })
        assertTrue(shown(audioState("06-truncated"), withCapture).truncated)
        assertEquals(DevicesPart.Unreachable, devicesPart(stateWith(withCapture).withAudio(audioState("02-sound-server-unreachable"))))
        val empty = AudioState(true, 100, null, null, emptyList(), emptyList(), truncated = false)
        assertEquals(DevicesPart.Empty, devicesPart(stateWith(withCapture).withAudio(empty)))
    }

    @Test
    fun `the devices part waits, locks or hides by FeatureAvailability`() {
        assertEquals("before welcome", DevicesPart.Waiting, devicesPart(UiState(pair = PairForm("Pixel"))))
        assertEquals("no audio state yet", DevicesPart.Waiting, devicesPart(stateWith(withCapture)))
        assertEquals(DevicesPart.Locked("audio"), devicesPart(stateWith(granted(listOf("state.read", "media")))))
        assertEquals(DevicesPart.Hidden, devicesPart(stateWith(granted(DEFAULT_SCOPES, knownOps = listOf(Op.PING)))))
    }

    @Test
    fun `now playing - the active player with its controls`() {
        val card = nowPlaying(mediaState("01-three-players"))
        assertEquals(1L, card.player.id)
        assertTrue(card.playing)
        assertFalse(card.titleWithheld)
        assertTrue(card.canSend(MediaCommand.PLAY_PAUSE))
        assertTrue(card.canSend(MediaCommand.NEXT))
        assertTrue(card.canSend(MediaCommand.PREVIOUS))
    }

    @Test
    fun `can flags of the player decide each button`() {
        val firefox = nowPlaying(mediaState("01-three-players"))
        val paused = firefox.copy(player = firefox.player.copy(status = PlayerStatus.PAUSED, canPause = false, canNext = false, canPrevious = false))
        assertTrue("paused: play needs can_play", paused.canSend(MediaCommand.PLAY_PAUSE))
        assertFalse(paused.canSend(MediaCommand.NEXT))
        assertFalse(paused.canSend(MediaCommand.PREVIOUS))
        val playing = paused.copy(player = paused.player.copy(status = PlayerStatus.PLAYING))
        assertFalse("playing: pause needs can_pause", playing.canSend(MediaCommand.PLAY_PAUSE))
    }

    @Test
    fun `titles withheld reads as the player name`() {
        val card = nowPlaying(mediaState("03-titles-withheld"))
        assertTrue(card.titleWithheld)
        assertNull(card.player.artist)
    }

    @Test
    fun `nothing playing, and the first playing player without an active id`() {
        assertEquals(MediaPart.Nothing, mediaPart(stateWith(withCapture).withMedia(mediaState("02-no-players"))))
        assertEquals(1L, nowPlaying(mediaState("06-no-active-player")).player.id)
    }

    @Test
    fun `media buttons take no tap when read-only, stale or waiting, and lock without media`() {
        val readOnly = mediaPart(stateWith(granted(DEFAULT_SCOPES, readOnly = true)).withMedia(mediaState("01-three-players"))) as MediaPart.Shown
        assertFalse(readOnly.card.canSend(MediaCommand.PLAY_PAUSE))
        val stale = mediaPart(stateWith(withCapture).withMedia(mediaState("01-three-players")).withMediaStale()) as MediaPart.Shown
        assertFalse(stale.card.canSend(MediaCommand.NEXT))
        val waiting = stateWith(withCapture).withMedia(mediaState("01-three-players")).copy(pendingControl = PendingControl.Player(1))
        assertFalse((mediaPart(waiting) as MediaPart.Shown).card.canSend(MediaCommand.PLAY_PAUSE))
        assertEquals(MediaPart.Locked("media"), mediaPart(stateWith(granted(listOf("state.read", "audio")))))
        assertEquals(MediaPart.Waiting, mediaPart(stateWith(withCapture)))
    }

    @Test
    fun `slider haptics - a threshold at 0 and the top, a tick every 5 percent`() {
        assertEquals(SliderHaptic.Threshold, sliderHapticOf(3, 0, 100))
        assertEquals(SliderHaptic.Threshold, sliderHapticOf(97, 100, 100))
        assertEquals(SliderHaptic.Threshold, sliderHapticOf(140, 150, 150))
        assertEquals(SliderHaptic.Tick, sliderHapticOf(44, 45, 100))
        assertEquals(SliderHaptic.Tick, sliderHapticOf(45, 44, 100))
        assertEquals(SliderHaptic.Tick, sliderHapticOf(41, 52, 100))
        assertNull(sliderHapticOf(41, 43, 100))
        assertNull(sliderHapticOf(50, 50, 100))
    }

    private fun shown(audio: AudioState, granted: Granted): DevicesPart.Shown = devicesPart(stateWith(granted).withAudio(audio)) as DevicesPart.Shown

    private fun nowPlaying(media: MediaState): NowPlaying = (mediaPart(stateWith(withCapture).withMedia(media)) as MediaPart.Shown).card

    private fun stateWith(granted: Granted) = UiState(pair = PairForm(deviceName = "Pixel"), desktopLive = true, granted = granted)

    private companion object {
        val DEFAULT_SCOPES = listOf("state.read", "state.titles", "control", "audio", "media")
        val AUDIO_OPS = listOf(Op.AUDIO_SET_VOLUME, Op.AUDIO_SET_MUTE, Op.AUDIO_SET_DEFAULT, Op.MEDIA_PLAY_PAUSE, Op.MEDIA_NEXT, Op.MEDIA_PREVIOUS)

        /** A server that knows the audio and media ops; it grants those whose scope the phone holds. */
        fun granted(scopes: List<String>, readOnly: Boolean = false, knownOps: List<String> = AUDIO_OPS): Granted {
            val grantedOps = when {
                readOnly -> emptyList()
                else -> knownOps.filter { op -> if (op.startsWith("media.")) "media" in scopes else "audio" in scopes }
            }
            return Granted.of(
                Welcome(
                    protocol = 1,
                    deviceId = "device",
                    serverProof = "proof",
                    scopes = scopes,
                    ops = listOf(Op.PING) + grantedOps,
                    topics = listOf("desktop", "audio", "media"),
                    readOnly = readOnly,
                    server = ServerInfo("desk", "0.2.0"),
                    limits = FrameLimits(16384, 1048576, 90000),
                    knownOps = knownOps,
                ),
            )
        }

        fun audioState(name: String): AudioState = vectorState("ext/audio.json", name, AudioState.serializer())

        fun mediaState(name: String): MediaState = vectorState("ext/media.json", name, MediaState.serializer())

        fun <T> vectorState(path: String, name: String, strategy: DeserializationStrategy<T>): T {
            val file = Wire.json.parseToJsonElement(vectorFile(path).readText()).jsonObject
            val case = file.getValue("states").jsonArray.first { it.jsonObject.getValue("name").jsonPrimitive.content == name }
            val frame = Wire.json.parseToJsonElement(case.jsonObject.getValue("payload").jsonPrimitive.content).jsonObject
            return Wire.decode(strategy, frame.getValue("data").jsonObject.getValue("data"))
        }

        /** The shared vectors, found from the test's working directory up to the repository root. */
        fun vectorFile(path: String): File {
            var dir: File? = File(System.getProperty("user.dir")).absoluteFile
            while (dir != null) {
                val candidate = File(dir, "protocol/vectors/v1/$path")
                if (candidate.isFile) return candidate
                dir = dir.parentFile
            }
            error("protocol/vectors/v1/$path not found above ${System.getProperty("user.dir")}")
        }
    }
}
