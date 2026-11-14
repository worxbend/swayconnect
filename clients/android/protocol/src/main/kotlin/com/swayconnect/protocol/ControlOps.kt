package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// The window and session operations of docs/PROTOCOL.md 17.6 to 17.8. Each one is an action: `ok` means sway or
// the server's own program accepted it, the new state arrives as a topic event. Ids come from topic `desktop`.
// The arguments are checked against every bound the client knows before anything is sent; a bound that only the
// server knows (`max_duration_s`) comes back as `bad_request`.

/** The layouts `window.layout` takes. */
enum class WindowLayout(val wire: String) {
    SPLIT_H("splith"),
    SPLIT_V("splitv"),
    TABBED("tabbed"),
    STACKING("stacking"),
}

/** Lowest `duration_s` of `keep_awake.set` (17.17). The highest is `max_duration_s` of [KeepAwake]. */
const val KEEP_AWAKE_MIN_DURATION_S = 60L

/** Highest workspace number of `window.move` and `workspace.focus`. */
const val MAX_WORKSPACE_NUMBER = 999

@Throws(IOException::class)
suspend fun Session.moveWindowToWorkspace(windowId: Long, workspaceId: Long) {
    requireId(workspaceId)
    request(Op.WINDOW_MOVE, windowArgs(windowId) { put("workspace_id", workspaceId) })
}

/** Sway creates the workspace when no workspace has [number]. */
@Throws(IOException::class)
suspend fun Session.moveWindowToNumber(windowId: Long, number: Int) {
    require(number in 0..MAX_WORKSPACE_NUMBER) { "workspace number must be 0 to $MAX_WORKSPACE_NUMBER" }
    request(Op.WINDOW_MOVE, windowArgs(windowId) { put("number", number) })
}

@Throws(IOException::class)
suspend fun Session.setFloating(windowId: Long, enabled: Boolean) {
    request(Op.WINDOW_FLOATING, windowArgs(windowId) { put("enabled", enabled) })
}

/** Changes the layout of the container that holds the window. */
@Throws(IOException::class)
suspend fun Session.setLayout(windowId: Long, layout: WindowLayout) {
    request(Op.WINDOW_LAYOUT, windowArgs(windowId) { put("layout", layout.wire) })
}

@Throws(IOException::class)
suspend fun Session.moveToScratchpad(windowId: Long) {
    request(Op.WINDOW_SCRATCHPAD, windowArgs(windowId))
}

/** [windowId] is the id of a [ScratchpadWindow]; the ids of [Desktop.windows] are `not_found` here. */
@Throws(IOException::class)
suspend fun Session.showScratchpad(windowId: Long) {
    request(Op.SCRATCHPAD_SHOW, windowArgs(windowId))
}

/** Asks the application to close; it may refuse or ask to save. Ask the user to confirm first (17.7). */
@Throws(IOException::class)
suspend fun Session.closeWindow(windowId: Long) {
    request(Op.WINDOW_CLOSE, windowArgs(windowId))
}

/**
 * Runs the server's lock command. Returns `already_locked`: true when the screen was locked, or a lock command
 * still ran, so the server started nothing.
 *
 * @throws MalformedMessageException when the result breaks the wire format.
 */
@Throws(IOException::class)
suspend fun Session.lockScreen(): Boolean =
    Wire.decode(LockResult.serializer(), request(Op.SESSION_LOCK, JsonObject(emptyMap()))).alreadyLocked

/**
 * Starts keep-awake, or replaces its expiry, for [durationS] seconds from now; the server's default when null.
 * Returns `remaining_ms`, which the server may have shortened to its continuous cap.
 *
 * @param durationS at least [KEEP_AWAKE_MIN_DURATION_S] and at most [KeepAwake.maxDurationS].
 * @throws RequestFailedException with [ErrorCode.LIMIT_REACHED] while the cap applies; wait `detail.retry_after_ms`.
 * @throws MalformedMessageException when the result breaks the wire format.
 */
@Throws(IOException::class)
suspend fun Session.keepAwakeOn(durationS: Long? = null): Long? {
    require(durationS == null || durationS in KEEP_AWAKE_MIN_DURATION_S..MAX_SAFE_INTEGER) {
        "keep-awake lasts at least $KEEP_AWAKE_MIN_DURATION_S s"
    }
    val args = buildJsonObject {
        put("enabled", true)
        durationS?.let { put("duration_s", it) }
    }
    return Wire.decode(KeepAwakeResult.serializer(), request(Op.KEEP_AWAKE_SET, args)).remainingMs
}

/**
 * Ends keep-awake, whichever device started it.
 *
 * @throws MalformedMessageException when the result breaks the wire format.
 */
@Throws(IOException::class)
suspend fun Session.keepAwakeOff() {
    val args = buildJsonObject { put("enabled", false) }
    Wire.decode(KeepAwakeResult.serializer(), request(Op.KEEP_AWAKE_SET, args))
}

/** Every id a client sends is one the server issued: an `int`. */
internal fun requireId(id: Long) = require(id in 0..MAX_SAFE_INTEGER) { "an id is an int of the protocol" }

private fun windowArgs(windowId: Long, more: JsonObjectBuilder.() -> Unit = {}): JsonObject {
    requireId(windowId)
    return buildJsonObject {
        put("window_id", windowId)
        more()
    }
}
