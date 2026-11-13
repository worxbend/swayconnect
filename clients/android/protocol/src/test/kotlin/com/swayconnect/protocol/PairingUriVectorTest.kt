package com.swayconnect.protocol

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class PairingUriVectorTest {
    private val vectors = Vectors.load("pairing-uri.json").jsonObject

    @Test
    fun `every valid URI is parsed`() {
        for (case in vectors.getValue("valid").jsonArray) {
            val uri = case.text("uri")
            val parsed = parsePairingUri(uri)

            assertEquals(uri, 1, case.jsonObject.getValue("v").jsonPrimitive.int)
            assertEquals(uri, case.text("host"), parsed.host)
            assertEquals(uri, case.jsonObject.getValue("port").jsonPrimitive.int, parsed.port)
            assertEquals(uri, case.text("fp_hex"), parsed.fingerprint.toHex())
            assertEquals(uri, case.text("secret_hex"), parsed.secret.toHex())
            assertEquals(uri, (case.jsonObject.getValue("name") as JsonPrimitive).contentOrNull, parsed.name)
        }
    }

    @Test
    fun `every invalid URI is refused`() {
        for (case in vectors.getValue("invalid").jsonArray) {
            try {
                parsePairingUri(case.text("uri"))
                fail("accepted a URI that is invalid: ${case.text("reason")}")
            } catch (expected: PairingUriException) {
                // The parser refused it, as the vector demands.
            }
        }
    }
}
