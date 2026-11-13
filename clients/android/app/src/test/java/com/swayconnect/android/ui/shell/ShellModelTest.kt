package com.swayconnect.android.ui.shell

import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.Output
import com.swayconnect.protocol.Rect
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Transform
import com.swayconnect.protocol.Welcome
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ShellModelTest {
    private val initial = UiState(pair = PairForm(deviceName = "Pixel 9"))

    private fun output(active: Boolean, power: Boolean) =
        Output("DP-1", null, Rect(0, 0, 1920, 1080), 1000, Transform.NORMAL, active = active, power = power, currentWorkspaceId = null)

    private fun desktop(online: Boolean = true, outputs: List<Output>? = null, urgent: Boolean = false) =
        Desktop(
            SwayStatus(online, version = null),
            listOf(Workspace(1, 1, "1", null, focused = true, visible = true, urgent = urgent)),
            windows = emptyList(),
            truncated = false,
            outputs = outputs,
        )

    private fun granted(scopes: List<String>, ops: List<String>, readOnly: Boolean = false) =
        Granted.of(
            Welcome(
                protocol = 1,
                deviceId = "device",
                serverProof = "proof",
                scopes = scopes,
                ops = ops,
                topics = listOf("desktop"),
                readOnly = readOnly,
                server = ServerInfo("desk", "0.2.0"),
                limits = FrameLimits(16384, 1048576, 90000),
                knownScopes = listOf("state.read", "control", "audio", "launch", "input.text"),
                knownOps = listOf(Op.PING, Op.STATE_SUBSCRIBE, Op.WORKSPACE_FOCUS, "audio.set_volume", "app.launch", "input.text"),
            ),
        )

    private fun ready(desktop: Desktop? = desktop(), granted: Granted? = null) =
        initial.copy(connection = ConnectionStatus.Ready, desktop = desktop, desktopLive = true, granted = granted)

    @Test
    fun `each connection state has its own status word`() {
        assertEquals(StatusKind.Connecting, statusKind(initial.copy(connection = ConnectionStatus.Connecting)))
        assertEquals(StatusKind.CheckingKey, statusKind(initial.copy(connection = ConnectionStatus.Authenticating)))
        assertEquals(StatusKind.Offline, statusKind(initial.copy(connection = ConnectionStatus.Backoff)))
        assertEquals(StatusKind.Offline, statusKind(initial.copy(connection = ConnectionStatus.Disconnected)))
        assertEquals(StatusKind.Ready, statusKind(ready()))
    }

    @Test
    fun `a refused connection and a revoked phone are problems, not offline`() {
        val refused = initial.copy(connection = ConnectionStatus.Disconnected, connectionProblem = "wrong key")

        assertEquals(StatusKind.Refused, statusKind(refused))
        assertEquals(StatusKind.NotPaired, statusKind(ready().copy(unpaired = true)))
    }

    @Test
    fun `sway offline outranks screens off and read-only`() {
        val state = ready(desktop(online = false, outputs = listOf(output(active = true, power = false))), granted(listOf("state.read"), listOf(Op.PING), readOnly = true))

        assertEquals(StatusKind.SwayOffline, statusKind(state))
    }

    @Test
    fun `screens off needs every active output powered off`() {
        assertEquals(StatusKind.ScreensOff, statusKind(ready(desktop(outputs = listOf(output(true, false), output(false, true))))))
        assertEquals(StatusKind.Ready, statusKind(ready(desktop(outputs = listOf(output(true, false), output(true, true))))))
        assertEquals(StatusKind.Ready, statusKind(ready(desktop(outputs = emptyList()))))
        assertEquals(StatusKind.Ready, statusKind(ready(desktop(outputs = null))))
    }

    @Test
    fun `a read-only server shows read-only when ready`() {
        assertEquals(StatusKind.ReadOnly, statusKind(ready(granted = granted(listOf("state.read", "control"), listOf(Op.PING), readOnly = true))))
    }

    @Test
    fun `an area is locked only when its gate op is locked`() {
        val state = ready(granted = granted(listOf("state.read", "control"), listOf(Op.PING, Op.WORKSPACE_FOCUS)))

        assertEquals("audio", lockedScopeOf(state, Area.Audio))
        assertEquals("launch", lockedScopeOf(state, Area.Launch))
        assertEquals("input.text", lockedScopeOf(state, Area.Keys))
        assertNull(lockedScopeOf(state, Area.Desktop))
        assertNull(lockedScopeOf(state, Area.More))
        assertNull("no lock before welcome", lockedScopeOf(ready(), Area.Audio))
    }

    @Test
    fun `the desktop badge follows any urgent workspace`() {
        assertTrue(anyUrgent(ready(desktop(urgent = true))))
        assertFalse(anyUrgent(ready(desktop(urgent = false))))
        assertFalse(anyUrgent(initial))
    }

    @Test
    fun `the selected label shows only when the whole bar fits`() {
        // 360dp screen, 16dp sides: a 328dp bar. 2 * 8 + (16 + 24 + 8 + 52 + 20) + 4 * 48 = 328.
        assertTrue(NavGeometry.labelFits(barWidth = 328f, labelWidth = 52f, itemCount = 5))
        assertFalse(NavGeometry.labelFits(barWidth = 328f, labelWidth = 53f, itemCount = 5))
        // Font scale 2.0 doubles the label: icon only, the label stays in semantics.
        assertFalse(NavGeometry.labelFits(barWidth = 328f, labelWidth = 104f, itemCount = 5))
    }

    @Test
    fun `the selected pill width is the formula of UX 4_2`() {
        assertEquals(16f + 24f + 8f + 50f + 20f, NavGeometry.selectedWidth(50f))
    }
}
