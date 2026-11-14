package com.swayconnect.protocol

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Replays `requests` of protocol/vectors/v1/ext/{audio,media,apps,input}.json (docs/PROTOCOL.md 17.9 to 17.13)
 * from the client side. Each case whose payload a typed call of the client sends exactly is run through that call
 * ([ExtRequestReplay]); the others hold a wrong type, a `null`, an unknown member or a missing member, which the
 * typed calls cannot produce.
 */
class DevicesRequestsExtVectorTest {
    @Test
    fun `audio requests`() = assertRequests("audio", setOf(Op.AUDIO_SET_VOLUME, Op.AUDIO_SET_MUTE, Op.AUDIO_SET_DEFAULT), minTyped = 30)

    @Test
    fun `media requests`() = assertRequests("media", MediaCommand.entries.map { it.op }.toSet(), minTyped = 15)

    @Test
    fun `apps requests`() = assertRequests("apps", setOf(Op.APP_LAUNCH, Op.LAUNCHER_OPEN), minTyped = 30)

    @Test
    fun `input requests`() = assertRequests("input", setOf(Op.INPUT_TEXT, Op.INPUT_KEY), minTyped = 120)

    private fun assertRequests(name: String, ops: Set<String>, minTyped: Int) {
        val file = Vectors.load("ext/$name.json").jsonObject
        val replay = ExtRequestReplay { case, topic -> topicState(file, case, topic) }
        val typed = file.getValue("requests").jsonArray.mapNotNull { case -> typedCall(replay, case)?.let { case to it } }
        assertTrue("$name: ${typed.size} typed cases", typed.size >= minTyped)
        val subscribes = if (name == "input") emptySet() else setOf(Op.STATE_SUBSCRIBE)
        assertEquals(name, ops + subscribes, typed.map { (case, _) -> requestOf(case).text("op") }.toSet())
        for ((case, call) in typed) replay.assertCase(case, call, breaksClientBound(requestOf(case)))
    }

    /** `server_states` by the name the case gives; a `desktop` subscription gets ext/desktop.json case 03. */
    private fun topicState(file: JsonObject, case: JsonElement, topic: String): JsonObject {
        if (topic == Topic.DESKTOP) return ExtRequestReplay.desktopCase("03-")
        val stateName = case.jsonObject[topic]?.let { (it as JsonPrimitive).content } ?: "main"
        return file.getValue("server_states").jsonObject.getValue(stateName).jsonObject.getValue("state").jsonObject
    }

    /** The typed call that sends exactly the case's request (the id aside), or null when no typed call does. */
    private fun typedCall(replay: ExtRequestReplay, case: JsonElement): TypedCall? {
        val request = requestOf(case)
        val args = request["args"] as? JsonObject ?: return null
        val names = args.keys
        return when (val op = request.text("op")) {
            Op.AUDIO_SET_VOLUME -> {
                val deviceId = args.int("device_id")
                val volume = args.int("volume_pct")?.takeIf { it in Int.MIN_VALUE..Int.MAX_VALUE }
                if (deviceId == null || volume == null || names != setOf("device_id", "volume_pct")) null
                else unit { it.setVolume(deviceId, volume.toInt()) }
            }
            Op.AUDIO_SET_MUTE -> {
                val deviceId = args.int("device_id")
                val muted = args.bool("muted")
                if (deviceId == null || muted == null || names != setOf("device_id", "muted")) null
                else unit { it.setMuted(deviceId, muted) }
            }
            Op.AUDIO_SET_DEFAULT -> args.int("device_id")?.takeIf { names == setOf("device_id") }
                ?.let { deviceId -> unit { it.setDefaultDevice(deviceId) } }
            in MediaCommand.entries.map { it.op } -> args.int("player_id")?.takeIf { names == setOf("player_id") }
                ?.let { playerId -> unit { it.controlMedia(playerId, MediaCommand.entries.single { command -> command.op == op }) } }
            Op.APP_LAUNCH -> launchCall(args)
            Op.LAUNCHER_OPEN -> if (names.isEmpty()) { session -> mapOf("already_open" to JsonPrimitive(session.openLauncher())) } else null
            Op.INPUT_TEXT -> {
                val target = targetOf(args)
                val text = args.string("text")
                if (target == null || text == null || names != target.second + "text") null
                else { session -> mapOf("delivered_chars" to JsonPrimitive(session.typeText(target.first, text))) }
            }
            Op.INPUT_KEY -> keyCall(args)
            Op.STATE_SUBSCRIBE -> topicsOf(args)?.let { replay.subscribeCall(case, it) }
            else -> null
        }
    }

    private fun launchCall(args: JsonObject): TypedCall? {
        val entryId = args.string("entry_id") ?: return null
        return when (args.keys) {
            setOf("entry_id") -> unit { it.launchApp(entryId) }
            setOf("entry_id", "number") -> args.int("number")?.takeIf { it in Int.MIN_VALUE..Int.MAX_VALUE }
                ?.let { number -> unit { it.launchApp(entryId, number.toInt()) } }
            else -> null
        }
    }

    /** `modifiers` is sent only when not empty and `repeat` only when not 1, as the typed call does. */
    private fun keyCall(args: JsonObject): TypedCall? {
        val (target, targetNames) = targetOf(args) ?: return null
        val key = args.string("key")?.let(InputKey::of) ?: return null
        val modifiers = when (val listed = args["modifiers"]) {
            null -> emptySet()
            is JsonArray -> listed.map { wire -> KeyModifier.entries.singleOrNull { JsonPrimitive(it.wire) == wire } ?: return null }
                .takeIf { it.isNotEmpty() && it.toSet().size == it.size }?.toSet() ?: return null
            else -> return null
        }
        val repeat = if ("repeat" in args) args.int("repeat")?.takeIf { it != 1L && it in Int.MIN_VALUE..Int.MAX_VALUE } ?: return null else 1L
        if (args.keys != targetNames + setOfNotNull("key", "modifiers".takeIf { it in args }, "repeat".takeIf { it in args })) return null
        return unit { it.pressKey(target, key, modifiers, repeat.toInt()) }
    }

    /** Exactly one of `window_id` (an integer) or `launcher: true`, with the member names it takes. */
    private fun targetOf(args: JsonObject): Pair<InputTarget, Set<String>>? {
        val hasWindow = "window_id" in args
        val hasLauncher = "launcher" in args
        return when {
            hasWindow && !hasLauncher -> args.int("window_id")?.let { InputTarget.Window(it) to setOf("window_id") }
            hasLauncher && !hasWindow && args.bool("launcher") == true -> InputTarget.Launcher to setOf("launcher")
            else -> null
        }
    }

    /**
     * The bounds the typed calls check before sending: ids are ints, a volume is at most 150, a workspace number
     * is 0 to 999, an entry id has its form, input text its rule, no `ctrl`+`alt` with a function key, `repeat` is
     * 1 to 32, topics are distinct. The server's own maximum volume and its id lists are the server's to check.
     */
    private fun breaksClientBound(request: JsonObject): Boolean {
        val args = request.getValue("args").jsonObject
        val ids = listOfNotNull(args.int("device_id"), args.int("player_id"), args.int("window_id"))
        val topics = topicsOf(args)
        val modifiers = (args["modifiers"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.content }
        val vtSwitch = args.string("key")?.let(InputKey::of)?.isFunctionKey == true && "ctrl" in modifiers && "alt" in modifiers
        return ids.any { it !in 0..MAX_SAFE_INTEGER } ||
            args.int("volume_pct")?.let { it !in 0..MAX_VOLUME_PCT_HIGHEST } == true ||
            args.int("number")?.let { it !in 0..MAX_WORKSPACE_NUMBER.toLong() } == true ||
            args.string("entry_id")?.let { !AppEntryId.isValid(it) } == true ||
            args.string("text")?.let { !InputText.isValid(it) } == true ||
            vtSwitch ||
            args.int("repeat")?.let { it !in 1..MAX_KEY_REPEAT.toLong() } == true ||
            (topics != null && topics.toSet().size != topics.size)
    }

    private fun unit(call: suspend (Session) -> Unit): TypedCall = { session ->
        call(session)
        emptyMap()
    }
}
