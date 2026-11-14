package com.swayconnect.android.ui.more

import com.swayconnect.android.ui.audio.AudioTap
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.SCOPE_AUDIO_CAPTURE
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.withAudio
import com.swayconnect.android.ui.state.withAudioStale
import com.swayconnect.protocol.AudioState
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.Welcome
import com.swayconnect.protocol.Wire
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The Mic and Launcher tiles of docs/UX.md 10.1 and 10.3, from protocol/vectors/v1/ext/audio.json. */
class MicLauncherTileTest {
    @Test
    fun `mic - the default input with audio capture unmutes by a hold`() {
        val tile = micTile(stateWith(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE).withAudio(audio()))
        val shown = tile as MicTile.Shown
        assertEquals("the default source of the vector", 2L, shown.card.device.id)
        assertTrue(shown.card.device.muted)
        assertEquals(AudioTap.Hold, tile.tap())
    }

    @Test
    fun `mic - without audio capture unmuting shows the lock, muting is a tap`() {
        val muted = micTile(stateWith(DEFAULT_SCOPES).withAudio(audio()))!!
        assertTrue("the tile stays a mic tile: muting is free", muted is MicTile.Shown)
        assertEquals(AudioTap.ShowLocked(SCOPE_AUDIO_CAPTURE), muted.tap())

        val live = micTile(stateWith(DEFAULT_SCOPES).withAudio(audio(defaultMicMuted = false)))!!
        assertEquals("muting a microphone needs audio only", AudioTap.Send, live.tap())
    }

    @Test
    fun `mic - without the audio scope the whole tile is locked`() {
        val tile = micTile(stateWith(listOf("state.read", "launch")).withAudio(audio()))
        assertEquals(MicTile.Locked("audio"), tile)
        assertEquals(AudioTap.ShowLocked("audio"), tile!!.tap())
    }

    @Test
    fun `mic - waiting, unreachable and no default input take no tap`() {
        val granted = stateWith(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE)
        val cases = mapOf(
            MicTile.Waiting to micTile(granted),
            MicTile.Unreachable to micTile(granted.withAudio(audio("02-sound-server-unreachable"))),
            MicTile.NoMic to micTile(granted.withAudio(audio("05-no-default-devices"))),
        )
        for ((expected, tile) in cases) {
            assertEquals(expected, tile)
            assertEquals(AudioTap.None, tile!!.tap())
        }
    }

    @Test
    fun `mic - read-only, stale and a waiting mute take no tap`() {
        val readOnly = micTile(stateWith(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE, readOnly = true).withAudio(audio()))!!
        val stale = micTile(stateWith(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE).withAudio(audio()).withAudioStale())!!
        val pending = micTile(stateWith(DEFAULT_SCOPES + SCOPE_AUDIO_CAPTURE).withAudio(audio()).copy(pendingControl = PendingControl.Device(2)))!!
        for (tile in listOf(readOnly, stale, pending)) assertEquals(AudioTap.None, tile.tap())
        assertEquals(OpAvailability.ReadOnly, (readOnly as MicTile.Shown).card.toggleMute)
        assertFalse((stale as MicTile.Shown).card.live)
        assertTrue((pending as MicTile.Shown).card.pending)
    }

    @Test
    fun `mic - no tile before welcome or when the server lacks audio set_mute`() {
        assertNull(micTile(UiState(pair = PairForm(deviceName = "Pixel"))))
        val noMute = stateWith(DEFAULT_SCOPES, knownOps = KNOWN_OPS - Op.AUDIO_SET_MUTE).withAudio(audio())
        assertNull(micTile(noMute))
    }

    @Test
    fun `launcher - a tap opens the launcher on the computer`() {
        val tile = launcherTile(stateWith(DEFAULT_SCOPES))!!
        assertEquals(OpAvailability.Available, tile.access)
        assertEquals(LauncherTap.Open, tile.tap())
    }

    @Test
    fun `launcher - without the launch scope the tap opens the Locked sheet`() {
        assertEquals(LauncherTap.ShowLocked("launch"), launcherTile(stateWith(listOf("state.read", "audio")))!!.tap())
    }

    @Test
    fun `launcher - read-only, a stale connection and a waiting open take no tap`() {
        val readOnly = launcherTile(stateWith(DEFAULT_SCOPES, readOnly = true))!!
        val stale = launcherTile(stateWith(DEFAULT_SCOPES).copy(desktopLive = false))!!
        val pending = launcherTile(stateWith(DEFAULT_SCOPES).copy(pendingControl = PendingControl.Launcher))!!
        assertTrue(pending.pending)
        assertFalse(stale.live)
        for (tile in listOf(readOnly, stale, pending)) assertEquals(LauncherTap.None, tile.tap())
    }

    @Test
    fun `launcher - no tile before welcome or when the server lacks launcher open`() {
        assertNull(launcherTile(UiState(pair = PairForm(deviceName = "Pixel"))))
        assertNull(launcherTile(stateWith(DEFAULT_SCOPES, knownOps = KNOWN_OPS - Op.LAUNCHER_OPEN)))
    }

    private fun stateWith(scopes: List<String>, readOnly: Boolean = false, knownOps: List<String> = KNOWN_OPS) =
        UiState(pair = PairForm(deviceName = "Pixel"), desktopLive = true, granted = granted(scopes, readOnly, knownOps))

    private companion object {
        val DEFAULT_SCOPES = listOf("state.read", "audio", "launch", "input.text")
        val KNOWN_OPS = listOf(Op.AUDIO_SET_VOLUME, Op.AUDIO_SET_MUTE, Op.AUDIO_SET_DEFAULT, Op.LAUNCHER_OPEN)

        /** A server that knows [knownOps]; it grants those whose scope the phone holds. */
        fun granted(scopes: List<String>, readOnly: Boolean, knownOps: List<String>): Granted {
            val grantedOps = if (readOnly) emptyList() else knownOps.filter { op -> if (op == Op.LAUNCHER_OPEN) "launch" in scopes else "audio" in scopes }
            return Granted.of(
                Welcome(
                    protocol = 1,
                    deviceId = "device",
                    serverProof = "proof",
                    scopes = scopes,
                    ops = listOf(Op.PING) + grantedOps,
                    topics = listOf("desktop", "audio", "session"),
                    readOnly = readOnly,
                    server = ServerInfo("desk", "0.2.0"),
                    limits = FrameLimits(16384, 1048576, 90000),
                    knownOps = knownOps,
                ),
            )
        }

        /** The vector [name]; [defaultMicMuted] false unmutes its default source. */
        fun audio(name: String = "01-two-sinks-two-sources", defaultMicMuted: Boolean = true): AudioState {
            val file = Wire.json.parseToJsonElement(vectorFile("ext/audio.json").readText()).jsonObject
            val case = file.getValue("states").jsonArray.first { it.jsonObject.getValue("name").jsonPrimitive.content == name }
            val frame = Wire.json.parseToJsonElement(case.jsonObject.getValue("payload").jsonPrimitive.content).jsonObject
            val state = Wire.decode(AudioState.serializer(), frame.getValue("data").jsonObject.getValue("data"))
            if (defaultMicMuted) return state
            return state.copy(sources = state.sources.map { if (it.id == state.defaultSource) it.copy(muted = false) else it })
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
