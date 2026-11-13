package com.swayconnect.android.data

import java.io.IOException

/**
 * Protected storage for the device key `K` (docs/PROTOCOL.md section 16 rule 14).
 * The key never goes to the profile file or to a log. Both calls block.
 */
interface SecretsStore {
    /**
     * Returns the stored key, or null when none was saved.
     *
     * @throws SecretsDecryptException when a blob exists but cannot be decrypted. A blob that can
     *   never decrypt again (invalidated key, damaged data) is deleted before the throw; after a
     *   failure of the key storage itself it stays, and a later call may succeed.
     */
    @Throws(SecretsDecryptException::class)
    fun deviceKey(): ByteArray?

    /** `null` removes the stored key. */
    @Throws(IOException::class)
    fun setDeviceKey(key: ByteArray?)
}

/** The stored key blob cannot be read. It carries no key material. */
class SecretsDecryptException(cause: Exception) : Exception("the stored device key is no longer readable", cause)
