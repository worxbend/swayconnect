package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// Topic `apps`, `app.launch` and `launcher.open` (docs/PROTOCOL.md 17.11). An entry id goes back to the server
// only as it came: the server matches it exactly against its own list and never puts it into a command.

/** State of topic `apps`. It never carries an Exec line or a path. */
@Serializable
data class AppsState(
    /** Sorted by name. */
    val apps: List<App>,
    /** Up to 8 entry ids, most recent first. */
    val recent: List<String>,
    @Serializable(with = WireBool::class) val truncated: Boolean,
)

@Serializable
data class App(
    @SerialName("entry_id") val entryId: String,
    val name: String,
    @SerialName("generic_name") val genericName: String?,
    val keywords: List<String>,
    /** Icon hash for [Session.iconGet]. */
    val icon: String?,
    /** Runs in a terminal. */
    @Serializable(with = WireBool::class) val terminal: Boolean,
    /** Launching it needs scope `launch.risky`; ask the user first (17.19 item 18). */
    @Serializable(with = WireBool::class) val risky: Boolean,
)

/** The form of a desktop entry id (17.11): 1 to 255 characters of `A-Z a-z 0-9 . _ -`. */
object AppEntryId {
    const val MAX_BYTES = 255

    fun isValid(text: String): Boolean = text.length in 1..MAX_BYTES && text.all { it in ENTRY_ID_CHARACTERS }

    private const val ENTRY_ID_CHARACTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-"
}

/**
 * Starts the app [entryId], first focusing workspace [number] when given. `ok` means started, not that a window
 * appeared.
 *
 * @param entryId an [App.entryId] of the current `apps` state, for which [AppEntryId.isValid] holds.
 * @param number 0 to [MAX_WORKSPACE_NUMBER].
 * @throws RequestFailedException with `not_found` when the id left the state, `forbidden` (`detail.scope`
 *   `launch.risky`) for a risky entry, `spawn_failed` when the server could not start it.
 */
@Throws(IOException::class)
suspend fun Session.launchApp(entryId: String, number: Int? = null) {
    require(AppEntryId.isValid(entryId)) { "an entry id is 1 to ${AppEntryId.MAX_BYTES} characters of A-Z a-z 0-9 . _ -" }
    require(number == null || number in 0..MAX_WORKSPACE_NUMBER) { "workspace number must be 0 to $MAX_WORKSPACE_NUMBER" }
    val args = buildJsonObject {
        put("entry_id", entryId)
        number?.let { put("number", it) }
    }
    request(Op.APP_LAUNCH, args)
}

/**
 * Runs the server's launcher. Returns `already_open`: true when a launcher ran already, so this device did not
 * become its holder unless it was already.
 *
 * @throws MalformedMessageException when the result breaks the wire format.
 */
@Throws(IOException::class)
suspend fun Session.openLauncher(): Boolean =
    Wire.decode(LauncherOpenResult.serializer(), request(Op.LAUNCHER_OPEN, JsonObject(emptyMap()))).alreadyOpen

@Serializable
internal class LauncherOpenResult(@SerialName("already_open") @Serializable(with = WireBool::class) val alreadyOpen: Boolean)
