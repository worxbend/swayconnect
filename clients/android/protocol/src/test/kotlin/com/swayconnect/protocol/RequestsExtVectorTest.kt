package com.swayconnect.protocol

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Replays protocol/vectors/v1/ext/requests.json (docs/PROTOCOL.md 17.6 to 17.8) from the client side.
 * Each case whose payload a typed call of the client sends exactly is run through that call ([ExtRequestReplay]).
 */
class RequestsExtVectorTest {
    private val file = Vectors.load("ext/requests.json").jsonObject

    private val sessionState = ExtRequestReplay.stateOf(Vectors.load("ext/session.json").jsonObject.getValue("states").jsonArray.first().text("payload"))
    private val desktopState = ExtRequestReplay.desktopCase("03-")

    /** A subscribe answer carries a state per topic: the 17.18 `session` example and the `desktop` of case 03. */
    private val replay = ExtRequestReplay(knownOps = ::knownOps) { _, topic -> if (topic == Topic.SESSION) sessionState else desktopState }

    @Test
    fun `typed calls send each request as printed or refuse it, and report the expected outcome`() {
        val typed = file.getValue("requests").jsonArray.mapNotNull { case -> typedCall(case)?.let { case to it } }
        assertTrue("the file has requests the typed calls send", typed.size >= 70)
        val m2aOps = setOf(
            Op.WINDOW_MOVE, Op.WINDOW_FLOATING, Op.WINDOW_LAYOUT, Op.WINDOW_SCRATCHPAD, Op.SCRATCHPAD_SHOW, Op.WINDOW_CLOSE,
            Op.SESSION_LOCK, Op.KEEP_AWAKE_SET, Op.STATE_SUBSCRIBE,
        )
        assertEquals(m2aOps, typed.map { (case, _) -> requestOf(case).text("op") }.toSet())
        for ((case, call) in typed) replay.assertCase(case, call, breaksClientBound(requestOf(case)))
    }

    @Test
    fun `the layouts of window_layout are the ones the file accepts`() {
        val accepted = file.getValue("requests").jsonArray
            .filter { requestOf(it).text("op") == Op.WINDOW_LAYOUT && expectOf(it) == JsonPrimitive("ok") }
            .map { requestOf(it).getValue("args").jsonObject.text("layout") }
        assertEquals(accepted.toSet(), WindowLayout.entries.map { it.wire }.toSet())
    }

    /** The ops of the 17.18 example, less `session.lock` when the owner configured no lock command (17.6 rule 1). */
    private fun knownOps(case: JsonElement): List<String> {
        val lockConfigured = case.jsonObject.getValue("session").jsonObject.getValue("lock_command").jsonPrimitive.boolean
        return ExtRequestReplay.EXAMPLE_KNOWN_OPS.filter { lockConfigured || it != Op.SESSION_LOCK }
    }

    /**
     * The typed call that sends exactly the case's request (the id aside), or null when no typed call does: the
     * client cannot put a wrong type, a `null`, an unknown member or a missing member into a request.
     */
    private fun typedCall(case: JsonElement): TypedCall? {
        val request = requestOf(case)
        val args = request["args"] as? JsonObject ?: return null
        val names = args.keys
        val windowId = args.int("window_id")
        return when (request.text("op")) {
            Op.WINDOW_MOVE -> when {
                windowId == null -> null
                names == setOf("window_id", "number") -> args.int("number")?.takeIf { it in Int.MIN_VALUE..Int.MAX_VALUE }?.let { number ->
                    windowCall { it.moveWindowToNumber(windowId, number.toInt()) }
                }
                names == setOf("window_id", "workspace_id") -> args.int("workspace_id")?.let { workspaceId ->
                    windowCall { it.moveWindowToWorkspace(windowId, workspaceId) }
                }
                else -> null
            }
            Op.WINDOW_FLOATING -> args.bool("enabled")?.takeIf { windowId != null && names == setOf("window_id", "enabled") }
                ?.let { enabled -> windowCall { it.setFloating(windowId!!, enabled) } }
            Op.WINDOW_LAYOUT -> WindowLayout.entries.singleOrNull { args["layout"] == JsonPrimitive(it.wire) }
                ?.takeIf { windowId != null && names == setOf("window_id", "layout") }
                ?.let { layout -> windowCall { it.setLayout(windowId!!, layout) } }
            Op.WINDOW_SCRATCHPAD -> onlyWindowId(args)?.let { id -> windowCall { it.moveToScratchpad(id) } }
            Op.SCRATCHPAD_SHOW -> onlyWindowId(args)?.let { id -> windowCall { it.showScratchpad(id) } }
            Op.WINDOW_CLOSE -> onlyWindowId(args)?.let { id -> windowCall { it.closeWindow(id) } }
            Op.SESSION_LOCK -> typed { session -> mapOf("already_locked" to JsonPrimitive(session.lockScreen())) }.takeIf { names.isEmpty() }
            Op.KEEP_AWAKE_SET -> keepAwakeCall(args)
            Op.STATE_SUBSCRIBE -> topicsOf(args)?.let { replay.subscribeCall(case, it) }
            else -> null
        }
    }

    private fun keepAwakeCall(args: JsonObject): TypedCall? =
        when {
            args.keys == setOf("enabled") && args.bool("enabled") == false -> typed { session ->
                session.keepAwakeOff()
                emptyMap()
            }
            args.keys == setOf("enabled") && args.bool("enabled") == true -> typed { session ->
                mapOf("remaining_ms" to JsonPrimitive(session.keepAwakeOn()))
            }
            args.keys == setOf("enabled", "duration_s") && args.bool("enabled") == true -> args.int("duration_s")?.let { duration ->
                typed { session -> mapOf("remaining_ms" to JsonPrimitive(session.keepAwakeOn(duration))) }
            }
            else -> null
        }

    private fun windowCall(call: suspend (Session) -> Unit): TypedCall = { session ->
        call(session)
        emptyMap()
    }

    private fun typed(call: TypedCall): TypedCall = call

    private fun onlyWindowId(args: JsonObject): Long? = args.int("window_id")?.takeIf { args.keys == setOf("window_id") }

    /**
     * The bounds the typed calls check before sending: ids are ints, a workspace number is 0 to 999,
     * `duration_s` is at least 60, topics are distinct. `max_duration_s` is the server's to check.
     */
    private fun breaksClientBound(request: JsonObject): Boolean {
        val args = request.getValue("args").jsonObject
        val ids = listOfNotNull(args.int("window_id"), args.int("workspace_id"))
        val topics = topicsOf(args)
        return ids.any { it !in 0..MAX_SAFE_INTEGER } ||
            args.int("number")?.let { it !in 0..MAX_WORKSPACE_NUMBER.toLong() } == true ||
            args.int("duration_s")?.let { it < KEEP_AWAKE_MIN_DURATION_S } == true ||
            (topics != null && topics.toSet().size != topics.size)
    }
}
