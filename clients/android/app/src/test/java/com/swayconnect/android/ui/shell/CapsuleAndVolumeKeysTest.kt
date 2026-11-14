package com.swayconnect.android.ui.shell

import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.Screen
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.withAudio
import com.swayconnect.android.ui.state.withAudioStale
import com.swayconnect.android.ui.state.withMedia
import com.swayconnect.android.ui.state.withMediaStale
import com.swayconnect.android.ui.state.withNowPlayingHidden
import com.swayconnect.protocol.AudioState
import com.swayconnect.protocol.Device
import com.swayconnect.protocol.DeviceKind
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.MediaState
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.Player
import com.swayconnect.protocol.PlayerStatus
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.Welcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CapsuleAndVolumeKeysTest {
    @Test
    fun `the capsule shows on Desktop, Launch and More only`() {
        val state = ready().withMedia(media(player(1)))

        for (area in Area.entries) {
            val shown = capsuleOf(state.copy(area = area)) != null
            assertEquals("area $area", area == Area.Desktop || area == Area.Launch || area == Area.More, shown)
        }
    }

    @Test
    fun `the capsule needs a playing or paused player`() {
        assertNull("no media state", capsuleOf(ready()))
        assertNull("no player", capsuleOf(ready().withMedia(MediaState(emptyList(), null, truncated = false))))
        assertNull("stopped", capsuleOf(ready().withMedia(media(player(1, PlayerStatus.STOPPED)))))
        assertEquals(1L, capsuleOf(ready().withMedia(media(player(1, PlayerStatus.PAUSED))))?.player?.id)
    }

    @Test
    fun `hide until next track hides the capsule until the track changes`() {
        val hidden = ready().withMedia(media(player(1, title = "Song"))).withNowPlayingHidden()

        assertNull(capsuleOf(hidden))
        assertNull("same track", capsuleOf(hidden.withMedia(media(player(1, title = "Song")))))
        assertEquals(1L, capsuleOf(hidden.withMedia(media(player(1, title = "Next song"))))?.player?.id)
    }

    @Test
    fun `capsule buttons follow the media op state, the stale state and the spinner`() {
        val state = ready().withMedia(media(player(1)))
        val card = checkNotNull(capsuleOf(state))
        assertTrue(card.canSend(MediaCommand.PLAY_PAUSE))
        assertTrue(card.canSend(MediaCommand.NEXT))

        val readOnly = checkNotNull(capsuleOf(state.copy(granted = granted(ops = BASE_OPS, readOnly = true))))
        assertEquals(OpAvailability.ReadOnly, readOnly.access)
        assertFalse(readOnly.canSend(MediaCommand.PLAY_PAUSE))

        assertFalse("stale", checkNotNull(capsuleOf(state.withMediaStale())).canSend(MediaCommand.NEXT))

        val pending = checkNotNull(capsuleOf(state.copy(pendingControl = PendingControl.Player(1))))
        assertTrue(pending.pending)
        assertFalse(pending.canSend(MediaCommand.PLAY_PAUSE))
    }

    @Test
    fun `before welcome the capsule waits`() {
        assertNull(capsuleOf(ready().withMedia(media(player(1))).copy(granted = null)))
    }

    @Test
    fun `volume keys act on the default output only while Audio is shown`() {
        val state = ready(area = Area.Audio).withAudio(audio())

        for (key in VolumeKey.entries) assertEquals("key $key", 1L, volumeKeyTarget(state, key)?.id)
        for (area in Area.entries - Area.Audio) assertNull("area $area", volumeKeyTarget(state.copy(area = area), VolumeKey.Up))
    }

    @Test
    fun `volume keys stay the phone's own when the computer cannot take them`() {
        val state = ready(area = Area.Audio).withAudio(audio())

        assertNull("no audio state", volumeKeyTarget(ready(area = Area.Audio), VolumeKey.Up))
        assertNull("no default output", volumeKeyTarget(ready(area = Area.Audio).withAudio(audio(defaultSink = null)), VolumeKey.Up))
        assertNull("stale", volumeKeyTarget(state.withAudioStale(), VolumeKey.Up))
        assertNull("read-only", volumeKeyTarget(state.copy(granted = granted(ops = BASE_OPS, readOnly = true)), VolumeKey.Down))
        assertNull("not granted", volumeKeyTarget(state.copy(granted = granted(scopes = listOf("state.read"), ops = listOf(Op.PING))), VolumeKey.Up))
        assertNull("pair screen", volumeKeyTarget(state.copy(screen = Screen.Pair), VolumeKey.Up))
        assertNull("revoked", volumeKeyTarget(state.copy(unpaired = true), VolumeKey.Up))
        assertNull("wrong key", volumeKeyTarget(state.copy(wrongKey = true), VolumeKey.Up))
    }

    @Test
    fun `the mute key needs the mute op, the step keys the volume op`() {
        val muteOnly = ready(area = Area.Audio, granted = granted(ops = BASE_OPS + Op.AUDIO_SET_MUTE)).withAudio(audio())

        assertEquals(1L, volumeKeyTarget(muteOnly, VolumeKey.Mute)?.id)
        assertNull(volumeKeyTarget(muteOnly, VolumeKey.Up))
    }

    private companion object {
        /** A read-only server lists no action op in `ops` (docs/PROTOCOL.md 17.3). */
        val BASE_OPS = listOf(Op.PING, Op.STATE_SUBSCRIBE)
        val ALL_OPS = BASE_OPS + listOf(Op.AUDIO_SET_VOLUME, Op.AUDIO_SET_MUTE, Op.MEDIA_PLAY_PAUSE, Op.MEDIA_NEXT, Op.MEDIA_PREVIOUS)

        fun granted(scopes: List<String> = listOf("state.read", "audio", "media"), ops: List<String> = ALL_OPS, readOnly: Boolean = false) =
            Granted.of(
                Welcome(
                    protocol = 1,
                    deviceId = "device",
                    serverProof = "proof",
                    scopes = scopes,
                    ops = ops,
                    topics = listOf("desktop", "audio", "media"),
                    readOnly = readOnly,
                    server = ServerInfo("desk", "0.2.0"),
                    limits = FrameLimits(16384, 1048576, 90000),
                    knownScopes = listOf("state.read", "audio", "media"),
                    knownOps = ALL_OPS,
                ),
            )

        fun ready(area: Area = Area.Desktop, granted: Granted = granted()) =
            UiState(pair = PairForm(deviceName = "Pixel 9")).copy(
                screen = Screen.Desktop,
                connection = ConnectionStatus.Ready,
                area = area,
                granted = granted,
            )

        fun player(id: Long, status: PlayerStatus = PlayerStatus.PLAYING, title: String? = "Song") =
            Player(id, "Chromium", "chromium", null, status, title, title?.let { "Artist" }, null, true, true, true, true)

        fun media(vararg players: Player) = MediaState(players.toList(), players.firstOrNull()?.id, truncated = false)

        fun audio(defaultSink: Long? = 1) =
            AudioState(
                true,
                100,
                defaultSink,
                null,
                listOf(Device(1, "Line out", DeviceKind.SPEAKER, 40, muted = false, available = true)),
                emptyList(),
                truncated = false,
            )
    }
}
