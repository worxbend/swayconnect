package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put

// The handshake classes are not data classes: a generated toString would print nonces and proofs.

/** A message the server may send (docs/PROTOCOL.md 6.5). */
sealed interface ServerMessage

@Serializable
data class ProtocolRange(val min: Int, val max: Int)

@Serializable
class Hello(val protocol: ProtocolRange, val nonce: String) : ServerMessage

@Serializable
class PairRequest(
    val protocol: Int,
    val nonce: String,
    val proof: String,
    @SerialName("device_name") val deviceName: String,
    val platform: String? = null,
    val client: String? = null,
)

@Serializable
class AuthRequest(
    val protocol: Int,
    @SerialName("device_id") val deviceId: String,
    val nonce: String,
    val proof: String,
    val client: String? = null,
)

@Serializable
class Welcome(
    val protocol: Int,
    @SerialName("device_id") val deviceId: String,
    @SerialName("server_proof") val serverProof: String,
    @SerialName("confirm_code") val confirmCode: String? = null,
    val scopes: List<String>,
    val ops: List<String>,
    val topics: List<String>,
    @SerialName("read_only") val readOnly: Boolean,
    val server: ServerInfo,
    val limits: FrameLimits,
    /** Every scope the server knows (17.3). Not granted: granted scopes are [scopes] only. Null from an older server. */
    @SerialName("known_scopes") val knownScopes: List<String>? = null,
    /** Every op the server implements and is configured for (17.3). Callable ops are [ops] only. */
    @SerialName("known_ops") val knownOps: List<String>? = null,
) : ServerMessage

private val welcomeSerializer = AbsentOrNotNullSerializer(Welcome.serializer(), setOf("known_scopes", "known_ops"))

@Serializable
data class ServerInfo(val name: String, val version: String)

@Serializable
data class FrameLimits(
    @SerialName("max_frame_c2s") val maxFrameC2s: Int,
    @SerialName("max_frame_s2c") val maxFrameS2c: Int,
    @SerialName("idle_timeout_ms") val idleTimeoutMs: Long,
)

@Serializable
data class Reject(val code: String, val message: String) : ServerMessage

@Serializable
data class Req(val id: Long, val op: String, val args: JsonObject? = null)

@Serializable
data class Res(
    val id: Long,
    val ok: Boolean,
    val result: JsonObject? = null,
    val error: ErrorBody? = null,
) : ServerMessage

@Serializable
data class ErrorBody(val code: String, val message: String, val detail: JsonObject? = null)

@Serializable
data class Evt(val event: String, val data: JsonObject) : ServerMessage

@Serializable
data class Bye(val code: String, val message: String) : ServerMessage

/** One entry of a `state.subscribe` result, and the `data` of a `state.changed` event. */
@Serializable
data class TopicState(val topic: String, val rev: Long, val data: JsonObject)

@Serializable
data class SubscribeResult(val states: List<TopicState>)

// Codes are strings on the wire and stay strings here: a client must accept codes it does not know
// (docs/PROTOCOL.md 13.2). Only the codes a client reacts to have a name.

object RejectCode {
    const val AUTH_FAILED = "auth_failed"
    const val PAIRING_FAILED = "pairing_failed"
    const val UNSUPPORTED_VERSION = "unsupported_version"
}

object ByeCode {
    const val SHUTDOWN = "shutdown"
    const val REVOKED = "revoked"
    const val SCOPES_CHANGED = "scopes_changed"
}

/** What a client does after `bye` (6.4, 13.2 rule 5, 17.15). */
enum class ByeReaction {
    /** `scopes_changed`: the new `welcome` lists the new scopes. */
    RECONNECT_NOW,

    /** `shutdown` and every code this client does not know. */
    RECONNECT_BACKOFF,

    /** `revoked`: forget the credential. */
    FORGET,
    ;

    companion object {
        fun of(code: String): ByeReaction =
            when (code) {
                ByeCode.SCOPES_CHANGED -> RECONNECT_NOW
                ByeCode.REVOKED -> FORGET
                else -> RECONNECT_BACKOFF
            }
    }
}

object ErrorCode {
    const val READ_ONLY = "read_only"
    const val RATE_LIMITED = "rate_limited"

    /** A server cap applies (17.15), for example the keep-awake continuous cap; wait `detail.retry_after_ms`. */
    const val LIMIT_REACHED = "limit_reached"
    const val BACKEND_UNAVAILABLE = "backend_unavailable"
    const val SPAWN_FAILED = "spawn_failed"

    // Remote keyboard refusals (17.13, 17.15); each carries `detail.delivered_chars`, see [InputRefusal].
    const val FOCUS_CHANGED = "focus_changed"
    const val LOCKED = "locked"
    const val KEYBOARD_GRABBED = "keyboard_grabbed"
    const val LAUNCHER_CLOSED = "launcher_closed"
}

object Op {
    const val PING = "ping"
    const val STATE_SUBSCRIBE = "state.subscribe"
    const val WORKSPACE_FOCUS = "workspace.focus"
    const val WINDOW_FOCUS = "window.focus"
    const val WINDOW_FULLSCREEN = "window.fullscreen"
    const val ICON_GET = "icon.get"
    const val WINDOW_MOVE = "window.move"
    const val WINDOW_FLOATING = "window.floating"
    const val WINDOW_LAYOUT = "window.layout"
    const val WINDOW_SCRATCHPAD = "window.scratchpad"
    const val SCRATCHPAD_SHOW = "scratchpad.show"
    const val SESSION_LOCK = "session.lock"
    const val WINDOW_CLOSE = "window.close"
    const val KEEP_AWAKE_SET = "keep_awake.set"
    const val AUDIO_SET_VOLUME = "audio.set_volume"
    const val AUDIO_SET_MUTE = "audio.set_mute"
    const val AUDIO_SET_DEFAULT = "audio.set_default"
    const val MEDIA_PLAY_PAUSE = "media.play_pause"
    const val MEDIA_NEXT = "media.next"
    const val MEDIA_PREVIOUS = "media.previous"
    const val APP_LAUNCH = "app.launch"
    const val LAUNCHER_OPEN = "launcher.open"
    const val INPUT_TEXT = "input.text"
    const val INPUT_KEY = "input.key"
}

object Topic {
    const val DESKTOP = "desktop"
    const val SESSION = "session"
    const val AUDIO = "audio"
    const val MEDIA = "media"
    const val APPS = "apps"
}

object EventName {
    const val STATE_CHANGED = "state.changed"
    const val PAIRING_CONFIRMED = "pairing.confirmed"
}

/** A frame was valid JSON but not a valid message. */
class MalformedMessageException(message: String, cause: Throwable? = null) : IOException(message, cause)

/** Maps messages to and from frame payloads. */
object Wire {
    // explicitNulls is the default, written out because docs/DESIGN.md 8.2 names it. It is an experimental setting.
    @OptIn(ExperimentalSerializationApi::class)
    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = true
    }

    fun encode(message: PairRequest): JsonObject = tagged("pair", json.encodeToJsonElement(PairRequest.serializer(), message))

    fun encode(message: AuthRequest): JsonObject = tagged("auth", json.encodeToJsonElement(AuthRequest.serializer(), message))

    fun encode(message: Req): JsonObject = tagged("req", json.encodeToJsonElement(Req.serializer(), message))

    /**
     * Returns null for a `type` this client does not know: the caller ignores such a frame (docs/PROTOCOL.md 13.2).
     */
    @Throws(MalformedMessageException::class)
    fun decodeServerMessage(frame: JsonObject): ServerMessage? {
        val type = (frame["type"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            ?: throw MalformedMessageException("frame has no string member \"type\"")
        return when (type) {
            "hello" -> decode(Hello.serializer(), frame)
            "welcome" -> decode(welcomeSerializer, frame)
            "reject" -> decode(Reject.serializer(), frame)
            "res" -> decode(Res.serializer(), frame)
            "evt" -> decode(Evt.serializer(), frame)
            "bye" -> decode(Bye.serializer(), frame)
            else -> null
        }
    }

    @Throws(MalformedMessageException::class)
    fun <T> decode(strategy: DeserializationStrategy<T>, element: JsonElement): T =
        try {
            json.decodeFromJsonElement(strategy, element)
        } catch (e: SerializationException) {
            throw MalformedMessageException("message does not have the shape the protocol defines", e)
        }

    private fun tagged(type: String, body: JsonElement): JsonObject =
        buildJsonObject {
            put("type", type)
            body.jsonObject.forEach { (name, value) -> put(name, value) }
        }
}
