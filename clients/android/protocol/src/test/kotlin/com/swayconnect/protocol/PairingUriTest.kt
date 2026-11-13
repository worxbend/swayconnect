package com.swayconnect.protocol

import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** Cases that protocol/vectors/v1/pairing-uri.json does not hold. */
class PairingUriTest {
    private val validUri = Vectors.load("pairing-uri.json").jsonObject.getValue("valid").jsonArray.first().text("uri")

    /** docs/PROTOCOL.md section 4 rule 1: the case of scheme and authority is ignored, in ASCII only. */
    @Test
    fun `a scheme or an authority with a non-ASCII lookalike is refused`() {
        for (prefix in listOf("ſwayconnect://pair", "swayconnect://paır")) {
            val uri = prefix + validUri.substringAfter("swayconnect://pair")

            assertThrows(PairingUriException::class.java) { parsePairingUri(uri) }
        }
    }

    /** A character outside the BMP is two chars in a JVM string. The Rust parser works on bytes and keeps it. */
    @Test
    fun `a name with a character outside the BMP that is not percent-encoded is kept`() {
        val uri = validUri.substringBefore("&name=") + "&name=desk\uD83D\uDDA5%20two"

        assertEquals("desk\uD83D\uDDA5 two", parsePairingUri(uri).name)
    }
}
