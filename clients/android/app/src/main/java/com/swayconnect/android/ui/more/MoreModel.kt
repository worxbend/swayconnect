package com.swayconnect.android.ui.more

import com.swayconnect.android.ui.audio.AudioTap
import com.swayconnect.android.ui.audio.DeviceCard
import com.swayconnect.android.ui.audio.DevicesPart
import com.swayconnect.android.ui.audio.devicesPart
import com.swayconnect.android.ui.state.KeepAwakeStatus
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.ServerTarget
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.keepAwake
import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

// Pure rules of the More area (docs/UX.md 10). No Compose import, so the JVM tests cover them.

/** What the keep-awake tile says, before the access rules. */
sealed interface KeepAwakeFace {
    /** The server offers keep-awake but sent no `session` state yet. */
    data object Waiting : KeepAwakeFace

    data object Off : KeepAwakeFace

    /** [leftMs] is null when the server sent no time. */
    data class On(val fromThisPhone: Boolean, val leftMs: Long?) : KeepAwakeFace

    /** `limit_reached`: available again after [leftMs]. */
    data class Cooldown(val leftMs: Long) : KeepAwakeFace
}

/** The keep-awake tile of docs/UX.md 10.2. */
data class KeepAwakeTile(
    val face: KeepAwakeFace,
    /** [OpAvailability.Unsupported] never reaches a tile: [keepAwakeTile] gives none. */
    val access: OpAvailability,
    /** The computer reports its screen locked: the tile adds "Computer is locked". */
    val computerLocked: Boolean,
    /** `windows_inhibiting > 0`: the tile adds "A window also keeps it awake". */
    val windowKeepsAwake: Boolean,
    /** False while the state is not confirmed by a live connection: dimmed, no taps. */
    val live: Boolean,
    /** A `keep_awake.set` waits for its answer. */
    val pending: Boolean,
)

/** What a tap or a long press on the keep-awake tile does. */
sealed interface KeepAwakeAction {
    data object TurnOn : KeepAwakeAction

    data object TurnOff : KeepAwakeAction

    data object ChooseDuration : KeepAwakeAction

    /** Cooldown: `Reject` and the line "Available again in N min.", nothing sent. */
    data class Refuse(val leftMs: Long) : KeepAwakeAction

    data class ShowLocked(val scope: String) : KeepAwakeAction

    data object None : KeepAwakeAction
}

/** The tile, or null when the server does not offer `keep_awake.set` or nothing is known yet (before `welcome`). */
fun keepAwakeTile(state: UiState): KeepAwakeTile? {
    val access = state.granted?.availabilityOf(Op.KEEP_AWAKE_SET) ?: return null
    if (access == OpAvailability.Unsupported) return null
    val session = state.session
    return KeepAwakeTile(
        face = faceOf(state.keepAwake),
        access = access,
        computerLocked = session?.state?.locked == true,
        windowKeepsAwake = (session?.state?.windowsInhibiting ?: 0) > 0,
        live = state.desktopLive && session?.live != false,
        pending = state.pendingTargetId == ServerTarget.KEEP_AWAKE,
    )
}

private fun faceOf(status: KeepAwakeStatus?): KeepAwakeFace =
    when (status) {
        null -> KeepAwakeFace.Waiting
        KeepAwakeStatus.Off -> KeepAwakeFace.Off
        is KeepAwakeStatus.On -> KeepAwakeFace.On(status.holder == Holder.THIS_DEVICE, status.leftMs)
        is KeepAwakeStatus.Cooldown -> KeepAwakeFace.Cooldown(status.leftMs)
    }

/** The Tap column of the UX 10.2 table. Any device may turn off a keep-awake. */
fun KeepAwakeTile.tap(): KeepAwakeAction =
    gated { face ->
        when (face) {
            KeepAwakeFace.Waiting -> KeepAwakeAction.None
            KeepAwakeFace.Off -> KeepAwakeAction.TurnOn
            is KeepAwakeFace.On -> KeepAwakeAction.TurnOff
            is KeepAwakeFace.Cooldown -> KeepAwakeAction.Refuse(face.leftMs)
        }
    }

/** The Long press column of the UX 10.2 table: the duration sheet, except in the cooldown. */
fun KeepAwakeTile.longPress(): KeepAwakeAction =
    gated { face ->
        when (face) {
            KeepAwakeFace.Waiting, is KeepAwakeFace.Cooldown -> KeepAwakeAction.None
            KeepAwakeFace.Off, is KeepAwakeFace.On -> KeepAwakeAction.ChooseDuration
        }
    }

/** A missing scope opens the Locked sheet; read-only, a stale state or a waiting answer take no tap. */
private inline fun KeepAwakeTile.gated(byFace: (KeepAwakeFace) -> KeepAwakeAction): KeepAwakeAction =
    when {
        access is OpAvailability.Locked -> KeepAwakeAction.ShowLocked(access.scope)
        access != OpAvailability.Available || !live || pending -> KeepAwakeAction.None
        else -> byFace(face)
    }

/**
 * The Mic tile of docs/UX.md 10.1 and 10.3: the default input device of topic `audio`. It follows the mute rules
 * of the Audio area (7.2): muting needs `audio`, unmuting needs `audio.capture` and a 600 ms hold.
 */
sealed interface MicTile {
    data class Locked(val scope: String) : MicTile

    /** The server offers audio but sent no `audio` state yet. */
    data object Waiting : MicTile

    data object Unreachable : MicTile

    /** No input device, or none is the default. */
    data object NoMic : MicTile

    data class Shown(val card: DeviceCard) : MicTile
}

/** The tile, or null before `welcome` or when the server does not offer `audio.set_mute`. */
fun micTile(state: UiState): MicTile? {
    val granted = state.granted ?: return null
    if (granted.availabilityOf(Op.AUDIO_SET_MUTE) == OpAvailability.Unsupported) return null
    return when (val part = devicesPart(state)) {
        DevicesPart.Hidden -> null
        is DevicesPart.Locked -> MicTile.Locked(part.scope)
        DevicesPart.Waiting -> MicTile.Waiting
        DevicesPart.Unreachable -> MicTile.Unreachable
        DevicesPart.Empty -> MicTile.NoMic
        is DevicesPart.Shown -> part.inputs.firstOrNull { it.isDefault }?.let(MicTile::Shown) ?: MicTile.NoMic
    }
}

/** A tap on the tile: the mute button of the default microphone (UX 7.2). */
fun MicTile.tap(): AudioTap =
    when (this) {
        is MicTile.Locked -> AudioTap.ShowLocked(scope)
        is MicTile.Shown -> card.muteTap()
        MicTile.Waiting, MicTile.Unreachable, MicTile.NoMic -> AudioTap.None
    }

/** The Launcher tile of docs/UX.md 10.3: `launcher.open` on the computer. */
data class LauncherTile(
    /** [OpAvailability.Unsupported] never reaches a tile: [launcherTile] gives none. */
    val access: OpAvailability,
    val live: Boolean,
    /** A `launcher.open` waits for its answer. */
    val pending: Boolean,
)

/** What a tap on the Launcher tile does. After `Open` the view model picks Keys or the line (UX 8.2). */
sealed interface LauncherTap {
    data object Open : LauncherTap

    data class ShowLocked(val scope: String) : LauncherTap

    data object None : LauncherTap
}

/** The tile, or null before `welcome` or when the server does not offer `launcher.open`. */
fun launcherTile(state: UiState): LauncherTile? {
    val access = state.granted?.availabilityOf(Op.LAUNCHER_OPEN) ?: return null
    if (access == OpAvailability.Unsupported) return null
    return LauncherTile(access, live = state.desktopLive, pending = state.pendingControl == PendingControl.Launcher)
}

fun LauncherTile.tap(): LauncherTap =
    when {
        access is OpAvailability.Locked -> LauncherTap.ShowLocked(access.scope)
        access != OpAvailability.Available || !live || pending -> LauncherTap.None
        else -> LauncherTap.Open
    }

/** A length as words name it: whole hours when it is a multiple of one, else minutes rounded up. */
sealed interface DurationWords {
    data class Hours(val hours: Long) : DurationWords

    data class Minutes(val minutes: Long) : DurationWords

    companion object {
        private const val HOUR_S = 3_600L
        private const val MINUTE_S = 60L

        fun of(seconds: Long): DurationWords {
            val positive = seconds.coerceAtLeast(0)
            return if (positive >= HOUR_S && positive % HOUR_S == 0L) {
                Hours(positive / HOUR_S)
            } else {
                Minutes((positive + MINUTE_S - 1) / MINUTE_S)
            }
        }
    }
}

private const val MINUTE_MS = 60_000L

/** Whole minutes, rounded up, for "Available again in N min." (the line of UX 16 `limit_reached`). */
fun minutesUp(leftMs: Long): Int = ((leftMs.coerceAtLeast(0) + MINUTE_MS - 1) / MINUTE_MS).toInt()
