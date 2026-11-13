package com.swayconnect.android.data

import java.security.GeneralSecurityException
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * AES/GCM/NoPadding blob cipher. It knows nothing about AndroidKeyStore, so JUnit can test the
 * round trip with any [SecretKey]. The production key comes from [KeystoreSecretsStore].
 */
internal class AesGcmCipher(private val key: SecretKey) {

    /** Encrypts to base64(IV || ciphertext and tag). The provider picks a fresh IV per call. */
    @Throws(GeneralSecurityException::class)
    fun encrypt(plaintext: ByteArray): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv
        val ciphertext = cipher.doFinal(plaintext)
        return Base64.getEncoder().encodeToString(iv + ciphertext)
    }

    /**
     * Decrypts base64(IV || ciphertext and tag).
     *
     * @throws GeneralSecurityException on a wrong key or a changed blob.
     * @throws IllegalArgumentException when the blob is not base64 or is too short.
     */
    @Throws(GeneralSecurityException::class)
    fun decrypt(blob: String): ByteArray {
        val bytes = Base64.getDecoder().decode(blob)
        require(bytes.size > GCM_IV_LENGTH) { "ciphertext blob too short" }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_LENGTH_BITS, bytes, 0, GCM_IV_LENGTH))
        return cipher.doFinal(bytes, GCM_IV_LENGTH, bytes.size - GCM_IV_LENGTH)
    }

    private companion object {
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val GCM_IV_LENGTH = 12
        const val GCM_TAG_LENGTH_BITS = 128
    }
}
