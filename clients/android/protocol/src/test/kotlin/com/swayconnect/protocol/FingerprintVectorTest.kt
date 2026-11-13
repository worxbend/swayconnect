package com.swayconnect.protocol

import java.util.Base64
import kotlinx.serialization.json.jsonArray
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FingerprintVectorTest {
    private val cases = Vectors.load("fingerprint.json").jsonArray

    @Test
    fun `fingerprint is the SHA-256 of the certificate DER`() {
        for (case in cases) {
            val fingerprint = fingerprintOf(Base64.getDecoder().decode(case.text("cert_der_b64")))

            assertEquals(case.text("fp_hex"), fingerprint.toHex())
            assertEquals(case.text("fp_b64url"), Base64Url.encode(fingerprint))
            assertArrayEquals(fingerprint, Base64Url.decode(case.text("fp_b64url"), FINGERPRINT_BYTES))
        }
    }

    @Test
    fun `base64url decoding accepts only the canonical text`() {
        for (case in cases) {
            val canonical = case.text("fp_b64url")
            val nonZeroTrailingBits = canonical.dropLast(1) + (canonical.last() + 1)

            assertNull("padding", Base64Url.decode("$canonical=", FINGERPRINT_BYTES))
            assertNull("trailing bits", Base64Url.decode(nonZeroTrailingBits, FINGERPRINT_BYTES))
            assertNull("standard alphabet", Base64Url.decode(canonical.replace('_', '/'), FINGERPRINT_BYTES))
            assertNull("wrong length", Base64Url.decode(canonical, FINGERPRINT_BYTES - 1))
        }
    }
}
