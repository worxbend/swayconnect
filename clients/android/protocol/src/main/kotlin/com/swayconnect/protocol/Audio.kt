package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// Topic `audio` and its operations (docs/PROTOCOL.md 17.9). Device ids come from the topic. A source write other
// than muting needs scope `audio.capture` as well; the server checks it and answers `forbidden` with `detail.scope`.

/** Bounds of `max_volume_pct` (17.9). A client never sends a volume above the highest. */
const val MAX_VOLUME_PCT_LOWEST = 100L
const val MAX_VOLUME_PCT_HIGHEST = 150L

/** Highest `volume_pct` a device reports (17.9). */
const val DEVICE_VOLUME_PCT_HIGHEST = 1000L

/** What a device is, for its icon. An unknown value is [OTHER]. */
@Serializable(with = DeviceKindSerializer::class)
enum class DeviceKind(val wire: String) {
    SPEAKER("speaker"),
    HEADPHONES("headphones"),
    HDMI("hdmi"),
    BLUETOOTH("bluetooth"),
    USB("usb"),
    MICROPHONE("microphone"),
    OTHER("other"),
}

internal object DeviceKindSerializer :
    FallbackEnumSerializer<DeviceKind>("com.swayconnect.protocol.DeviceKind", DeviceKind.entries, DeviceKind.OTHER, DeviceKind::wire)

internal object MaxVolumePct :
    StrictIntegerSerializer("com.swayconnect.protocol.MaxVolumePct", MAX_VOLUME_PCT_LOWEST..MAX_VOLUME_PCT_HIGHEST)

internal object DeviceVolumePct : StrictIntegerSerializer("com.swayconnect.protocol.DeviceVolumePct", 0..DEVICE_VOLUME_PCT_HIGHEST)

/** State of topic `audio`. */
@Serializable
data class AudioState(
    /** `false` when the sound server cannot be reached; both lists are empty then. */
    @Serializable(with = WireBool::class) val available: Boolean,
    /** The highest volume this server takes. */
    @SerialName("max_volume_pct") @Serializable(with = MaxVolumePct::class) val maxVolumePct: Long,
    @SerialName("default_sink") @Serializable(with = WireInt::class) val defaultSink: Long?,
    @SerialName("default_source") @Serializable(with = WireInt::class) val defaultSource: Long?,
    val sinks: List<Device>,
    /** Monitor sources are not listed. */
    val sources: List<Device>,
    @Serializable(with = WireBool::class) val truncated: Boolean,
)

@Serializable
data class Device(
    @Serializable(with = WireInt::class) val id: Long,
    val description: String,
    val kind: DeviceKind,
    /** Highest channel, 0 to [DEVICE_VOLUME_PCT_HIGHEST]: it may be above [AudioState.maxVolumePct]. */
    @SerialName("volume_pct") @Serializable(with = DeviceVolumePct::class) val volumePct: Long,
    @Serializable(with = WireBool::class) val muted: Boolean,
    /** `false` when its port is unplugged. It stays a valid target. */
    @Serializable(with = WireBool::class) val available: Boolean,
)

/**
 * Sets a device's volume. Uses the `continuous` rate class: a slider may send up to 10 per second.
 *
 * @param volumePct 0 to [AudioState.maxVolumePct]; the client refuses more than [MAX_VOLUME_PCT_HIGHEST], the
 *   server refuses more than its own maximum with `bad_request`.
 */
@Throws(IOException::class)
suspend fun Session.setVolume(deviceId: Long, volumePct: Int) {
    require(volumePct in 0..MAX_VOLUME_PCT_HIGHEST) { "volume_pct must be 0 to $MAX_VOLUME_PCT_HIGHEST" }
    request(Op.AUDIO_SET_VOLUME, deviceArgs(deviceId) { put("volume_pct", volumePct) })
}

/** Unmuting a source needs `audio.capture` and shows a notice on the desktop. */
@Throws(IOException::class)
suspend fun Session.setMuted(deviceId: Long, muted: Boolean) {
    request(Op.AUDIO_SET_MUTE, deviceArgs(deviceId) { put("muted", muted) })
}

/** Makes a sink the default output, or a source the default input (which needs `audio.capture`). */
@Throws(IOException::class)
suspend fun Session.setDefaultDevice(deviceId: Long) {
    request(Op.AUDIO_SET_DEFAULT, deviceArgs(deviceId))
}

private fun deviceArgs(deviceId: Long, more: JsonObjectBuilder.() -> Unit = {}): JsonObject {
    requireId(deviceId)
    return buildJsonObject {
        put("device_id", deviceId)
        more()
    }
}
