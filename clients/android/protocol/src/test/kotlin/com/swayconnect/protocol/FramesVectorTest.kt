package com.swayconnect.protocol

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.SequenceInputStream
import java.util.Collections
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class FramesVectorTest {
    private val cases = Vectors.load("frames.json").jsonArray

    @Test
    fun `decoder gives the frames or the failure of each case`() {
        for (case in cases) {
            val name = case.text("name")
            val limit = case.jsonObject.getValue("limit").jsonPrimitive.int
            val expect = case.jsonObject.getValue("expect")

            if (expect is JsonArray) {
                val payloads = expect.map { parseObject(it.jsonPrimitive.content) }
                assertEquals(name, payloads, readAll(readsOf(case), limit))
            } else {
                assertEquals(name, expect.jsonPrimitive.content, failureOf(readsOf(case), limit))
            }
        }
    }

    @Test
    fun `encoder writes the ping example byte for byte`() {
        val ping = cases.first { it.text("name") == "ping" }
        val frame = parseObject(ping.jsonObject.getValue("expect").jsonArray.single().jsonPrimitive.content)
        val wire = ByteArrayOutputStream()

        FrameCodec.write(wire, frame, Limits.MAX_FRAME_C2S_BEFORE_WELCOME)

        assertEquals(ping.jsonObject.getValue("input_hex").jsonArray.single().jsonPrimitive.content, wire.toByteArray().toHex())
    }

    @Test
    fun `encoder refuses a frame over the limit`() {
        try {
            FrameCodec.write(ByteArrayOutputStream(), parseObject("""{"type":"req","id":5,"op":"ping"}"""), 32)
            fail("wrote a 33 byte frame with limit 32")
        } catch (expected: FrameException) {
            assertEquals(FrameException.Kind.TOO_LARGE, expected.kind)
        }
    }

    /** A stream that ends each read at the end of a chunk of `input_hex`. */
    private fun readsOf(case: JsonElement): InputStream {
        val chunks = case.jsonObject.getValue("input_hex").jsonArray.map { ByteArrayInputStream(hexToBytes(it.jsonPrimitive.content)) }
        return SequenceInputStream(Collections.enumeration(chunks))
    }

    private fun readAll(input: InputStream, limit: Int): List<JsonObject> = generateSequence { FrameCodec.read(input, limit) }.toList()

    /** Reads until the decoder fails and names the failure as the vector file does. */
    private fun failureOf(input: InputStream, limit: Int): String =
        try {
            readAll(input, limit)
            "no failure"
        } catch (e: FrameException) {
            if (e.kind == FrameException.Kind.TOO_LARGE) "too_large" else "bad"
        }
}
