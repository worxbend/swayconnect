package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// Topic `media` and its operations (docs/PROTOCOL.md 17.10). Player ids come from the topic.

/** An unknown value is [STOPPED]. */
@Serializable(with = PlayerStatusSerializer::class)
enum class PlayerStatus(val wire: String) {
    PLAYING("playing"),
    PAUSED("paused"),
    STOPPED("stopped"),
}

internal object PlayerStatusSerializer :
    FallbackEnumSerializer<PlayerStatus>("com.swayconnect.protocol.PlayerStatus", PlayerStatus.entries, PlayerStatus.STOPPED, PlayerStatus::wire)

/** State of topic `media`. */
@Serializable
data class MediaState(
    val players: List<Player>,
    /** The player a simple client controls: the last one that played. */
    @SerialName("active_player_id") @Serializable(with = WireInt::class) val activePlayerId: Long?,
    @Serializable(with = WireBool::class) val truncated: Boolean,
)

/** An MPRIS player. Every string comes from an application: plain text only. */
@Serializable
data class Player(
    @Serializable(with = WireInt::class) val id: Long,
    val name: String,
    /** Icon hint only. */
    @SerialName("app_id") val appId: String?,
    /** Icon hash for [Session.iconGet]. */
    val icon: String?,
    val status: PlayerStatus,
    /** `null` without scope `state.titles`, or when the player has none. */
    val title: String?,
    val artist: String?,
    val album: String?,
    @SerialName("can_play") @Serializable(with = WireBool::class) val canPlay: Boolean,
    @SerialName("can_pause") @Serializable(with = WireBool::class) val canPause: Boolean,
    @SerialName("can_next") @Serializable(with = WireBool::class) val canNext: Boolean,
    @SerialName("can_previous") @Serializable(with = WireBool::class) val canPrevious: Boolean,
)

/** The `media.*` operations. Each takes a player id only. */
enum class MediaCommand(val op: String) {
    PLAY_PAUSE(Op.MEDIA_PLAY_PAUSE),
    NEXT(Op.MEDIA_NEXT),
    PREVIOUS(Op.MEDIA_PREVIOUS),
}

/**
 * Sends [command] to the player [playerId].
 *
 * @throws RequestFailedException with `unsupported_target` when the player does not allow it (its `can_*` flags).
 */
@Throws(IOException::class)
suspend fun Session.controlMedia(playerId: Long, command: MediaCommand) {
    requireId(playerId)
    request(command.op, buildJsonObject { put("player_id", playerId) })
}
