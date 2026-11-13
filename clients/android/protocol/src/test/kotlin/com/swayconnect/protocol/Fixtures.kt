package com.swayconnect.protocol

import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject

/** The values of docs/PROTOCOL.md 15.2 and the first pairing URI of section 4, as the transcripts use them. */
internal object Fixtures {
    private val proofs = Vectors.load("proofs.json").jsonObject
    private val inputs = proofs.getValue("inputs")
    private val expect = proofs.getValue("expect")

    val fingerprint: ByteArray = hexToBytes(inputs.text("fp"))
    val deviceId: String = inputs.text("device_id")
    val key: ByteArray = hexToBytes(expect.text("k"))
    val confirmCode: String = expect.text("confirm_code")
    val pairingClientNonce: String = inputs.jsonObject.getValue("pairing").text("client_nonce")
    val laterClientNonce: String = inputs.jsonObject.getValue("later").text("client_nonce")

    val pairingUri: PairingUri =
        parsePairingUri(Vectors.load("pairing-uri.json").jsonObject.getValue("valid").jsonArray.first().text("uri"))
    val profile = Profile(pairingUri.host, pairingUri.port, Base64Url.encode(fingerprint), deviceId, "desk")

    /** The client identities that T1 and T2 print. */
    val androidClient = ClientInfo(platform = "android", client = "swayconnect-android/0.1.0")
    val cliClient = ClientInfo(platform = null, client = "swayconnect-cli/0.1.0")
    val anonymousClient = ClientInfo(platform = null, client = null)

    val pairHello: List<Step> = Vectors.transcript("t1-pair-ok").take(1)
    val authHandshake: List<Step> = Vectors.transcript("t2-auth-ok")
    val authHello: List<Step> = authHandshake.take(1)
}
