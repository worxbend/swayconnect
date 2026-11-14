package com.swayconnect.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Replays `states`, `results` and `errors` of protocol/vectors/v1/ext/{audio,media,apps,input}.json
 * (docs/PROTOCOL.md 17.9 to 17.13, README "M2b files: shared rules").
 */
class DevicesExtVectorTest {
    @Test
    fun `audio states are read as the file says`() = assertStates("audio", Topic.AUDIO, AudioState.serializer())

    @Test
    fun `media states are read as the file says`() = assertStates("media", Topic.MEDIA, MediaState.serializer())

    @Test
    fun `apps states are read as the file says`() = assertStates("apps", Topic.APPS, AppsState.serializer())

    /** The states a server publishes after the cut rule of 17.11, up to exactly 262144 bytes, are read whole. */
    @Test
    fun `apps states after the cut are read`() {
        val cuts = file("apps").getValue("cut").jsonArray
        assertTrue(cuts.size >= 5)
        for (case in cuts) {
            val cut = case.jsonObject["expect"]?.takeIf { it != JsonNull }?.jsonPrimitive?.content
            val published = Wire.decode(AppsState.serializer(), parseObject(cut ?: case.text("input")))
            assertEquals(case.text("name"), cut != null, published.truncated)
        }
    }

    @Test
    fun `results are accepted or refused as the file says`() {
        val cases = listOf("audio", "media", "apps", "input").flatMap { file(it).getValue("results").jsonArray }
        assertTrue(cases.size >= 20)
        for (case in cases) {
            val name = case.text("name")
            val result = (Wire.decodeServerMessage(parseObject(case.text("payload"))) as Res).result!!
            val read = { readResult(case.text("op"), result) }
            if (case.jsonObject.getValue("expect") == JsonPrimitive("bad")) {
                assertThrows(name, MalformedMessageException::class.java) { read() }
            } else {
                read()
            }
        }
    }

    /** `expect` lists the code and the detail members a client reads; input refusals are read by [InputRefusal]. */
    @Test
    fun `errors give the code and the detail members the client reads`() {
        val cases = listOf("audio", "input").flatMap { file(it).getValue("errors").jsonArray }
        assertTrue(cases.size >= 8)
        for (case in cases) {
            val name = case.text("name")
            val error = (Wire.decodeServerMessage(parseObject(case.text("payload"))) as Res).error!!
            val failure = RequestFailedException(error.code, error.message, error.detail)
            val expect = case.jsonObject.getValue("expect").jsonObject
            assertEquals(name, expect.text("code"), failure.code)
            val refusal = InputRefusal.of(failure)
            assertEquals(name, case.text("op").startsWith("input."), refusal != null)
            for ((member, value) in expect - "code") {
                val read = when (member) {
                    "delivered_chars" -> JsonPrimitive(refusal!!.deliveredChars)
                    "focused_window_id" -> JsonPrimitive(refusal!!.focusedWindowId)
                    else -> failure.detail?.get(member)
                }
                assertEquals("$name: $member", value, read)
            }
        }
    }

    private fun <T> assertStates(name: String, topic: String, serializer: KSerializer<T>) {
        val states = file(name).getValue("states").jsonArray
        val okCases = states.filter { it.jsonObject["expect"] == JsonPrimitive("ok") }
        val badCases = states.filter { it.jsonObject["expect"] == JsonPrimitive("bad") }
        val normalizedCases = states.filter { it.jsonObject["expect"] is JsonObject }
        assertTrue(okCases.size >= 5 && badCases.size >= 15 && normalizedCases.isNotEmpty())
        for (case in okCases) {
            val state = stateOf(case.text("payload"), topic)
            assertEquals(case.text("name"), state, Wire.json.encodeToJsonElement(serializer, Wire.decode(serializer, state)))
        }
        // A `data` that is not an object is refused with the topic state that carries it.
        for (case in badCases) {
            assertThrows(case.text("name"), MalformedMessageException::class.java) { Wire.decode(serializer, stateOf(case.text("payload"), topic)) }
        }
        // Unknown enum values take their fallback and unknown members are ignored, both while decoding.
        for (case in normalizedCases) {
            val expected = stateOf(case.jsonObject.getValue("expect").text("normalized"), topic)
            val actual = Wire.decode(serializer, stateOf(case.text("payload"), topic))
            assertEquals(case.text("name"), Wire.decode(serializer, expected), actual)
            assertEquals(case.text("name"), expected, Wire.json.encodeToJsonElement(serializer, actual))
        }
    }

    /** What the typed calls decode from the result of [op]; the others read nothing. */
    private fun readResult(op: String, result: JsonObject): Any? =
        when (op) {
            Op.LAUNCHER_OPEN -> Wire.decode(LauncherOpenResult.serializer(), result).alreadyOpen
            Op.INPUT_TEXT -> Wire.decode(TypeTextResult.serializer(), result).deliveredChars
            in OpTable.rules -> null
            else -> error("no result reader for $op")
        }

    private fun stateOf(eventPayload: String, topic: String): JsonObject {
        val event = Wire.decodeServerMessage(parseObject(eventPayload)) as Evt
        val topicState = Wire.decode(TopicState.serializer(), event.data)
        assertEquals(topic, topicState.topic)
        return topicState.data
    }

    private fun file(name: String): JsonObject = Vectors.load("ext/$name.json").jsonObject
}
