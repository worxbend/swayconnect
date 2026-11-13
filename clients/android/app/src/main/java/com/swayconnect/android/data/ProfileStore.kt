package com.swayconnect.android.data

import com.swayconnect.protocol.Profile
import com.swayconnect.protocol.Proofs
import com.swayconnect.protocol.Wire
import java.io.File
import java.io.IOException
import kotlinx.serialization.SerializationException

/** One confirmed pairing: what the profile file holds, and the device key `K` from the secrets store. */
class StoredPairing(val profile: Profile, val key: ByteArray)

/** A pairing was stored, but the profile file or the key cannot be read any more. Pair again. */
class StoredPairingLostException(message: String, cause: Exception? = null) : Exception(message, cause)

/**
 * Keeps the one pairing of this app: the profile as a private JSON file, `K` through [SecretsStore].
 * Every call blocks.
 */
class ProfileStore(private val profileFile: File, private val secrets: SecretsStore) {

    /** Returns the stored pairing, or null when the app was never paired or the pairing was forgotten. */
    @Throws(StoredPairingLostException::class)
    fun load(): StoredPairing? {
        if (!profileFile.exists()) return null
        val profile =
            try {
                Wire.json.decodeFromString(Profile.serializer(), profileFile.readText(Charsets.UTF_8))
            } catch (e: IOException) {
                throw StoredPairingLostException("the profile file cannot be read", e)
            } catch (e: SerializationException) {
                throw StoredPairingLostException("the profile file is damaged", e)
            }
        val key =
            try {
                secrets.deviceKey()
            } catch (e: SecretsDecryptException) {
                throw StoredPairingLostException("the device key cannot be decrypted", e)
            }
        if (key == null || key.size != Proofs.KEY_BYTES) throw StoredPairingLostException("the device key is missing")
        return StoredPairing(profile, key)
    }

    /** Replaces the stored pairing. Call it only after `pairing.confirmed` (docs/PROTOCOL.md 5.4). */
    @Throws(IOException::class)
    fun save(pairing: StoredPairing) {
        // Written beside the target and renamed, so a crash never leaves half a profile.
        val draft = File(profileFile.parentFile, profileFile.name + ".tmp")
        draft.writeText(Wire.json.encodeToString(Profile.serializer(), pairing.profile), Charsets.UTF_8)
        // The key is replaced only after the write that a full disk refuses. When that write or this
        // call fails, the old profile and the old key still belong together.
        secrets.setDeviceKey(pairing.key)
        if (!draft.renameTo(profileFile)) throw IOException("the profile file could not be replaced")
    }

    /** Forgets the pairing on this phone. The computer still lists the device until it is revoked there. */
    @Throws(IOException::class)
    fun clear() {
        if (profileFile.exists() && !profileFile.delete()) throw IOException("the profile file could not be deleted")
        secrets.setDeviceKey(null)
    }
}
