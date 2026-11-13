package com.swayconnect.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What a paired client stores about its server. The device key `K` is not part of it:
 * it goes to the protected storage of the platform.
 */
@Serializable
data class Profile(
    val host: String,
    val port: Int,
    /** The pinned fingerprint, base64url. */
    val fp: String,
    @SerialName("device_id") val deviceId: String,
    @SerialName("server_name") val serverName: String,
)
