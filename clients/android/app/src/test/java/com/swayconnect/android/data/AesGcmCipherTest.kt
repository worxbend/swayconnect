package com.swayconnect.android.data

import java.security.GeneralSecurityException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AesGcmCipherTest {
    private val deviceKey = ByteArray(32) { it.toByte() }

    private fun newKey(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    @Test
    fun `decrypt returns what encrypt got`() {
        val cipher = AesGcmCipher(newKey())

        val blob = cipher.encrypt(deviceKey)

        assertArrayEquals(deviceKey, cipher.decrypt(blob))
    }

    @Test
    fun `two encryptions of one key differ`() {
        val cipher = AesGcmCipher(newKey())

        assertNotEquals(cipher.encrypt(deviceKey), cipher.encrypt(deviceKey))
    }

    @Test
    fun `decrypt with another key fails`() {
        val blob = AesGcmCipher(newKey()).encrypt(deviceKey)
        val other = AesGcmCipher(newKey())

        assertThrows(GeneralSecurityException::class.java) { other.decrypt(blob) }
    }

    @Test
    fun `a changed blob fails`() {
        val cipher = AesGcmCipher(newKey())
        val blob = cipher.encrypt(deviceKey)
        val lastCharacter = if (blob.last() == 'A') 'B' else 'A'
        val changed = blob.dropLast(1) + lastCharacter

        assertThrows(GeneralSecurityException::class.java) { cipher.decrypt(changed) }
    }

    @Test
    fun `a blob without ciphertext is refused`() {
        val cipher = AesGcmCipher(newKey())

        assertThrows(IllegalArgumentException::class.java) { cipher.decrypt("AAAA") }
    }
}
