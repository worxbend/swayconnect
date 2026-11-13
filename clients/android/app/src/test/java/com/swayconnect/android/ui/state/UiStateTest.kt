package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class UiStateTest {
    private val initial = UiState(pair = PairForm(deviceName = "Pixel 9"))

    private fun workspace(id: Long, focused: Boolean) =
        Workspace(id = id, number = id, name = id.toString(), output = null, focused = focused, visible = focused, urgent = false)

    private fun desktop(online: Boolean, vararg workspaces: Workspace) =
        Desktop(SwayStatus(online, version = null), workspaces.toList(), windows = emptyList(), truncated = false)

    private fun UiState.withDesktop(next: Desktop, followFocus: Boolean = true) = withDesktop(next, followFocus, seenAtMs = 0)

    @Test
    fun `the first state selects the focused workspace`() {
        val state = initial.withDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)))

        assertEquals(9L, state.selectedWorkspaceId)
    }

    @Test
    fun `a selection stays while the focus does not move`() {
        val focusedOnNine = desktop(true, workspace(4, focused = false), workspace(9, focused = true))
        val selectedFour = initial.withDesktop(focusedOnNine).copy(selectedWorkspaceId = 4L)

        assertEquals(4L, selectedFour.withDesktop(focusedOnNine).selectedWorkspaceId)
    }

    @Test
    fun `the selection follows a focus change and a new state ends the spinner`() {
        val before = initial.withDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)))
        val waiting = before.copy(pendingTargetId = 4L)

        val after = waiting.withDesktop(desktop(true, workspace(4, focused = true), workspace(9, focused = false)))

        assertEquals(4L, after.selectedWorkspaceId)
        assertNull(after.pendingTargetId)
    }

    @Test
    fun `a selected workspace that is gone falls back to the focused one`() {
        val selectedFour = initial.withDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)))
            .copy(selectedWorkspaceId = 4L)

        assertEquals(9L, selectedFour.withDesktop(desktop(true, workspace(9, focused = true))).selectedWorkspaceId)
    }

    /** UX 5.5: the page index is found again by id when sway drops a workspace. */
    @Test
    fun `a dropped workspace before the shown page keeps the shown workspace and moves its index`() {
        val shownSeven = initial
            .withDesktop(desktop(true, workspace(2, focused = true), workspace(5, focused = false), workspace(7, focused = false)))
            .copy(selectedWorkspaceId = 7L)
        assertEquals(2, shownSeven.shownPageIndex)

        val after = shownSeven.withDesktop(desktop(true, workspace(5, focused = true), workspace(7, focused = false)), followFocus = false)

        assertEquals(7L, after.selectedWorkspaceId)
        assertEquals(1, after.shownPageIndex)
    }

    @Test
    fun `a focus move the guard holds back shows the chip and keeps the page`() {
        val shownFour = initial.withDesktop(desktop(true, workspace(4, focused = true), workspace(9, focused = false)))

        val moved = shownFour.withDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)), followFocus = false)

        assertEquals(4L, moved.selectedWorkspaceId)
        assertEquals(9L, moved.followFocusChip)
        val followed = moved.followChip()
        assertEquals(9L, followed.selectedWorkspaceId)
        assertNull(followed.followFocusChip)
    }

    @Test
    fun `the chip goes when the focus comes to the shown page`() {
        val chip = initial.withDesktop(desktop(true, workspace(4, focused = true), workspace(9, focused = false)))
            .withDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)), followFocus = false)

        val back = chip.withDesktop(desktop(true, workspace(4, focused = true), workspace(9, focused = false)), followFocus = false)

        assertNull(back.followFocusChip)
        assertEquals(4L, back.selectedWorkspaceId)
    }

    @Test
    fun `a sheet that names a window that is gone closes`() {
        val window = Window(id = 7, workspaceId = 4, appId = null, title = null, focused = false, urgent = false, floating = false, fullscreen = false)
        val open = initial.withDesktop(desktop(true, workspace(4, focused = true)).copy(windows = listOf(window)))
            .copy(sheet = Sheet.WindowActions(7))

        assertEquals(Sheet.WindowActions(7), open.withDesktop(desktop(true, workspace(4, focused = true)).copy(windows = listOf(window))).sheet)
        assertNull(open.withDesktop(desktop(true, workspace(4, focused = true))).sheet)
    }

    @Test
    fun `a cached snapshot is stale and the end of a connection makes the live state stale`() {
        val cached = initial.withCachedDesktop(desktop(true, workspace(4, focused = false), workspace(9, focused = true)), savedAtMs = 42)
        assertEquals(false, cached.desktopLive)
        assertEquals(9L, cached.selectedWorkspaceId)
        assertEquals(42L, cached.lastSeenAtMs)

        val live = cached.withDesktop(desktop(true, workspace(9, focused = true)))
        assertEquals(true, live.desktopLive)

        val ended = live.copy(pendingTargetId = 9L).withDesktopStale()
        assertEquals(false, ended.desktopLive)
        assertNotNull("the last state stays on screen", ended.desktop)
        assertNull(ended.pendingTargetId)
    }

    @Test
    fun `the banner names sway offline only on a ready connection`() {
        val ready = initial.copy(connection = ConnectionStatus.Ready)

        assertEquals("Ready", ready.withDesktop(desktop(true)).bannerText())
        assertEquals("Sway offline", ready.withDesktop(desktop(false)).bannerText())
        assertEquals("Reconnecting", initial.copy(connection = ConnectionStatus.Backoff).bannerText())
    }

    /** Compose throws on a list key that comes twice, and the list is keyed by window id. */
    @Test
    fun `a window id that comes twice is shown once`() {
        val window = Window(id = 7, workspaceId = 4, appId = null, title = "a", focused = false, urgent = false, floating = false, fullscreen = false)
        val twice = desktop(true, workspace(4, focused = true)).copy(windows = listOf(window, window.copy(title = "b")))

        assertEquals(listOf(window), initial.withDesktop(twice).desktop?.windows)
    }

    @Test
    fun `the control hint says why a connection offers no action`() {
        val reading = setOf(Op.PING, Op.STATE_SUBSCRIBE)
        fun granted(ops: Set<String>, readOnly: Boolean) = initial.copy(granted = Granted(ops, scopes = emptyList(), readOnly))

        assertNull("before welcome", initial.controlHint())
        assertNull("an action is offered", granted(reading + Op.WINDOW_FOCUS, readOnly = false).controlHint())
        assertEquals("Read-only: this phone can look, not control.", granted(reading, readOnly = true).controlHint())
        assertEquals("This device may not control the computer.", granted(reading, readOnly = false).controlHint())
    }

    @Test
    fun `text that is not a pairing link gives an error and no target`() {
        val form = initial.pair.withLink("https://example.org/")

        assertNull(form.target)
        assertNotNull(form.error)
    }

    /** The parser names URI parameters ("fp is not base64url of 32 bytes"). That is no text for a user. */
    @Test
    fun `a link with a bad parameter gives the same plain error as any other text`() {
        val form = initial.pair.withLink("swayconnect://pair?v=1&host=192.0.2.1&port=4000&fp=short&secret=short")

        assertEquals(NOT_A_PAIRING_LINK, form.error)
    }

    @Test
    fun `the short fingerprint is the first eight bytes in groups`() {
        val fingerprint = ByteArray(32) { (0xAA + it).toByte() }

        assertEquals("aaab acad aeaf b0b1", shortFingerprint(fingerprint))
    }
}
