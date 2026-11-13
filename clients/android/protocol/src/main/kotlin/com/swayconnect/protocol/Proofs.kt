package com.swayconnect.protocol

import java.nio.ByteBuffer
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/** The proof construction of docs/PROTOCOL.md 5.3. Keys and nonces are 32 bytes. */
object Proofs {
    const val NONCE_BYTES = 32
    const val KEY_BYTES = 32
    const val PROOF_BYTES = 32

    private const val HMAC_SHA256 = "HmacSHA256"
    private const val CONFIRM_CODE_MODULUS = 1_000_000L
    private const val CONFIRM_CODE_DIGITS = 6

    /** `ctx = fp || server_nonce || client_nonce`, 96 bytes. */
    fun context(fingerprint: ByteArray, serverNonce: ByteArray, clientNonce: ByteArray): ByteArray {
        require(fingerprint.size == FINGERPRINT_BYTES) { "fingerprint must be $FINGERPRINT_BYTES bytes" }
        require(serverNonce.size == NONCE_BYTES) { "server nonce must be $NONCE_BYTES bytes" }
        require(clientNonce.size == NONCE_BYTES) { "client nonce must be $NONCE_BYTES bytes" }
        return fingerprint + serverNonce + clientNonce
    }

    fun pairProof(secret: ByteArray, context: ByteArray): ByteArray = hmac(secret, "pair", context)

    /** The device key `K`. Computed once, at pairing. */
    fun deviceKey(secret: ByteArray, context: ByteArray): ByteArray = hmac(secret, "key", context)

    fun confirmMac(key: ByteArray, context: ByteArray): ByteArray = hmac(key, "confirm", context)

    fun confirmCode(key: ByteArray, context: ByteArray): String {
        val firstFourBytes = ByteBuffer.wrap(confirmMac(key, context)).int.toLong() and 0xFFFFFFFFL
        // Not String.format: it writes locale digits.
        return (firstFourBytes % CONFIRM_CODE_MODULUS).toString().padStart(CONFIRM_CODE_DIGITS, '0')
    }

    /** [deviceId] is the id string as it appears in JSON; its ASCII bytes are hashed, not the decoded id. */
    fun clientProof(key: ByteArray, context: ByteArray, deviceId: String): ByteArray =
        hmac(key, "client", context + deviceId.toByteArray(Charsets.US_ASCII))

    fun serverProof(key: ByteArray, context: ByteArray, deviceId: String): ByteArray =
        hmac(key, "server", context + deviceId.toByteArray(Charsets.US_ASCII))

    /** Constant-time comparison. */
    fun matches(expected: ByteArray, actual: ByteArray): Boolean = MessageDigest.isEqual(expected, actual)

    private fun hmac(key: ByteArray, label: String, message: ByteArray): ByteArray {
        require(key.size == KEY_BYTES) { "key must be $KEY_BYTES bytes" }
        val mac = Mac.getInstance(HMAC_SHA256)
        mac.init(SecretKeySpec(key, HMAC_SHA256))
        mac.update("${Limits.ALPN} $label".toByteArray(Charsets.US_ASCII))
        mac.update(0.toByte())
        return mac.doFinal(message)
    }
}
