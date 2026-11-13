package com.swayconnect.android.ui

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
