package com.swayconnect.android.ui.state

/** What a refused action shows (docs/UX.md 16): one plain line, or the Locked sheet for a missing scope. */
sealed interface Refusal {
    data class Line(val text: String) : Refusal

    data class Locked(val scope: String) : Refusal

    companion object {
        /**
         * The UX 16 row for [code]. [scope] and [retryAfterMs] come from the server's `detail` and are untrusted:
         * a scope that is not a plain scope name gives the line for an unknown refusal instead of a sheet.
         */
        fun of(code: String, scope: String?, retryAfterMs: Long?): Refusal =
            when (code) {
                "forbidden" -> scope?.takeIf { SCOPE_NAME.matches(it) }?.let(::Locked) ?: Line(UNKNOWN)
                "limit_reached" -> Line(availableAgain(retryAfterMs))
                else -> Line(LINES[code] ?: UNKNOWN)
            }

        /** UX 16 "timeout" row; also a request that got no answer at all. */
        const val NO_ANSWER = "No answer from the computer."

        private const val UNKNOWN = "The computer refused this."
        private const val MINUTE_MS = 60_000L
        private val SCOPE_NAME = Regex("[a-z][a-z0-9._]{0,63}")
        private val LINES = mapOf(
            "rate_limited" to "Too many actions. Wait a moment.",
            "read_only" to "The computer is set to look only.",
            "not_found" to "That window is gone.",
            "sway_unavailable" to "Sway is not running.",
            "timeout" to NO_ANSWER,
            "backend_unavailable" to "This needs a service that is not running on the computer.",
            "spawn_failed" to "The computer could not start it.",
            "launcher_closed" to "The launcher on the computer is closed.",
        )

        private fun availableAgain(retryAfterMs: Long?): String {
            if (retryAfterMs == null || retryAfterMs <= 0) return "Available again later."
            val minutes = (retryAfterMs + MINUTE_MS - 1) / MINUTE_MS
            return "Available again in $minutes min."
        }
    }
}
