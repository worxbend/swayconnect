package com.swayconnect.android.ui.state

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** docs/UX.md 10.2: minutes, seconds in the last minute, and a description that changes at most every 5 minutes. */
class CountdownTest {
    @Test
    fun `shows whole minutes rounded up and seconds in the last minute`() {
        val cases =
            mapOf(
                -5L to Countdown.Seconds(0),
                0L to Countdown.Seconds(0),
                1L to Countdown.Seconds(1),
                1_000L to Countdown.Seconds(1),
                1_001L to Countdown.Seconds(2),
                59_000L to Countdown.Seconds(59),
                59_001L to Countdown.Minutes(1),
                60_000L to Countdown.Minutes(1),
                60_001L to Countdown.Minutes(2),
                52L * MINUTE_MS to Countdown.Minutes(52),
            )
        cases.forEach { (leftMs, shown) -> assertEquals("left $leftMs ms", shown, Countdown.of(leftMs)) }
    }

    @Test
    fun `the next change is a minute away above one minute and a second away below`() {
        assertNull(Countdown.msUntilChange(0))
        assertNull(Countdown.msUntilChange(-1))
        assertEquals(1L, Countdown.msUntilChange(1))
        assertEquals(500L, Countdown.msUntilChange(1_500))
        assertEquals(1_000L, Countdown.msUntilChange(60_000))
        assertEquals(1_000L, Countdown.msUntilChange(61_000))
        assertEquals(MINUTE_MS, Countdown.msUntilChange(52L * MINUTE_MS))
        assertEquals(1L, Countdown.msUntilChange(52L * MINUTE_MS + 1))
    }

    @Test
    fun `ticking at each change walks every shown value once`() {
        var leftMs = 2L * HOUR_MS + 30_000
        var shown = Countdown.of(leftMs)
        var ticks = 0
        while (true) {
            val waitMs = Countdown.msUntilChange(leftMs) ?: break
            assertTrue(waitMs > 0)
            leftMs -= waitMs
            val next = Countdown.of(leftMs)
            assertTrue("$shown then $next", next != shown)
            shown = next
            ticks++
        }
        assertEquals(Countdown.Seconds(0), shown)
        // 121 minute values down to 1 min, then 59 seconds values and 0.
        assertEquals(120 + 59 + 1, ticks)
    }

    @Test
    fun `the announced minutes step by five and never say zero`() {
        val cases = mapOf(0L to 5L, 30_000L to 5L, 7 * MINUTE_MS to 5L, 8 * MINUTE_MS to 10L, 52 * MINUTE_MS to 50L, 53 * MINUTE_MS to 55L)
        cases.forEach { (leftMs, minutes) -> assertEquals("left $leftMs ms", minutes, Countdown.announcedMinutes(leftMs)) }
    }

    @Test
    fun `the announced minutes change at most every five minutes`() {
        var lastChangeMs: Long? = null
        var announced = Countdown.announcedMinutes(2 * HOUR_MS)
        for (leftMs in 2 * HOUR_MS downTo 0 step 1_000) {
            val now = Countdown.announcedMinutes(leftMs)
            if (now == announced) continue
            lastChangeMs?.let { assertTrue("changed again after ${it - leftMs} ms", it - leftMs >= 5 * MINUTE_MS) }
            lastChangeMs = leftMs
            announced = now
        }
    }

    private companion object {
        const val MINUTE_MS = 60_000L
        const val HOUR_MS = 3_600_000L
    }
}
