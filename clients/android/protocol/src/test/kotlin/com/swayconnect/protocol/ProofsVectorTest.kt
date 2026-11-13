package com.swayconnect.protocol

import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProofsVectorTest {
    private val vectors = Vectors.load("proofs.json").jsonObject
    private val inputs = vectors.getValue("inputs")
    private val expect = vectors.getValue("expect")

    private val secret = hexToBytes(inputs.text("s"))
    private val deviceId = inputs.text("device_id")
    private val pairing = contextOf("pairing")
    private val later = contextOf("later")
    private val key = Proofs.deviceKey(secret, pairing)

    private fun contextOf(connection: String): ByteArray {
        val nonces = inputs.jsonObject.getValue(connection)
        return Proofs.context(
            hexToBytes(inputs.text("fp")),
            hexToBytes(nonces.text("server_nonce")),
            hexToBytes(nonces.text("client_nonce")),
        )
    }

    @Test
    fun `pairing connection values`() {
        assertEquals(expect.text("pair_proof"), Proofs.pairProof(secret, pairing).toHex())
        assertEquals(expect.text("k"), key.toHex())
        assertEquals(expect.text("confirm_mac"), Proofs.confirmMac(key, pairing).toHex())
        assertEquals(expect.text("confirm_code"), Proofs.confirmCode(key, pairing))
        assertEquals(expect.text("server_proof_pairing"), Proofs.serverProof(key, pairing, deviceId).toHex())
    }

    @Test
    fun `later connection values`() {
        assertEquals(expect.text("client_proof_later"), Proofs.clientProof(key, later, deviceId).toHex())
        assertEquals(expect.text("server_proof_later"), Proofs.serverProof(key, later, deviceId).toHex())
    }

    @Test
    fun `matches compares whole proofs`() {
        val proof = Proofs.pairProof(secret, pairing)

        assertTrue(Proofs.matches(proof, proof.copyOf()))
        assertFalse(Proofs.matches(proof, proof.copyOf().also { it[31] = (it[31] + 1).toByte() }))
        assertFalse(Proofs.matches(proof, proof.copyOf(31)))
    }
}
