package com.swayconnect.protocol

import java.security.MessageDigest
import java.util.Base64

const val FINGERPRINT_BYTES = 32

/** The server identity: SHA-256 of the DER encoding of the leaf certificate (docs/PROTOCOL.md 3.2). */
fun fingerprintOf(certificateDer: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(certificateDer)

/** base64url without padding (docs/PROTOCOL.md 2.2). */
object Base64Url {
    fun encode(bytes: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)

    /**
     * Returns the decoded bytes, or null when [text] is not the canonical encoding of exactly [size] bytes.
     * The Java decoder accepts padding and non-zero trailing bits, so the text is compared with its re-encoding.
     */
    fun decode(text: String, size: Int): ByteArray? {
        val bytes =
            try {
                Base64.getUrlDecoder().decode(text)
            } catch (e: IllegalArgumentException) {
                return null
            }
        return bytes.takeIf { it.size == size && encode(it) == text }
    }
}
