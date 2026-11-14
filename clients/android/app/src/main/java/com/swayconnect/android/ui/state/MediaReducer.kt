package com.swayconnect.android.ui.state

import com.swayconnect.protocol.MediaState
import com.swayconnect.protocol.Player
import com.swayconnect.protocol.PlayerStatus

/** Topic `media` as the phone shows it (docs/UX.md 4.3 and 7). */
data class MediaView(
    val state: MediaState,
    /** False once the connection ended. */
    val live: Boolean = true,
    /** "Hide until next track" (docs/UX.md 4.3): the capsule stays hidden while the active player plays this track. */
    val hiddenTrack: Track? = null,
) {
    /**
     * The player the now-playing card controls: `active_player_id`, else the first that plays, else the first paused
     * one (the server names none until a player has played, so a paused player would get no Play button). Null:
     * "Nothing playing".
     */
    val activePlayer: Player?
        get() =
            state.players.firstOrNull { it.id == state.activePlayerId }
                ?: state.players.firstOrNull { it.status == PlayerStatus.PLAYING }
                ?: state.players.firstOrNull { it.status == PlayerStatus.PAUSED }
}

/**
 * What tells one track from the next. Without `state.titles` the server sends no title, artist or album, so a hidden
 * capsule then stays hidden until another player becomes the active one.
 */
data class Track(val playerId: Long, val title: String?, val artist: String?, val album: String?) {
    companion object {
        fun of(player: Player) = Track(player.id, player.title, player.artist, player.album)
    }
}

/** Applies a `media` state. A hidden capsule shows again with the next track. A player's spinner ends here or with its answer. */
fun UiState.withMedia(next: MediaState): UiState {
    val view = MediaView(next.copy(players = next.players.distinctBy { it.id }))
    val stillHidden = media?.hiddenTrack?.takeIf { hidden -> view.activePlayer?.let(Track::of) == hidden }
    return copy(
        media = view.copy(hiddenTrack = stillHidden),
        pendingControl = pendingControl.takeUnless { it is PendingControl.Player },
    )
}

/** The connection ended: the last state stays, dimmed. */
fun UiState.withMediaStale(): UiState = copy(media = media?.copy(live = false))

/** "Hide until next track" on the capsule. */
fun UiState.withNowPlayingHidden(): UiState {
    val view = media ?: return this
    val player = view.activePlayer ?: return this
    return copy(media = view.copy(hiddenTrack = Track.of(player)))
}

/**
 * The player of the now-playing capsule (docs/UX.md 4.3): the active player while it plays or is paused, unless its
 * track is hidden. Which areas show the capsule is the shell's rule.
 */
val UiState.capsulePlayer: Player?
    get() {
        val view = media ?: return null
        val player = view.activePlayer?.takeIf { it.status != PlayerStatus.STOPPED } ?: return null
        return player.takeUnless { Track.of(it) == view.hiddenTrack }
    }
