package com.swayconnect.protocol

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

/**
 * handshake.json holds first frames that the server must reject. A client can check three things with it:
 * the frames fit the limit before `welcome`, its own input rules refuse the values the cases are about,
 * and it reports the `reject` code of each case.
 */
class HandshakeVectorTest {
    private val cases = Vectors.load("handshake.json").jsonArray

    @Test
    fun `every payload passes the frame codec within the limit before welcome`() {
        for (case in cases) {
            val payload = parseObject(case.text("payload"))
            assertEquals(case.text("name"), payload, throughCodec(payload, Limits.MAX_FRAME_C2S_BEFORE_WELCOME))
        }
    }

    @Test
    fun `the client rules refuse the values that the server rejects`() {
        val payloads = cases.associate { it.text("name") to parseObject(it.text("payload")) }

        assertNull(Base64Url.decode(payloads.getValue("device-id-not-canonical").text("device_id"), DEVICE_ID_BYTES))
        assertFalse(isInfoText(payloads.getValue("client-with-control-character").text("client"), Limits.CLIENT_MAX_BYTES))
        assertFalse(isInfoText(payloads.getValue("platform-with-newline").text("platform"), Limits.PLATFORM_MAX_BYTES))
        for (name in listOf("device-name-empty", "device-name-with-u202e", "device-name-over-64-bytes")) {
            assertFalse(name, isDisplayLabel(payloads.getValue(name).text("device_name")))
        }
    }

    @Test
    fun `the client reports the reject code of every case`() {
        for (case in cases) {
            val pairs = case.text("after") == "t1"
            val expectedCode = case.jsonObject.getValue("expect").text("reject")
            // The frame of the case is one a correct client does not send, so the script takes any frame.
            val script = (if (pairs) Fixtures.pairHello else Fixtures.authHello) + Step.ExpectAnyFrame + Step.Send(reject(expectedCode))
            val server = ScriptedServer(script, Fixtures.fingerprint)

            val rejected = assertThrows(case.text("name"), RejectedException::class.java) {
                replay(server) {
                    if (pairs) {
                        clientFor(server, Fixtures.androidClient, Fixtures.pairingClientNonce).pair(Fixtures.pairingUri, "Pixel 9") {}
                    } else {
                        clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce).runSession(Fixtures.profile, Fixtures.key) {}
                    }
                }
            }

            assertEquals(case.text("name"), expectedCode, rejected.code)
        }
    }

    private fun reject(code: String): JsonObject =
        buildJsonObject {
            put("type", "reject")
            put("code", code)
            put("message", "from the vector file")
        }

    private companion object {
        const val DEVICE_ID_BYTES = 16
    }
}
