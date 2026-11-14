package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.KEEP_AWAKE_MIN_DURATION_S
import com.swayconnect.protocol.KeepAwake
import com.swayconnect.protocol.SessionState

/**
 * Topic `session` as the phone holds it (docs/PROTOCOL.md 17.8). `remaining_ms` is the time left when the server
 * built the state, so the reducer turns it into monotonic deadlines; [UiState.clockMs] counts towards them.
 */
data class SessionView(
    val state: SessionState,
    /** When keep-awake ends; null when it is off or the server sent no time. */
    val keepAwakeEndsAtMs: Long?,
    /** When `limit_reached` stops applying; null outside the cooldown. */
    val cooldownEndsAtMs: Long?,
    /**
     * False once the connection ended. The countdown goes on, since keep-awake outlives a disconnect (17.8 rule 6),
     * but nothing confirms it any more: the UI dims it as it dims a stale desktop.
     */
    val live: Boolean = true,
)

/** What the keep-awake tile and the status pill show (docs/UX.md 10.2), at [UiState.clockMs]. */
sealed interface KeepAwakeStatus {
    data object Off : KeepAwakeStatus

    /** [leftMs] is null when the server sent no time. A holder the server left out counts as another device. */
    data class On(val holder: Holder, val leftMs: Long?) : KeepAwakeStatus

    /** `limit_reached`: the tile is disabled until [leftMs] runs out. */
    data class Cooldown(val leftMs: Long) : KeepAwakeStatus
}

/** Applies a `session` state that arrived at the monotonic time [receivedAtMs]. */
fun UiState.withSession(next: SessionState, receivedAtMs: Long): UiState {
    val keepAwake = next.keepAwake
    val view =
        SessionView(
            state = next,
            keepAwakeEndsAtMs = keepAwake.remainingMs?.takeIf { keepAwake.active }?.let { receivedAtMs + it },
            cooldownEndsAtMs = keepAwake.cooldownRemainingMs?.let { receivedAtMs + it },
        )
    return copy(session = view, clockMs = receivedAtMs)
}

/** The connection ended: the last `session` state stays, marked as not live. */
fun UiState.withSessionStale(): UiState = copy(session = session?.copy(live = false))

/** The countdown clock moved to [nowMs] (monotonic). */
fun UiState.withClock(nowMs: Long): UiState = copy(clockMs = nowMs)

/**
 * Null before the first `session` state. A keep-awake whose deadline passed reads as off before the server's
 * next state confirms it, and so does a cooldown that ran out.
 */
val UiState.keepAwake: KeepAwakeStatus?
    get() {
        val view = session ?: return null
        val keepAwakeLeftMs = view.keepAwakeEndsAtMs?.let { leftMs(it) }
        val cooldownLeftMs = view.cooldownEndsAtMs?.let { leftMs(it) }
        return when {
            view.state.keepAwake.active && keepAwakeLeftMs != 0L ->
                KeepAwakeStatus.On(view.state.keepAwake.holder ?: Holder.OTHER_DEVICE, keepAwakeLeftMs)
            cooldownLeftMs != null && cooldownLeftMs > 0 -> KeepAwakeStatus.Cooldown(cooldownLeftMs)
            else -> KeepAwakeStatus.Off
        }
    }

/** Milliseconds until the countdown shown for [keepAwake] changes; null when nothing counts down. */
fun UiState.msUntilCountdownTick(): Long? =
    when (val status = keepAwake) {
        is KeepAwakeStatus.On -> status.leftMs?.let(Countdown::msUntilChange)
        is KeepAwakeStatus.Cooldown -> Countdown.msUntilChange(status.leftMs)
        KeepAwakeStatus.Off, null -> null
    }

private fun UiState.leftMs(deadlineMs: Long): Long = (deadlineMs - clockMs).coerceAtLeast(0)

private const val ONE_HOUR_S = 3_600L
private val DURATION_CHOICES_S = listOf(900L, 1_800L, ONE_HOUR_S, 7_200L)

/** The duration sheet's choices (docs/UX.md 10.2): 15 min, 30 min, 1 h and 2 h, only those up to `max_duration_s`. */
fun KeepAwake.durationChoicesS(): List<Long> = DURATION_CHOICES_S.filter { it in KEEP_AWAKE_MIN_DURATION_S..maxDurationS }

/** "1 h from now": `min(3600, max_duration_s)`. Null when the server's maximum is below the protocol's minimum. */
fun KeepAwake.oneHourFromNowS(): Long? = minOf(ONE_HOUR_S, maxDurationS).takeIf { it >= KEEP_AWAKE_MIN_DURATION_S }
