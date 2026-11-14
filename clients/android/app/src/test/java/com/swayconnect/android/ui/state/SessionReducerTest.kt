package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.KeepAwake
import com.swayconnect.protocol.SessionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The session reducer of docs/PROTOCOL.md 17.8 and the keep-awake tile states of docs/UX.md 10.2. */
class SessionReducerTest {
    private val base = UiState(pair = PairForm(deviceName = "Pixel"))

    @Test
    fun `no session state shows no tile state`() {
        assertNull(base.keepAwake)
        assertNull(base.msUntilCountdownTick())
    }

    @Test
    fun `off`() {
        val state = base.withSession(session(keepAwake()), receivedAtMs = 1_000)
        assertEquals(KeepAwakeStatus.Off, state.keepAwake)
        assertNull(state.msUntilCountdownTick())
    }

    @Test
    fun `on from this phone counts down from remaining_ms`() {
        val state = base.withSession(session(keepAwake(active = true, remainingMs = 52 * MINUTE_MS, holder = Holder.THIS_DEVICE)), 1_000)
        assertEquals(KeepAwakeStatus.On(Holder.THIS_DEVICE, 52 * MINUTE_MS), state.keepAwake)
        assertEquals(MINUTE_MS, state.msUntilCountdownTick())

        val later = state.withClock(1_000 + 51 * MINUTE_MS + 30_000)
        assertEquals(KeepAwakeStatus.On(Holder.THIS_DEVICE, 30_000), later.keepAwake)
        assertEquals(1_000L, later.msUntilCountdownTick())
    }

    @Test
    fun `on from another device, and without a holder`() {
        val other = base.withSession(session(keepAwake(active = true, remainingMs = MINUTE_MS, holder = Holder.OTHER_DEVICE)), 0)
        assertEquals(KeepAwakeStatus.On(Holder.OTHER_DEVICE, MINUTE_MS), other.keepAwake)
        val unnamed = base.withSession(session(keepAwake(active = true, remainingMs = MINUTE_MS, holder = null)), 0)
        assertEquals(KeepAwakeStatus.On(Holder.OTHER_DEVICE, MINUTE_MS), unnamed.keepAwake)
    }

    @Test
    fun `on without a time shows no countdown`() {
        val state = base.withSession(session(keepAwake(active = true, remainingMs = null, holder = Holder.THIS_DEVICE)), 0)
        assertEquals(KeepAwakeStatus.On(Holder.THIS_DEVICE, null), state.keepAwake)
        assertNull(state.msUntilCountdownTick())
    }

    @Test
    fun `a passed deadline reads as off before the server confirms it`() {
        val state = base.withSession(session(keepAwake(active = true, remainingMs = 5_000, holder = Holder.THIS_DEVICE)), 0)
        assertEquals(KeepAwakeStatus.Off, state.withClock(5_000).keepAwake)
        assertEquals(KeepAwakeStatus.Off, state.withClock(9_000).keepAwake)
    }

    @Test
    fun `cooldown counts down and then turns off`() {
        val state = base.withSession(session(keepAwake(cooldownRemainingMs = 9 * MINUTE_MS)), 0)
        assertEquals(KeepAwakeStatus.Cooldown(9 * MINUTE_MS), state.keepAwake)
        assertEquals(Countdown.Minutes(9), Countdown.of(9 * MINUTE_MS))
        assertEquals(KeepAwakeStatus.Off, state.withClock(9 * MINUTE_MS).keepAwake)
    }

    @Test
    fun `locked and windows inhibiting pass through`() {
        val state = base.withSession(session(keepAwake(), locked = true, windowsInhibiting = 2), 0)
        assertEquals(true, state.session?.state?.locked)
        assertEquals(2L, state.session?.state?.windowsInhibiting)
        val unknown = base.withSession(session(keepAwake(), locked = null), 0)
        assertNull(unknown.session?.state?.locked)
    }

    @Test
    fun `a newer state replaces the deadlines`() {
        val first = base.withSession(session(keepAwake(active = true, remainingMs = MINUTE_MS, holder = Holder.THIS_DEVICE)), 0)
        val second = first.withSession(session(keepAwake()), 10_000)
        assertEquals(KeepAwakeStatus.Off, second.keepAwake)
        assertNull(second.session?.keepAwakeEndsAtMs)
        assertEquals(10_000L, second.clockMs)
    }

    @Test
    fun `a closed connection keeps the state as not live`() {
        val state = base.withSession(session(keepAwake(active = true, remainingMs = MINUTE_MS, holder = Holder.THIS_DEVICE)), 0)
        val stale = state.withSessionStale()
        assertEquals(false, stale.session?.live)
        assertEquals(state.keepAwake, stale.keepAwake)
        assertNull(base.withSessionStale().session)
    }

    @Test
    fun `duration choices stop at max_duration_s`() {
        assertEquals(listOf(900L, 1_800L, 3_600L, 7_200L), keepAwake(maxDurationS = 7_200).durationChoicesS())
        assertEquals(listOf(900L, 1_800L, 3_600L), keepAwake(maxDurationS = 5_000).durationChoicesS())
        assertEquals(emptyList<Long>(), keepAwake(maxDurationS = 600).durationChoicesS())
    }

    @Test
    fun `one hour from now is capped by max_duration_s`() {
        assertEquals(3_600L, keepAwake(maxDurationS = 7_200).oneHourFromNowS())
        assertEquals(1_800L, keepAwake(maxDurationS = 1_800).oneHourFromNowS())
        assertEquals(60L, keepAwake(maxDurationS = 60).oneHourFromNowS())
        assertNull(keepAwake(maxDurationS = 59).oneHourFromNowS())
    }

    private fun keepAwake(
        active: Boolean = false,
        remainingMs: Long? = null,
        holder: Holder? = null,
        maxDurationS: Long = 7_200,
        cooldownRemainingMs: Long? = null,
    ) = KeepAwake(active, remainingMs, holder, 3_600, maxDurationS, 14_400, cooldownRemainingMs)

    private fun session(keepAwake: KeepAwake, locked: Boolean? = false, windowsInhibiting: Long = 0) =
        SessionState(keepAwake, windowsInhibiting, locked)

    private companion object {
        const val MINUTE_MS = 60_000L
    }
}
