package com.swayconnect.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Topic `session` and the results of `keep_awake.set` and `session.lock` (docs/PROTOCOL.md 17.6 and 17.8).

/** Which device holds a keep-awake, a remote keyboard or a launcher. An unknown value is [OTHER_DEVICE]. */
@Serializable(with = HolderSerializer::class)
enum class Holder(val wire: String) {
    THIS_DEVICE("this_device"),
    OTHER_DEVICE("other_device"),
}

internal object HolderSerializer :
    FallbackEnumSerializer<Holder>("com.swayconnect.protocol.Holder", Holder.entries, Holder.OTHER_DEVICE, Holder::wire)

/** State of topic `session`. The server builds it per device, so [Holder] is seen from this device. */
@Serializable
data class SessionState(
    @SerialName("keep_awake") val keepAwake: KeepAwake,
    /** Windows that keep the desktop awake by themselves, for example a fullscreen video. */
    @SerialName("windows_inhibiting") @Serializable(with = WireInt::class) val windowsInhibiting: Long,
    /** `null` when the server cannot tell. */
    @Serializable(with = WireBool::class) val locked: Boolean?,
    /** Absent when the server offers no `input.*` operation. */
    val input: RemoteInput? = null,
    /** Absent when the server offers no `launcher.open`. */
    val launcher: Launcher? = null,
)

/** Decodes a `session` state. `input` and `launcher` are optional but never `null`. */
object SessionStateSerializer : AbsentOrNotNullSerializer<SessionState>(SessionState.serializer(), setOf("input", "launcher"))

@Serializable
data class KeepAwake(
    @Serializable(with = WireBool::class) val active: Boolean,
    /** Time left when the state was built; the client counts down from here. `null` when off. */
    @SerialName("remaining_ms") @Serializable(with = WireInt::class) val remainingMs: Long?,
    /** `null` when off. */
    val holder: Holder?,
    @SerialName("default_duration_s") @Serializable(with = WireInt::class) val defaultDurationS: Long,
    @SerialName("max_duration_s") @Serializable(with = WireInt::class) val maxDurationS: Long,
    /** The continuous cap: past it the server refuses `true` with `limit_reached` until the cooldown ends. */
    @SerialName("max_continuous_s") @Serializable(with = WireInt::class) val maxContinuousS: Long,
    /** Set while `limit_reached` applies. */
    @SerialName("cooldown_remaining_ms") @Serializable(with = WireInt::class) val cooldownRemainingMs: Long?,
)

/** A remote keyboard session armed on the server. */
@Serializable
data class RemoteInput(@Serializable(with = WireBool::class) val active: Boolean, val holder: Holder?)

/** A launcher the server started. */
@Serializable
data class Launcher(@Serializable(with = WireBool::class) val open: Boolean, val holder: Holder?)

@Serializable
internal class KeepAwakeResult(@SerialName("remaining_ms") @Serializable(with = WireInt::class) val remainingMs: Long?)

@Serializable
internal class LockResult(@SerialName("already_locked") @Serializable(with = WireBool::class) val alreadyLocked: Boolean)
