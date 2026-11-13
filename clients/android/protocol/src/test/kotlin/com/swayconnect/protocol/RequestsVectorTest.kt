package com.swayconnect.protocol

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * requests.json describes what the server answers. A client can check two things with it:
 * that it sends these requests as printed, and that it reports each answer as the file says.
 */
class RequestsVectorTest {
    private val cases = Vectors.load("requests.json").jsonArray

    @Test
    fun `every payload passes the frame codec unchanged`() {
        for (case in cases) {
            val payload = parseObject(case.text("payload"))
            assertEquals(case.text("name"), payload, throughCodec(payload, CLIENT_FRAME_LIMIT))
        }
    }

    @Test
    fun `the client sends each request as printed and reports the expected outcome`() {
        val sendable = cases.filter { isRequestTheClientCanSend(parseObject(it.text("payload"))) }
        assertTrue("the file has requests a client can send", sendable.isNotEmpty())

        for (case in sendable) {
            val request = parseObject(case.text("payload"))
            val expect = case.jsonObject.getValue("expect")
            if (request.text("op") in OFFERED_OPS) {
                assertOutcome(case.text("name"), request, expect)
            } else {
                assertNeverSent(case.text("name"), request)
            }
        }
    }

    /** A `req` with an id and with `args` absent or an object. The other cases are frames no client API produces. */
    private fun isRequestTheClientCanSend(payload: JsonObject): Boolean {
        val isRequest = (payload["type"] as? JsonPrimitive)?.content == "req" && "id" in payload && "op" in payload
        return isRequest && (payload["args"] == null || payload["args"] is JsonObject)
    }

    private fun assertOutcome(name: String, request: JsonObject, expect: JsonElement) {
        // The client numbers its own requests, so the id is not compared and the answer carries id 1.
        val script = Fixtures.authHandshake + Step.Expect(request) + Step.Send(answerTo(expect)) + Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint, ignoredMembers = setOf("id"))
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var failure: RequestFailedException? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                try {
                    session.request(request.text("op"), request["args"] as? JsonObject)
                } catch (e: RequestFailedException) {
                    failure = e
                }
                server.release()
            }
        }

        if (expect is JsonObject) {
            assertEquals(name, expect.text("error"), failure?.code)
            expect["detail"]?.let { assertEquals(name, concrete(it.jsonObject), failure?.detail) }
        } else {
            assertEquals(name, null, failure)
        }
    }

    /** docs/PROTOCOL.md 13.2 rule 7: an operation that `welcome.ops` does not list is refused before it is sent. */
    private fun assertNeverSent(name: String, request: JsonObject) {
        val server = ScriptedServer(Fixtures.authHandshake + Step.AwaitRelease, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var failure: RequestFailedException? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                try {
                    session.request(request.text("op"), request["args"] as? JsonObject)
                    fail("$name: sent an operation that the server does not offer")
                } catch (e: RequestFailedException) {
                    failure = e
                }
                server.release()
            }
        }

        assertEquals(name, RequestFailedException.NOT_OFFERED, failure?.code)
    }

    private fun answerTo(expect: JsonElement): JsonObject =
        buildJsonObject {
            put("type", "res")
            put("id", 1)
            put("ok", expect !is JsonObject)
            if (expect is JsonObject) {
                putJsonObject("error") {
                    put("code", expect.text("error"))
                    put("message", "from the vector file")
                    expect["detail"]?.let { put("detail", concrete(it.jsonObject)) }
                }
            } else {
                putJsonObject("result") {}
            }
        }

    /** README rule 6: a member given as `{"min", "max"}` stands for any value in that range. The lowest is used. */
    private fun concrete(detail: JsonObject): JsonObject =
        JsonObject(detail.mapValues { (_, value) -> (value as? JsonObject)?.get("min") ?: value })

    private companion object {
        const val CLIENT_FRAME_LIMIT = 16384

        /** `welcome.ops` of T2. */
        val OFFERED_OPS = setOf(Op.PING, Op.STATE_SUBSCRIBE, Op.WORKSPACE_FOCUS, Op.WINDOW_FOCUS, Op.WINDOW_FULLSCREEN)
    }
}
