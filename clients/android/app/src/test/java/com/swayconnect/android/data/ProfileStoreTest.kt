package com.swayconnect.android.data

import com.swayconnect.protocol.Profile
import com.swayconnect.protocol.Proofs
import java.io.File
import java.io.IOException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ProfileStoreTest {
    @get:Rule val directory = TemporaryFolder()

    private val profile = Profile(host = "192.0.2.1", port = 4000, fp = "fp", deviceId = "device", serverName = "desk")
    private val oldKey = ByteArray(Proofs.KEY_BYTES) { 1 }
    private val newKey = ByteArray(Proofs.KEY_BYTES) { 2 }

    @Test
    fun `a saved pairing is loaded again`() {
        val store = ProfileStore(File(directory.root, "profile.json"), MemorySecretsStore(oldKey))

        store.save(StoredPairing(profile, newKey))

        val loaded = store.load()
        assertEquals(profile, loaded?.profile)
        assertArrayEquals(newKey, loaded?.key)
    }

    @Test
    fun `a profile that cannot be written leaves the stored key alone`() {
        val secrets = MemorySecretsStore(oldKey)
        val store = ProfileStore(File(File(directory.root, "missing"), "profile.json"), secrets)

        assertThrows(IOException::class.java) { store.save(StoredPairing(profile, newKey)) }

        assertSame(oldKey, secrets.key)
    }
}

/** Keeps the device key in memory, where AndroidKeyStore is not available. */
internal class MemorySecretsStore(var key: ByteArray?) : SecretsStore {
    override fun deviceKey(): ByteArray? = key

    override fun setDeviceKey(key: ByteArray?) {
        this.key = key
    }
}
