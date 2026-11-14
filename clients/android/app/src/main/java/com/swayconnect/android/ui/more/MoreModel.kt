package com.swayconnect.android.ui.more

import com.swayconnect.android.ui.state.KeepAwakeStatus
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
