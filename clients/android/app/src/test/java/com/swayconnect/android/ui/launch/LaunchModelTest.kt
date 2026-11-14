package com.swayconnect.android.ui.launch

import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.SCOPE_LAUNCH_RISKY
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.withApps
import com.swayconnect.android.ui.state.withAppsStale
import com.swayconnect.protocol.AppsState
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.FrameLimits
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.ServerInfo
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Welcome
import com.swayconnect.protocol.Wire
import com.swayconnect.protocol.Workspace
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The Launch area rules of docs/UX.md 8, from protocol/vectors/v1/ext/apps.json. */
class LaunchModelTest {
    private val withRisky = granted(DEFAULT_SCOPES + SCOPE_LAUNCH_RISKY)
    private val withoutRisky = granted(DEFAULT_SCOPES)

    @Test
    fun `recent and all apps as cells`() {
        val part = shown(withoutRisky)
        assertEquals(listOf("firefox", "Alacritty"), part.recent.map { it.app.entryId })
        assertEquals(listOf("Alacritty", "org.gnome.Nautilus", "firefox", "gparted", "htop", "mpv"), part.all.map { it.app.entryId })
        assertFalse(part.truncated)
        assertTrue(part.live)
    }

    @Test
    fun `risky apps are locked without launch risky`() {
        val taps = shown(withoutRisky).all.associate { it.app.entryId to it.tap() }
        assertEquals(LaunchTap.ShowLocked(SCOPE_LAUNCH_RISKY), taps["Alacritty"])
        assertEquals("a terminal entry", LaunchTap.ShowLocked(SCOPE_LAUNCH_RISKY), taps["htop"])
        assertEquals(LaunchTap.ShowLocked(SCOPE_LAUNCH_RISKY), taps["gparted"])
        assertEquals(LaunchTap.Send, taps["firefox"])
        assertEquals(LaunchTap.Send, taps["mpv"])
    }

    @Test
    fun `only a cell refused by its op looks disabled, not a stale or pending one`() {
        val firefox = shown(withoutRisky).all.first { it.app.entryId == "firefox" }
        assertFalse(firefox.looksDisabled())
        assertTrue("read-only", firefox.copy(access = OpAvailability.ReadOnly).looksDisabled())
        assertFalse("the stale grid dims as a whole", firefox.copy(access = OpAvailability.ReadOnly, live = false).looksDisabled())
        assertFalse("a pending start shows its spinner", firefox.copy(pending = true).looksDisabled())
        assertFalse("a locked cell shows its lock", firefox.copy(access = OpAvailability.Locked(SCOPE_LAUNCH_RISKY)).looksDisabled())
    }

    @Test
    fun `a read-only cell fades only its badge, another refusal the whole cell`() {
        val firefox = shown(withoutRisky).all.first { it.app.entryId == "firefox" }
        assertEquals(CellLook.Normal, firefox.look())
        assertEquals(CellLook.ReadOnly, firefox.copy(access = OpAvailability.ReadOnly).look())
        assertEquals(CellLook.Disabled, firefox.copy(access = OpAvailability.Unsupported).look())
        assertEquals("the stale grid dims as a whole", CellLook.Normal, firefox.copy(access = OpAvailability.ReadOnly, live = false).look())
        assertEquals("a pending start shows its spinner", CellLook.Normal, firefox.copy(pending = true).look())
        assertEquals("a locked cell shows its lock", CellLook.Normal, firefox.copy(access = OpAvailability.Locked(SCOPE_LAUNCH_RISKY)).look())
    }

    @Test
    fun `risky apps need the hold with launch risky, others a tap`() {
        val taps = shown(withRisky).all.associate { it.app.entryId to it.tap() }
        assertEquals(LaunchTap.Hold, taps["Alacritty"])
        assertEquals(LaunchTap.Hold, taps["htop"])
        assertEquals(LaunchTap.Hold, taps["gparted"])
        assertEquals(LaunchTap.Send, taps["firefox"])
        assertEquals(LaunchTap.Hold, shown(withRisky).recent.first { it.app.entryId == "Alacritty" }.tap())
    }

    @Test
    fun `read-only, a stale list and a waiting start take no touch`() {
        val readOnly = shown(granted(DEFAULT_SCOPES + SCOPE_LAUNCH_RISKY, readOnly = true))
        assertTrue(readOnly.all.all { it.tap() == LaunchTap.None })

        val stale = launchPart(stateWith(withRisky).withApps(appsState("01-six-apps")).withAppsStale(), "") as LaunchPart.Shown
        assertFalse(stale.live)
        assertTrue(stale.all.all { it.tap() == LaunchTap.None })

        val waiting = stateWith(withRisky).withApps(appsState("01-six-apps")).copy(pendingControl = PendingControl.Launch("firefox"))
        val cells = (launchPart(waiting, "") as LaunchPart.Shown).all.associateBy { it.app.entryId }
        assertTrue(cells.getValue("firefox").pending)
        assertEquals(LaunchTap.None, cells.getValue("firefox").tap())
        assertEquals("another app still starts", LaunchTap.Send, cells.getValue("mpv").tap())
    }

    @Test
    fun `a locked risky app still opens the Locked sheet on a stale list`() {
        val stale = launchPart(stateWith(withoutRisky).withApps(appsState("01-six-apps")).withAppsStale(), "") as LaunchPart.Shown
        assertEquals(LaunchTap.ShowLocked(SCOPE_LAUNCH_RISKY), stale.all.first { it.app.entryId == "Alacritty" }.tap())
    }

    @Test
    fun `a search hides Recent and shows the matches only`() {
        val part = launchPart(stateWith(withRisky).withApps(appsState("01-six-apps")), "browser") as LaunchPart.Shown
        assertEquals(emptyList<String>(), part.recent.map { it.app.entryId })
        assertEquals(listOf("firefox"), part.all.map { it.app.entryId })
        val none = launchPart(stateWith(withRisky).withApps(appsState("01-six-apps")), "xyz") as LaunchPart.Shown
        assertTrue("No app matches", none.all.isEmpty())
    }

    @Test
    fun `no apps, a cut list`() {
        assertTrue((launchPart(stateWith(withRisky).withApps(appsState("02-no-apps")), "") as LaunchPart.Shown).all.isEmpty())
        assertTrue((launchPart(stateWith(withRisky).withApps(appsState("03-truncated")), "") as LaunchPart.Shown).truncated)
    }

    @Test
    fun `waiting, not offered and locked`() {
        assertEquals("before welcome", LaunchPart.Waiting, launchPart(UiState(pair = PairForm(deviceName = "Pixel")), ""))
        assertEquals("no apps state yet", LaunchPart.Waiting, launchPart(stateWith(withRisky), ""))
        assertEquals(LaunchPart.Hidden, launchPart(stateWith(granted(DEFAULT_SCOPES, knownOps = listOf(Op.PING))), ""))
        assertEquals(LaunchPart.Locked("launch"), launchPart(stateWith(granted(listOf("state.read", "control"))).withApps(appsState("01-six-apps")), ""))
    }

    @Test
    fun `targets are the workspace numbers app launch accepts, in order`() {
        val desktop = desktop(
            Workspace(10, 3, "3", null, false, false, false),
            Workspace(11, 1, "1", null, true, true, false),
            Workspace(12, null, "mail", null, false, false, false),
            Workspace(13, 1000, "1000", null, false, false, false),
            Workspace(14, -1, "-1", null, false, false, false),
            Workspace(15, 0, "0", null, false, false, false),
        )
        assertEquals(listOf(0, 1, 3), launchTargets(desktop))
        assertEquals(emptyList<Int>(), launchTargets(null))
    }

    @Test
    fun `the pill shows the picked number, else the focused workspace's`() {
        val focusedThree = desktop(Workspace(1, 1, "1", null, false, false, false), Workspace(3, 3, "3", null, true, true, false))
        assertEquals(3, shownTarget(null, focusedThree))
        assertEquals(1, shownTarget(1, focusedThree))
        val focusedNamed = desktop(Workspace(1, 1, "1", null, false, false, false), Workspace(9, null, "mail", null, true, true, false))
        assertNull(shownTarget(null, focusedNamed))
    }

    @Test
    fun `Desktop's plus aims at the shown page when it has a number`() {
        val workspaces = desktop(Workspace(1, 1, "1", null, true, true, false), Workspace(9, null, "mail", null, false, false, false))
        val base = stateWith(withRisky).copy(desktop = workspaces)
        assertEquals(1, shownPageTarget(base.copy(selectedWorkspaceId = 1)))
        assertNull(shownPageTarget(base.copy(selectedWorkspaceId = 9)))
        assertNull(shownPageTarget(base.copy(selectedWorkspaceId = null)))
    }

    private fun shown(granted: Granted): LaunchPart.Shown = launchPart(stateWith(granted).withApps(appsState("01-six-apps")), "") as LaunchPart.Shown

    private fun stateWith(granted: Granted) = UiState(pair = PairForm(deviceName = "Pixel"), desktopLive = true, granted = granted)

    private companion object {
        val DEFAULT_SCOPES = listOf("state.read", "state.titles", "control", "launch")
        val LAUNCH_OPS = listOf(Op.APP_LAUNCH, Op.LAUNCHER_OPEN)

        /** A server that knows the launch ops; it grants them when the phone holds `launch`. */
        fun granted(scopes: List<String>, readOnly: Boolean = false, knownOps: List<String> = LAUNCH_OPS): Granted =
            Granted.of(
                Welcome(
                    protocol = 1,
                    deviceId = "device",
                    serverProof = "proof",
                    scopes = scopes,
                    ops = listOf(Op.PING) + if (readOnly || "launch" !in scopes) emptyList() else knownOps,
                    topics = listOf("desktop", "apps"),
                    readOnly = readOnly,
                    server = ServerInfo("desk", "0.2.0"),
                    limits = FrameLimits(16384, 1048576, 90000),
                    knownOps = knownOps,
                ),
            )

        fun desktop(vararg workspaces: Workspace) = Desktop(SwayStatus(true, null), workspaces.toList(), emptyList(), truncated = false)

        fun appsState(name: String): AppsState {
            val file = Wire.json.parseToJsonElement(vectorFile("ext/apps.json").readText()).jsonObject
            val case = file.getValue("states").jsonArray.first { it.jsonObject.getValue("name").jsonPrimitive.content == name }
            val frame = Wire.json.parseToJsonElement(case.jsonObject.getValue("payload").jsonPrimitive.content).jsonObject
            return Wire.decode(AppsState.serializer(), frame.getValue("data").jsonObject.getValue("data"))
        }

        /** The shared vectors, found from the test's working directory up to the repository root. */
        fun vectorFile(path: String): File {
            var dir: File? = File(checkNotNull(System.getProperty("user.dir"))).absoluteFile
            while (dir != null) {
                val candidate = File(dir, "protocol/vectors/v1/$path")
                if (candidate.isFile) return candidate
                dir = dir.parentFile
            }
            error("protocol/vectors/v1/$path not found above ${System.getProperty("user.dir")}")
        }
    }
}
