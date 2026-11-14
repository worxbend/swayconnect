package com.swayconnect.android.ui.state

/**
 * A countdown as docs/UX.md 10.2 shows it: whole minutes from one minute up, seconds below, both rounded up,
 * so "1 min" stays until 59 s are left and "1 s" until the end.
 */
sealed interface Countdown {
    data class Minutes(val minutes: Long) : Countdown

    data class Seconds(val seconds: Long) : Countdown

    companion object {
        private const val SECOND_MS = 1_000L
        private const val MINUTE_MS = 60_000L
        private const val SECONDS_PER_MINUTE = 60L
        private const val LAST_MINUTE_MS = 59_000L
        private const val ANNOUNCE_STEP_MINUTES = 5L

        /** What the tile shows with [leftMs] left. Zero or less shows `Seconds(0)`. */
        fun of(leftMs: Long): Countdown {
            val seconds = ceilDiv(leftMs.coerceAtLeast(0), SECOND_MS)
            return if (seconds < SECONDS_PER_MINUTE) Seconds(seconds) else Minutes(ceilDiv(leftMs, MINUTE_MS))
        }

        /**
         * Milliseconds until [of] gives another value: per minute, per second in the last minute. Null at zero or
         * less: nothing is left to count.
         */
        fun msUntilChange(leftMs: Long): Long? {
            if (leftMs <= 0) return null
            val nextValueAtMs =
                when (val shown = of(leftMs)) {
                    is Seconds -> (shown.seconds - 1) * SECOND_MS
                    is Minutes -> maxOf((shown.minutes - 1) * MINUTE_MS, LAST_MINUTE_MS)
                }
            return leftMs - nextValueAtMs
        }

        /**
         * The minutes a `contentDescription` names ("about 50 minutes left"): the shown minutes rounded to a step of 5,
         * at least 5, so the description changes at most every 5 minutes and is not a per-minute announcement.
         */
        fun announcedMinutes(leftMs: Long): Long {
            val minutes = ceilDiv(leftMs.coerceAtLeast(0), MINUTE_MS)
            val rounded = (minutes + ANNOUNCE_STEP_MINUTES / 2) / ANNOUNCE_STEP_MINUTES * ANNOUNCE_STEP_MINUTES
            return maxOf(ANNOUNCE_STEP_MINUTES, rounded)
        }

        private fun ceilDiv(dividend: Long, divisor: Long): Long = (dividend + divisor - 1) / divisor
    }
}
