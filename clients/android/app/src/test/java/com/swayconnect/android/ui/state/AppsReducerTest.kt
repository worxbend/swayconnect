package com.swayconnect.android.ui.state

import com.swayconnect.protocol.App
import com.swayconnect.protocol.AppsState
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The apps reducer, risky apps and the pending launch (ghost tile) of docs/UX.md 8.2. */
class AppsReducerTest {
    private val base = UiState(pair = PairForm(deviceName = "Pixel"))

    @Test
    fun `entries of another id form and duplicate ids are dropped, recent resolves to apps`() {
        val apps = listOf(app("firefox.desktop"), app("bad id"), app(""), app("firefox.desktop"), app("foot"))
        val state = base.withApps(AppsState(apps, listOf("foot", "gone", "foot", "firefox.desktop"), false))
        assertEquals(listOf("firefox.desktop", "foot"), state.apps?.state?.apps?.map { it.entryId })
        assertEquals(listOf("foot", "firefox.desktop"), state.apps?.recentApps?.map { it.entryId })
    }

    @Test
    fun `a risky app is locked without launch risky`() {
        val without = Granted(ops = setOf(Op.APP_LAUNCH), scopes = listOf("launch"), readOnly = false)
        assertEquals(OpAvailability.Locked(SCOPE_LAUNCH_RISKY), without.launchAvailability(app("foot", risky = true)))
        assertEquals(OpAvailability.Available, without.launchAvailability(app("firefox")))
        val with = Granted(ops = setOf(Op.APP_LAUNCH), scopes = listOf("launch", SCOPE_LAUNCH_RISKY), readOnly = false)
        assertEquals(OpAvailability.Available, with.launchAvailability(app("foot", risky = true)))
        val noLaunch = Granted(ops = emptySet(), scopes = listOf("state.read"), readOnly = false)
        assertEquals(OpAvailability.Unsupported, noLaunch.launchAvailability(app("firefox")))
    }

    @Test
    fun `a started launch returns to Desktop on the target page and waits for a new window there`() {
        val shown = base.copy(area = Area.Launch, desktop = desktop(windows = listOf(window(10, workspace = 1))), selectedWorkspaceId = 1)
        val started = shown.withLaunchStarted(launch(LaunchTarget.Workspace(2), known = setOf(10)))
        assertEquals(Area.Desktop, started.area)
        assertEquals(2L, started.selectedWorkspaceId)
        assertEquals("firefox", started.pendingLaunch?.entryId)

        val elsewhere = started.copy(desktop = desktop(windows = listOf(window(10, 1), window(11, 1)))).withLaunchSeen()
        assertEquals("a window on another workspace", "firefox", elsewhere.pendingLaunch?.entryId)
        val moved = started.copy(desktop = desktop(windows = listOf(window(10, 2)))).withLaunchSeen()
        assertEquals("a known window moved there", "firefox", moved.pendingLaunch?.entryId)
        assertNull(started.copy(desktop = desktop(windows = listOf(window(10, 1), window(12, 2)))).withLaunchSeen().pendingLaunch)
    }

    @Test
    fun `a number target waits until sway created the workspace`() {
        val started = base.copy(desktop = desktop(windows = emptyList())).withLaunchStarted(launch(LaunchTarget.Number(7)))
        assertNull("no workspace 7 yet: the page stays", started.selectedWorkspaceId)
        assertEquals("firefox", started.withLaunchSeen().pendingLaunch?.entryId)
        val created = desktop(windows = listOf(window(20, 70)), extra = Workspace(70, 7, "7", "DP-1", false, false, false))
        assertNull(started.withDesktop(created, followFocus = false, seenAtMs = 0).pendingLaunch)
    }

    @Test
    fun `the timeout ends only its own launch, with one line`() {
        val first = launch(LaunchTarget.Workspace(1), startedAtMs = 1)
        val second = launch(LaunchTarget.Workspace(1), startedAtMs = 2)
        val state = base.copy(desktop = desktop(windows = emptyList())).withLaunchStarted(first).withLaunchStarted(second)
        assertEquals(state, state.withLaunchTimedOut(first, "late"))
        val timedOut = state.withLaunchTimedOut(second, "Firefox did not open a window yet.")
        assertNull(timedOut.pendingLaunch)
        assertEquals("Firefox did not open a window yet.", timedOut.notice)
    }

    @Test
    fun `stale drops the ghost tile and dims the list`() {
        val started = base.withApps(AppsState(emptyList(), emptyList(), false)).withLaunchStarted(launch(LaunchTarget.Workspace(1)))
        val stale = started.withAppsStale()
        assertNull(stale.pendingLaunch)
        assertEquals(false, stale.apps?.live)
    }

    private companion object {
        fun app(id: String, risky: Boolean = false) = App(id, id, null, emptyList(), null, terminal = risky, risky = risky)

        fun launch(target: LaunchTarget, known: Set<Long> = emptySet(), startedAtMs: Long = 0) =
            PendingLaunch("firefox", "Firefox", null, target, known, startedAtMs)

        fun window(id: Long, workspace: Long) =
            Window(id, workspace, "foot", null, focused = false, urgent = false, floating = false, fullscreen = false)

        fun desktop(windows: List<Window>, extra: Workspace? = null) =
            Desktop(
                SwayStatus(true, null),
                listOfNotNull(Workspace(1, 1, "1", "DP-1", true, true, false), Workspace(2, 2, "2", "DP-1", false, false, false), extra),
                windows,
                truncated = false,
            )
    }
}
