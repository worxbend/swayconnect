package com.swayconnect.android.ui.state

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** docs/UX.md 5.5 Drive mode, and the follow-focus guard. */
class DriveControllerTest {
    private val pages = listOf(1L, 2L, 3L, 4L, 5L)
    private val drive = DriveController()

    @Test
    fun `a user fling sends one request after the settle holds`() {
        drive.userDrag(shownId = 1)
        drive.settled(5, nowMs = 1_000)

        assertNull("not before 250ms", drive.due(nowMs = 1_249, shownId = 5, focusedId = 1, pageIds = pages))
        assertEquals(5L, drive.due(nowMs = 1_250, shownId = 5, focusedId = 1, pageIds = pages))
        assertNull("only once", drive.due(nowMs = 2_000, shownId = 5, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `a second settle of the same fling replaces the first and sends once`() {
        drive.userDrag(shownId = 1)
        drive.settled(3, nowMs = 1_000)
        drive.settled(4, nowMs = 1_100)

        assertNull(drive.due(nowMs = 1_250, shownId = 4, focusedId = 1, pageIds = pages))
        assertEquals(4L, drive.due(nowMs = 1_350, shownId = 4, focusedId = 1, pageIds = pages))
        assertNull(drive.due(nowMs = 1_400, shownId = 4, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `no send when the settled id equals the focused id`() {
        drive.userDrag(shownId = 1)
        drive.settled(2, nowMs = 0)

        assertNull(drive.due(nowMs = 300, shownId = 2, focusedId = 2, pageIds = pages))
    }

    @Test
    fun `a follow-focus scroll or a re-sync jump sends nothing`() {
        drive.settled(4, nowMs = 0)

        assertNull(drive.due(nowMs = 1_000, shownId = 4, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `the user flag clears after one settle`() {
        drive.userDrag(shownId = 1)
        drive.settled(3, nowMs = 0)
        assertEquals(3L, drive.due(nowMs = 300, shownId = 3, focusedId = 1, pageIds = pages))

        drive.settled(4, nowMs = 400)

        assertNull("a later programmatic settle", drive.due(nowMs = 1_000, shownId = 4, focusedId = 3, pageIds = pages))
    }

    @Test
    fun `a new drag drops the settle that still waits`() {
        drive.userDrag(shownId = 1)
        drive.settled(3, nowMs = 0)
        drive.userDrag(shownId = 1)

        assertNull(drive.due(nowMs = 300, shownId = 3, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `a workspace removed before the current page sends nothing`() {
        // No gesture: the pager jumps to keep the shown id when a page before it goes away.
        drive.settled(4, nowMs = 0)

        assertNull(drive.due(nowMs = 300, shownId = 4, focusedId = 1, pageIds = listOf(1L, 3L, 4L, 5L)))
    }

    @Test
    fun `a settled workspace that is gone before the hold ends sends nothing`() {
        drive.userDrag(shownId = 1)
        drive.settled(4, nowMs = 0)

        assertNull(drive.due(nowMs = 300, shownId = 5, focusedId = 1, pageIds = listOf(1L, 5L)))
    }

    @Test
    fun `a drag that springs back to the page it started on sends nothing`() {
        // Browse, then Drive on: page 3 is shown while the computer shows 1. A small nudge snaps back to 3.
        drive.userDrag(shownId = 3)
        drive.settled(3, nowMs = 0)

        assertNull(drive.due(nowMs = 300, shownId = 3, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `a tap on the card already shown still drives`() {
        drive.cardTap()
        drive.settled(3, nowMs = 0)

        assertEquals(3L, drive.due(nowMs = 300, shownId = 3, focusedId = 1, pageIds = pages))
    }

    @Test
    fun `the follow-focus guard`() {
        val now = 10_000L
        assertTrue(PagerFacts().allowsFollow(ownRequestPending = false, nowMs = now))
        assertFalse("finger down", PagerFacts(fingerDown = true).allowsFollow(false, now))
        assertFalse("own request waits", PagerFacts().allowsFollow(ownRequestPending = true, nowMs = now))
        assertFalse("touched 1.4 s ago", PagerFacts(lastTouchAtMs = now - 1_400).allowsFollow(false, now))
        assertTrue("touched 1.5 s ago", PagerFacts(lastTouchAtMs = now - 1_500).allowsFollow(false, now))
        assertFalse("TalkBack", PagerFacts(touchExploration = true).allowsFollow(false, now))
    }
}
