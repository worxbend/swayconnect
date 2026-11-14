package com.swayconnect.android.ui.desktop

import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.android.ui.common.GroupPosition
import com.swayconnect.android.ui.state.ViewMode
import com.swayconnect.protocol.App
import com.swayconnect.protocol.ContainerKind
import com.swayconnect.protocol.ContainerTile
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Layout
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.WindowLayout
import com.swayconnect.protocol.WindowTile
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DesktopModelTest {
    private val whole = listOf(0L, 0L, 10000L, 10000L)

    private fun workspace(id: Long, name: String, number: Long? = id, layout: Layout? = null) =
        Workspace(id = id, number = number, name = name, output = "DP-1", focused = false, visible = false, urgent = false, layout = layout)

    private fun window(id: Long, workspaceId: Long = 1, appId: String? = "app$id", title: String? = "t$id", focused: Boolean = false) =
        Window(id, workspaceId, appId, title, focused = focused, urgent = false, floating = false, fullscreen = false)

    private fun desktop(workspaces: List<Workspace>, windows: List<Window>, layoutsCut: Boolean? = null, outputs: Boolean = true) =
        Desktop(SwayStatus(true, "1.10"), workspaces, windows, truncated = false, outputs = if (outputs) emptyList() else null, layoutsCut = layoutsCut)

    @Test
    fun `a card drops the N colon prefix and an equal name`() {
        assertEquals(CardText("code", "3"), cardText(workspace(3, "3:code")))
        assertEquals(CardText("", "3"), cardText(workspace(3, "3")))
        assertEquals(CardText("3code", "3"), cardText(workspace(3, "3code")))
        assertEquals("3", cardText(workspace(3, "3")).label())
        assertEquals("code", cardText(workspace(3, "3:code")).label())
    }

    @Test
    fun `a workspace without a number shows its first two graphemes`() {
        assertEquals(CardText("mail", "ma"), cardText(workspace(9, "mail", number = null)))
        // A flag emoji is one grapheme of two code points: it is never cut in half.
        assertEquals("🇩🇪x", cardText(workspace(9, "🇩🇪xyz", number = null)).number)
        assertEquals("a", cardText(workspace(9, "a", number = null)).number)
    }

    @Test
    fun `display text replaces control and format characters`() {
        val shown = desktop(listOf(workspace(1, "1:‮evil")), listOf(window(5, title = "a\u0007b", appId = "x‎y"))).forDisplay()
        assertEquals("1:�evil", shown.workspaces.single().name)
        // Titles: the replaced character is never shown as itself; inside a title it reads as a space (displayTitle).
        assertEquals("a b", shown.windows.single().title)
        assertEquals("x�y", shown.windows.single().appId)
    }

    @Test
    fun `windows come in tree order, tiled then floating, then those the layout leaves out`() {
        val layout = Layout(
            size = listOf(1920, 1080),
            fullscreen = null,
            tiled = ContainerTile(ContainerKind.SPLIT_H, whole, listOf(WindowTile(3, whole), WindowTile(1, whole))),
            floating = listOf(WindowTile(2, whole)),
        )
        val ws = workspace(1, "1", layout = layout)
        val page = pageWindows(desktop(listOf(ws), listOf(window(1), window(2), window(3), window(4), window(9, workspaceId = 2))), ws)
        assertEquals(listOf(3L, 1L, 2L, 4L), page.ordered.map { it.id })
        assertEquals(listOf(4L), page.outsideLayout.map { it.id })
    }

    @Test
    fun `unknown and repeated ids in the layout are skipped`() {
        val layout = Layout(
            listOf(1920, 1080),
            null,
            ContainerTile(ContainerKind.SPLIT_V, whole, listOf(WindowTile(77, whole), WindowTile(1, whole), WindowTile(1, whole))),
            emptyList(),
        )
        val ws = workspace(1, "1", layout = layout)
        assertEquals(listOf(1L), pageWindows(desktop(listOf(ws), listOf(window(1))), ws).ordered.map { it.id })
    }

    @Test
    fun `tabbed and stacked children get their group position, nested splits keep it`() {
        val tabs = ContainerTile(
            ContainerKind.TABBED,
            whole,
            listOf(WindowTile(1, whole), ContainerTile(ContainerKind.SPLIT_H, whole, listOf(WindowTile(2, whole), WindowTile(3, whole)))),
            active = 0,
        )
        val stack = ContainerTile(ContainerKind.STACKED, whole, listOf(WindowTile(4, whole), WindowTile(5, whole), WindowTile(6, whole)))
        val ws = workspace(1, "1", layout = Layout(listOf(1, 1), null, ContainerTile(ContainerKind.SPLIT_H, whole, listOf(tabs, stack)), emptyList()))
        val page = pageWindows(desktop(listOf(ws), (1L..6L).map { window(it) }), ws)
        assertEquals(GroupPosition(GroupKind.Tabbed, 1, 2), page.groups[1])
        assertEquals(GroupPosition(GroupKind.Tabbed, 2, 2), page.groups[2])
        assertEquals(GroupPosition(GroupKind.Tabbed, 2, 2), page.groups[3])
        assertEquals(GroupPosition(GroupKind.Stacked, 3, 3), page.groups[6])
    }

    @Test
    fun `same-app windows are numbered in tree order`() {
        val layout = Layout(listOf(1, 1), null, ContainerTile(ContainerKind.SPLIT_H, whole, listOf(WindowTile(2, whole), WindowTile(1, whole))), emptyList())
        val ws = workspace(1, "1", layout = layout)
        val page = pageWindows(desktop(listOf(ws), listOf(window(1, appId = "term"), window(2, appId = "term"), window(3, appId = "web"))), ws)
        assertEquals(mapOf(2L to 1, 1L to 2), page.sameAppIndex)
    }

    @Test
    fun `the map shows unless the layout is missing or the page asked for the list`() {
        val layout = Layout(listOf(1, 1), null, null, emptyList())
        val ws = workspace(1, "1", layout = layout)
        val d = desktop(listOf(ws), emptyList())
        assertEquals(PageMode(showMap = true, forced = null), pageMode(d, ws, ViewMode.Map, listForPage = false))
        assertEquals(PageMode(showMap = false, forced = null), pageMode(d, ws, ViewMode.Map, listForPage = true))
        assertEquals(PageMode(showMap = false, forced = null), pageMode(d, ws, ViewMode.List, listForPage = false))
    }

    @Test
    fun `replacement marks from invisible direction marks do not show in titles`() {
        // Telegram: U+200E U+2068 ... U+2069 arrive as U+FFFD (docs/PROTOCOL.md 2.4).
        assertEquals("Мій ЛУН бот", displayTitle("\uFFFD\uFFFDМій\uFFFDЛУН бот\uFFFD"))
        assertEquals("a b", displayTitle("a\uFFFD\uFFFD\uFFFDb"))
        assertEquals("plain", displayTitle("plain"))
        assertEquals("an inner control character becomes one space, never itself", "a b", displayTitle("a\u0007b"))
        assertNull("nothing left: the app id stands in", displayTitle("\uFFFD\u200E"))
    }

    @Test
    fun `a cut layout forces the list with its chip, an old daemon disables the toggle`() {
        val bare = workspace(1, "1")
        assertEquals(ListReason.LayoutCut, pageMode(desktop(listOf(bare), emptyList(), layoutsCut = true), bare, ViewMode.Map, false).forced)
        assertEquals(ListReason.OldDaemon, pageMode(desktop(listOf(bare), emptyList(), outputs = false), bare, ViewMode.Map, false).forced)
        // A v2 server that sends `layout: null` without `layouts_cut` (a workspace with no measurable area): not
        // "too large", and not old either.
        assertEquals(ListReason.NoLayout, pageMode(desktop(listOf(bare), emptyList()), bare, ViewMode.Map, false).forced)
        assertEquals(ListReason.NoLayout, pageMode(desktop(listOf(bare), emptyList(), layoutsCut = false), bare, ViewMode.Map, false).forced)
        assertFalse(pageMode(desktop(listOf(bare), emptyList()), bare, ViewMode.Map, false).showMap)
    }

    @Test
    fun `the focused window card only takes a window of its own page`() {
        val one = workspace(1, "1")
        val two = workspace(2, "2")
        val d = desktop(listOf(one, two), listOf(window(1, workspaceId = 2, focused = true), window(2, workspaceId = 1)))
        assertNull(focusedWindowOn(d, one))
        assertEquals(1L, focusedWindowOn(d, two)?.id)
        assertEquals(1, windowCount(d, one))
    }

    @Test
    fun `last seen counts seconds for a minute, then minutes`() {
        assertEquals(LastSeen(12, inMinutes = false), LastSeen.of(nowMs = 112_500, seenAtMs = 100_000))
        assertEquals(LastSeen(59, inMinutes = false), LastSeen.of(nowMs = 59_999, seenAtMs = 0))
        assertEquals(LastSeen(1, inMinutes = true), LastSeen.of(nowMs = 60_000, seenAtMs = 0))
        assertEquals(LastSeen(0, inMinutes = false), LastSeen.of(nowMs = 0, seenAtMs = 5_000))
        assertTrue(LastSeen(1, inMinutes = true).nextTickMs > LastSeen(1, inMinutes = false).nextTickMs)
    }

    @Test
    fun `new workspace number is the lowest free one from 1`() {
        assertEquals(1, lowestFreeNumber(desktop(emptyList(), emptyList())))
        // Unnumbered workspaces and number 0 take no number from 1 up; a gap is filled first.
        val some = listOf(workspace(10, "0", number = 0), workspace(11, "1", number = 1), workspace(12, "mail", number = null), workspace(13, "3", number = 3))
        assertEquals(2, lowestFreeNumber(desktop(some, emptyList())))
        val upTo998 = (1L..998L).map { workspace(it, "$it") }
        assertEquals(999, lowestFreeNumber(desktop(upTo998, emptyList())))
        assertNull(lowestFreeNumber(desktop(upTo998 + workspace(999, "999"), emptyList())))
    }

    @Test
    fun `container layout is the kind of the window's nearest parent`() {
        val tabbed = ContainerTile(ContainerKind.TABBED, whole, listOf(WindowTile(2, whole), WindowTile(3, whole)), active = 0)
        val tree = ContainerTile(ContainerKind.SPLIT_H, whole, listOf(WindowTile(1, whole), ContainerTile(ContainerKind.SPLIT_V, whole, listOf(tabbed, WindowTile(4, whole)))))
        val ws = workspace(1, "1", layout = Layout(listOf(1920L, 1080L), fullscreen = null, tiled = tree, floating = listOf(WindowTile(5, whole))))
        assertEquals(WindowLayout.SPLIT_H, containerLayoutOf(ws, 1))
        assertEquals(WindowLayout.TABBED, containerLayoutOf(ws, 3))
        assertEquals(WindowLayout.SPLIT_V, containerLayoutOf(ws, 4))
        // Floating, not in the tree, no layout, or the only tile: no container to mark.
        assertNull(containerLayoutOf(ws, 5))
        assertNull(containerLayoutOf(ws, 9))
        assertNull(containerLayoutOf(workspace(2, "2"), 1))
        val single = workspace(3, "3", layout = Layout(listOf(1920L, 1080L), fullscreen = null, tiled = WindowTile(7, whole), floating = emptyList()))
        assertNull(containerLayoutOf(single, 7))
    }

    @Test
    fun `stacked containers map to the stacking layout`() {
        val tree = ContainerTile(ContainerKind.STACKED, whole, listOf(WindowTile(1, whole)), active = 0)
        val ws = workspace(1, "1", layout = Layout(listOf(1920L, 1080L), fullscreen = null, tiled = tree, floating = emptyList()))
        assertEquals(WindowLayout.STACKING, containerLayoutOf(ws, 1))
    }

    private fun app(entryId: String, name: String) =
        App(entryId = entryId, name = name, genericName = null, keywords = emptyList(), icon = null, terminal = false, risky = false)

    @Test
    fun `an app id shows the name of its desktop entry, matched without the suffix and ignoring case`() {
        val names = appNames(listOf(app("firefox.desktop", "Firefox"), app("org.gnome.Nautilus.desktop", "Files"), app("Alacritty", "Alacritty")))
        assertEquals("Firefox", appNameFor("firefox", names))
        assertEquals("Firefox", appNameFor("Firefox", names))
        assertEquals("Files", appNameFor("org.gnome.Nautilus", names))
        assertEquals("Alacritty", appNameFor("alacritty", names))
    }

    @Test
    fun `an app id without an entry, or with a blank name, shows itself`() {
        val names = appNames(listOf(app("firefox.desktop", "Firefox"), app("blank.desktop", " ")))
        assertEquals("sealed_sink_1", appNameFor("sealed_sink_1", names))
        assertEquals("blank", appNameFor("blank", names))
        assertEquals("fire", appNameFor("fire", names))
        assertEquals("firefox", appNameFor("firefox", appNames(emptyList())))
    }

    @Test
    fun `a reverse-DNS app id falls back to the entry named by its last segment`() {
        val names = appNames(listOf(app("codium.desktop", "VSCodium"), app("org.example.Notes.desktop", "Notes")))
        assertEquals("VSCodium", appNameFor("com.vscodium.codium", names))
        assertEquals("the exact id wins", "Notes", appNameFor("org.example.Notes", names))
        assertEquals("no entry for the last segment either", "dev.vencord.Vesktop", appNameFor("dev.vencord.Vesktop", names))
        assertEquals("a dot at the end matches nothing", "codium.", appNameFor("codium.", names))
    }

    @Test
    fun `the first entry of an id names it`() {
        val names = appNames(listOf(app("foot.desktop", "Foot"), app("Foot.desktop", "Foot server")))
        assertEquals("Foot", appNameFor("foot", names))
    }

    @Test
    fun `an entry name with control characters is made safe`() {
        val shown = appNameFor("evil", appNames(listOf(app("evil.desktop", "Ev\u0007il"))))
        assertFalse(shown.contains('\u0007'))
    }

    @Test
    fun `the focused row keeps its labels only when the buttons, divider and Close fit`() {
        // Full, Float, Move at 28, 35, 38dp: buttons 74 + 81 + 84, gaps 4 x 8, divider 1, Close 48 = 320dp, plus 2dp
        // for whole-pixel rounding = 322dp.
        val labels = listOf(28f, 35f, 38f)
        assertTrue(FocusedRow.labelsFit(322f, labels))
        assertTrue(FocusedRow.labelsFit(347f, labels))
        assertFalse(FocusedRow.labelsFit(321f, labels))
        // 360dp window: 296dp of card content.
        assertFalse(FocusedRow.labelsFit(296f, labels))
        // Font 1.3 on a 411dp window: labels 30 % wider no longer fit 347dp.
        assertFalse(FocusedRow.labelsFit(347f, labels.map { it * 1.3f }))
    }

    @Test
    fun `keep in view scrolls only a card that is cut by an edge, and only as far as needed`() {
        assertEquals(0, keepInViewDelta(offset = 10, size = 64, start = 0, end = 300))
        assertEquals(0, keepInViewDelta(offset = 0, size = 64, start = 0, end = 64))
        assertEquals(-20, keepInViewDelta(offset = -20, size = 64, start = 0, end = 300))
        assertEquals(14, keepInViewDelta(offset = 250, size = 64, start = 0, end = 300))
        // Wider than the span: its start lines up on the start.
        assertEquals(10, keepInViewDelta(offset = 10, size = 400, start = 0, end = 300))
    }

    @Test
    fun `a first card cut by the start edge comes back whole only when the target stays in view`() {
        assertEquals(-40, revealFirstDelta(firstOffset = -40, targetEnd = 200, end = 300))
        assertEquals(-40, revealFirstDelta(firstOffset = -40, targetEnd = 260, end = 300))
        assertEquals(0, revealFirstDelta(firstOffset = -40, targetEnd = 261, end = 300))
        assertEquals(0, revealFirstDelta(firstOffset = 0, targetEnd = 100, end = 300))
    }
}
