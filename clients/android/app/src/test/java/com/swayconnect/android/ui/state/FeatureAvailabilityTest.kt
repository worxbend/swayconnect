package com.swayconnect.android.ui.state

import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.Welcome
import org.junit.Assert.assertEquals
import org.junit.Test

/** Every row of the client rule of docs/PROTOCOL.md 17.3. */
class FeatureAvailabilityTest {
    private val allOps =
        listOf(Op.PING, Op.STATE_SUBSCRIBE, Op.ICON_GET, Op.WORKSPACE_FOCUS, Op.WINDOW_FOCUS, Op.WINDOW_FULLSCREEN, "window.close")

    private fun granted(scopes: List<String>, ops: List<String>, knownOps: List<String>?, readOnly: Boolean = false) =
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
                knownScopes = knownOps?.let { listOf("state.read", "control", "destructive") },
                knownOps = knownOps,
            ),
        )

    @Test
    fun `an op in ops is available`() {
        val granted = granted(listOf("state.read", "control"), listOf(Op.PING, Op.WORKSPACE_FOCUS), allOps)

        assertEquals(OpAvailability.Available, granted.availabilityOf(Op.WORKSPACE_FOCUS))
    }

    @Test
    fun `a known op whose scope is held on a read-only server is read-only`() {
        val granted = granted(listOf("state.read", "control"), listOf(Op.PING, Op.STATE_SUBSCRIBE), allOps, readOnly = true)

        assertEquals(OpAvailability.ReadOnly, granted.availabilityOf(Op.WORKSPACE_FOCUS))
    }

    @Test
    fun `a known op whose scope is missing is locked with that scope`() {
        val granted = granted(listOf("state.read"), listOf(Op.PING, Op.STATE_SUBSCRIBE), allOps)

        assertEquals(OpAvailability.Locked("control"), granted.availabilityOf(Op.WORKSPACE_FOCUS))
        assertEquals(OpAvailability.Locked("destructive"), granted.availabilityOf("window.close"))
    }

    @Test
    fun `an op that is not in known_ops is unsupported`() {
        val granted = granted(listOf("state.read", "control"), listOf(Op.PING), allOps)

        assertEquals(OpAvailability.Unsupported, granted.availabilityOf("keep_awake.set"))
        assertEquals(OpAvailability.Unsupported, granted.availabilityOf("no.such_op"))
    }

    @Test
    fun `without known_ops only the ops in ops are available`() {
        val granted = granted(listOf("state.read"), listOf(Op.PING, Op.WINDOW_FOCUS), knownOps = null, readOnly = true)

        assertEquals(OpAvailability.Available, granted.availabilityOf(Op.WINDOW_FOCUS))
        assertEquals(OpAvailability.Unsupported, granted.availabilityOf(Op.WORKSPACE_FOCUS))
        assertEquals(null, granted.knownScopes)
    }

    @Test
    fun `known scopes are not granted scopes`() {
        val granted = granted(listOf("state.read"), listOf(Op.PING), allOps)

        assertEquals(listOf("state.read"), granted.scopes)
        assertEquals(listOf("state.read", "control", "destructive"), granted.knownScopes)
    }

    @Test
    fun `drive needs workspace focus to be available and a live desktop`() {
        val base = UiState(pair = PairForm(deviceName = "Pixel 9"), desktopLive = true)
        val control = granted(listOf("state.read", "control"), listOf(Op.WORKSPACE_FOCUS), allOps)
        val readOnly = granted(listOf("state.read", "control"), listOf(Op.PING), allOps, readOnly = true)

        assertEquals(true, base.copy(granted = control).isDriving)
        assertEquals(false, base.copy(granted = control, drive = DriveMode.Browse).isDriving)
        assertEquals(false, base.copy(granted = readOnly).isDriving)
        assertEquals(false, base.copy(granted = control, desktopLive = false).isDriving)
    }
}
