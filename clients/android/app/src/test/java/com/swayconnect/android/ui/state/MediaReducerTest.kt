package com.swayconnect.android.ui.state

import com.swayconnect.protocol.MediaState
import com.swayconnect.protocol.Player
import com.swayconnect.protocol.PlayerStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The media reducer, the now-playing card and the capsule of docs/UX.md 4.3 and 7. */
class MediaReducerTest {
    private val base = UiState(pair = PairForm(deviceName = "Pixel"))

    @Test
    fun `the active player is the named one, else the first that plays, else none`() {
        val named = base.withMedia(media(player(1, PlayerStatus.PLAYING), player(2, PlayerStatus.PAUSED), active = 2))
        assertEquals(2L, named.media?.activePlayer?.id)
        val unnamed = base.withMedia(media(player(1, PlayerStatus.PAUSED), player(2, PlayerStatus.PLAYING), active = 9))
        assertEquals(2L, unnamed.media?.activePlayer?.id)
        val nothing = base.withMedia(media(player(1, PlayerStatus.STOPPED), active = null))
        assertNull("Nothing playing", nothing.media?.activePlayer)
    }

    @Test
    fun `a paused player that has not played since the server started is the active one`() {
        // Seen on a real MPRIS player (M2c R5): the server sends active_player_id null until a player plays.
        val paused = base.withMedia(media(player(1, PlayerStatus.STOPPED), player(2, PlayerStatus.PAUSED), active = null))
        assertEquals(2L, paused.media?.activePlayer?.id)
        assertEquals(2L, paused.capsulePlayer?.id)
    }

    @Test
    fun `the capsule shows a playing or paused player, not a stopped one`() {
        assertEquals(1L, base.withMedia(media(player(1, PlayerStatus.PAUSED), active = 1)).capsulePlayer?.id)
        assertNull(base.withMedia(media(player(1, PlayerStatus.STOPPED), active = 1)).capsulePlayer)
        assertNull(base.capsulePlayer)
    }

    @Test
    fun `hide until next track hides the capsule until the track changes`() {
        val hidden = base.withMedia(media(player(1, title = "One"), active = 1)).withNowPlayingHidden()
        assertNull(hidden.capsulePlayer)
        val sameTrack = hidden.withMedia(media(player(1, PlayerStatus.PAUSED, title = "One"), active = 1))
        assertNull("pausing is not a new track", sameTrack.capsulePlayer)

        val nextTrack = sameTrack.withMedia(media(player(1, title = "Two"), active = 1))
        assertEquals(1L, nextTrack.capsulePlayer?.id)
        val backToFirst = nextTrack.withMedia(media(player(1, title = "One"), active = 1))
        assertEquals("the hide is used up", 1L, backToFirst.capsulePlayer?.id)
    }

    @Test
    fun `without titles a hidden capsule waits for another player`() {
        val hidden = base.withMedia(media(player(1, title = null), active = 1)).withNowPlayingHidden()
        assertNull(hidden.withMedia(media(player(1, title = null), active = 1)).capsulePlayer)
        assertEquals(2L, hidden.withMedia(media(player(1, title = null), player(2, title = null), active = 2)).capsulePlayer?.id)
    }

    @Test
    fun `a media state ends a player spinner, drops duplicate ids, and goes stale`() {
        val state = base.copy(pendingControl = PendingControl.Player(1)).withMedia(media(player(1), player(1), active = 1))
        assertNull(state.pendingControl)
        assertEquals(1, state.media?.state?.players?.size)
        assertEquals(false, state.withMediaStale().media?.live)
    }

    private companion object {
        fun player(id: Long, status: PlayerStatus = PlayerStatus.PLAYING, title: String? = "Song") =
            Player(id, "Chromium", "chromium", null, status, title, title?.let { "Artist" }, null, true, true, true, true)

        fun media(vararg players: Player, active: Long?) = MediaState(players.toList(), active, truncated = false)
    }
}
