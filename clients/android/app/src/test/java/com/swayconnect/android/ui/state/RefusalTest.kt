package com.swayconnect.android.ui.state

import org.junit.Assert.assertEquals
import org.junit.Test

/** docs/UX.md 16: every refusal reads as one plain line, never as a wire code. */
class RefusalTest {
    private fun line(code: String, scope: String? = null, retryAfterMs: Long? = null) =
        (Refusal.of(code, scope, retryAfterMs) as Refusal.Line).text

    @Test
    fun `each known code has its own sentence`() {
        assertEquals("Too many actions. Wait a moment.", line("rate_limited"))
        assertEquals("The computer is set to look only.", line("read_only"))
        assertEquals("That window is gone.", line("not_found"))
        assertEquals("Sway is not running.", line("sway_unavailable"))
        assertEquals("No answer from the computer.", line("timeout"))
        assertEquals("This needs a service that is not running on the computer.", line("backend_unavailable"))
        assertEquals("The computer could not start it.", line("spawn_failed"))
        assertEquals("The launcher on the computer is closed.", line("launcher_closed"))
    }

    @Test
    fun `an unknown code never shows the code`() {
        assertEquals("The computer refused this.", line("sway_error"))
        assertEquals("The computer refused this.", line("‮evil"))
    }

    @Test
    fun `forbidden opens the Locked sheet for a plain scope name only`() {
        assertEquals(Refusal.Locked("control"), Refusal.of("forbidden", "control", null))
        assertEquals("The computer refused this.", line("forbidden", scope = "control; rm -rf"))
        assertEquals("The computer refused this.", line("forbidden", scope = null))
    }

    @Test
    fun `limit reached names the minutes it waits, rounded up`() {
        assertEquals("Available again in 9 min.", line("limit_reached", retryAfterMs = 8 * 60_000L + 1))
        assertEquals("Available again later.", line("limit_reached"))
    }
}
