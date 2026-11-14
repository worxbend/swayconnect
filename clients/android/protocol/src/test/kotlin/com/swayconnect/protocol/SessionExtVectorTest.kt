package com.swayconnect.protocol

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** Replays protocol/vectors/v1/ext/session.json (docs/PROTOCOL.md 17.6 and 17.8). */
class SessionExtVectorTest {
    private val file = Vectors.load("ext/session.json").jsonObject
    private val states = file.getValue("states").jsonArray
    private val results = file.getValue("results").jsonArray

    @Test
    fun `ok states decode and re-encode to the same value`() {
        val okCases = states.filter { it.jsonObject["expect"] == JsonPrimitive("ok") }
        assertTrue(okCases.size >= 10)
        for (case in okCases) {
            val state = stateOf(case.text("payload"))
            val encoded = Wire.json.encodeToJsonElement(SessionStateSerializer, Wire.decode(SessionStateSerializer, state))
            assertEquals(case.text("name"), state, encoded)
        }
    }

    @Test
    fun `bad states are refused`() {
        val badCases = states.filter { it.jsonObject["expect"] == JsonPrimitive("bad") }
        assertTrue(badCases.size >= 20)
        for (case in badCases) {
            val state = stateOf(case.text("payload"))
            assertThrows(case.text("name"), MalformedMessageException::class.java) { Wire.decode(SessionStateSerializer, state) }
        }
    }

    /** An unknown holder is `other_device` and unknown members are ignored, both while decoding. */
    @Test
    fun `normalizing gives the normalized state`() {
        val normalizedCases = states.filter { it.jsonObject["expect"] is JsonObject }
        assertTrue(normalizedCases.size >= 3)
        for (case in normalizedCases) {
            val expected = stateOf(case.jsonObject.getValue("expect").text("normalized"))
            val actual = Wire.decode(SessionStateSerializer, stateOf(case.text("payload")))
            assertEquals(case.text("name"), Wire.decode(SessionStateSerializer, expected), actual)
            assertEquals(case.text("name"), expected, Wire.json.encodeToJsonElement(SessionStateSerializer, actual))
        }
    }

    @Test
    fun `results are accepted or refused as the file says`() {
        for (case in results) {
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

    /** What the typed calls of ControlOps.kt decode from the result of [op]; the window ops read nothing. */
    private fun readResult(op: String, result: JsonObject): Any? =
        when (op) {
            Op.KEEP_AWAKE_SET -> Wire.decode(KeepAwakeResult.serializer(), result).remainingMs
            Op.SESSION_LOCK -> Wire.decode(LockResult.serializer(), result).alreadyLocked
            Op.WINDOW_MOVE -> null
            else -> error("no result reader for $op")
        }

    private fun stateOf(eventPayload: String): JsonObject {
        val event = Wire.decodeServerMessage(parseObject(eventPayload)) as Evt
        val topicState = Wire.decode(TopicState.serializer(), event.data)
        assertEquals(Topic.SESSION, topicState.topic)
        return topicState.data
    }
}
