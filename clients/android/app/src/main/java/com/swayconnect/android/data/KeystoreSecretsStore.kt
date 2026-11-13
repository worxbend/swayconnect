package com.swayconnect.android.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.AEADBadTagException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * [SecretsStore] with an AES-256-GCM key in AndroidKeyStore. Only the encrypted blob is kept,
 * in a private SharedPreferences file.
 */
class KeystoreSecretsStore(context: Context) : SecretsStore {

    private val prefs = context.applicationContext.getSharedPreferences(PREFS_FILE, Context.MODE_PRIVATE)

    override fun deviceKey(): ByteArray? {
        val blob = prefs.getString(BLOB_NAME, null) ?: return null
        return try {
            AesGcmCipher(wrappingKey()).decrypt(blob)
        } catch (failure: GeneralSecurityException) {
            throw unreadable(failure)
        } catch (failure: IllegalArgumentException) {
            throw unreadable(failure)
        } catch (failure: ProviderException) {
            // How the platform reports a Keystore that does not answer. It is unchecked.
            throw unreadable(failure)
        }
    }

    override fun setDeviceKey(key: ByteArray?) {
        val edit = prefs.edit()
        if (key == null) {
            edit.remove(BLOB_NAME)
        } else {
            edit.putString(BLOB_NAME, encrypt(key))
        }
        if (!edit.commit()) throw IOException("the device key could not be written")
    }

    private fun encrypt(key: ByteArray): String =
        try {
            AesGcmCipher(wrappingKey()).encrypt(key)
        } catch (failure: GeneralSecurityException) {
            throw IOException("the device key could not be protected", failure)
        } catch (failure: ProviderException) {
            // Unchecked Keystore failure; without this it escapes pair()'s IOException handling.
            throw IOException("the device key could not be protected", failure)
        }

    /**
     * The blob is deleted only when it can never decrypt again: a wrong tag, an invalidated key or
     * damaged data. Every other failure may be a Keystore that does not answer right now, and the
     * blob stays for the next start.
     */
    private fun unreadable(cause: Exception): SecretsDecryptException {
        val lostForGood =
            cause is AEADBadTagException || cause is KeyPermanentlyInvalidatedException || cause is IllegalArgumentException
        if (lostForGood) prefs.edit().remove(BLOB_NAME).apply()
        return SecretsDecryptException(cause)
    }

    @Throws(GeneralSecurityException::class)
    private fun wrappingKey(): SecretKey {
        val keyStore =
            try {
                KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
            } catch (failure: IOException) {
                throw GeneralSecurityException("AndroidKeyStore could not be opened", failure)
            }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let {
            return it.secretKey
        }

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(AES_KEY_BITS)
                .build()
        )
        return generator.generateKey()
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "swayconnect_device_key"
        const val PREFS_FILE = "swayconnect_secrets"
        const val BLOB_NAME = "device_key"
        const val AES_KEY_BITS = 256
    }
}
