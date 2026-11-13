package com.swayconnect.android.ui.map

import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.protocol.ContainerKind
import com.swayconnect.protocol.ContainerTile
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.DesktopSerializer
import com.swayconnect.protocol.Layout
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.WindowTile
import com.swayconnect.protocol.Wire
import com.swayconnect.protocol.normalized
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private const val EPS = 1e-3f

class MapGeometryTest {
    private val fixture: Desktop = run {
        val text = checkNotNull(javaClass.getResource("map_fixture.json")).readText()
        Wire.json.decodeFromString(DesktopSerializer, text).normalized()
    }

    private fun layoutOf(workspaceId: Long): Layout = checkNotNull(fixture.workspaces.single { it.id == workspaceId }.layout)

    private fun planOf(workspaceId: Long, showFloating: Boolean = true, fontScale: Float = 1f) =
        MapGeometry.place(layoutOf(workspaceId), fixture.windows, 328f, 200f, fontScale, showFloating)

    @Test
    fun `the formula of UX 6_1 letterboxes, scales and insets by 2dp`() {
        val layout = Layout(listOf(2560, 1440), null, WindowTile(1, listOf(0, 0, 5000, 10000)), emptyList())
        val plan = MapGeometry.place(layout, listOf(window(1)), 328f, 200f, fontScale = 1f)
        // mapW = min(328, 200 * 2560 / 1440) = 328; mapH = 184.5; offset = (0, 7.75).
        assertRect(MapRect(0f, 7.75f, 328f, 184.5f), plan.map)
        assertRect(MapRect(2f, 9.75f, 160f, 180.5f), plan.tiles.single().rect)
    }

    @Test
    fun `a tall workspace is letterboxed left and right`() {
        // mapW = min(328, 200 * 1080 / 1920) = 112.5; offset x = (328 - 112.5) / 2.
        assertRect(MapRect(107.75f, 0f, 112.5f, 200f), MapGeometry.letterbox(listOf(1080, 1920), 328f, 200f))
    }

    @Test
    fun `a rect past 10000 is clamped and a tile of an unknown window is skipped`() {
        val tiled = ContainerTile(
            ContainerKind.SPLIT_H,
            listOf(0, 0, 10000, 10000),
            listOf(WindowTile(1, listOf(9000, 0, 5000, 10000)), WindowTile(99, listOf(0, 0, 9000, 10000))),
        )
        val layout = Layout(listOf(1000, 1000), null, tiled, emptyList())
        val plan = MapGeometry.place(layout, listOf(window(1)), 100f, 100f, fontScale = 1f)
        assertEquals(listOf(1L), plan.tiles.map { it.windowId })
        // x 90..100 of a 100dp map, minus the inset.
        assertRect(MapRect(92f, 2f, 6f, 96f), plan.tiles.single().rect)
    }

    @Test
    fun `size classes follow UX 6_2 and large text falls to icon-only sooner`() {
        assertEquals(SizeClass.Large, MapGeometry.sizeClassOf(MapRect(0f, 0f, 120f, 80f), 1f))
        assertEquals(SizeClass.Medium, MapGeometry.sizeClassOf(MapRect(0f, 0f, 119f, 200f), 1f))
        assertEquals(SizeClass.Medium, MapGeometry.sizeClassOf(MapRect(0f, 0f, 48f, 48f), 1f))
        assertEquals(SizeClass.Small, MapGeometry.sizeClassOf(MapRect(0f, 0f, 300f, 47.9f), 1f))
        assertEquals(SizeClass.Small, MapGeometry.sizeClassOf(MapRect(0f, 0f, 24f, 24f), 1f))
        assertEquals(SizeClass.Sliver, MapGeometry.sizeClassOf(MapRect(0f, 0f, 300f, 23.9f), 1f))
        // At 200 % one title line and the app id line need 16 + 36 * 2 = 88dp.
        assertEquals(SizeClass.Medium, MapGeometry.sizeClassOf(MapRect(0f, 0f, 120f, 85f), 2f))
        assertEquals(SizeClass.Large, MapGeometry.sizeClassOf(MapRect(0f, 0f, 120f, 88f), 2f))
    }

    @Test
    fun `at font scale 2 a tile too narrow for a short title falls to the centred icon`() {
        // 8 characters of labelLarge at 200 % measure about 120dp; with the disc, the gap and the padding a large
        // tile needs 176dp. The 145 x 88 tile of the 7-Alacritty workspace showed "Alacri…" before.
        val shortTitle = 120f
        assertEquals(SizeClass.Medium, MapGeometry.sizeClassOf(MapRect(0f, 0f, 145f, 88f), 2f, shortTitle))
        assertEquals(SizeClass.Large, MapGeometry.sizeClassOf(MapRect(0f, 0f, 176f, 88f), 2f, shortTitle))
        // At 100 % the measured title (about 60dp) asks for less than the 120dp floor: nothing changes.
        assertEquals(SizeClass.Large, MapGeometry.sizeClassOf(MapRect(0f, 0f, 120f, 80f), 1f, 60f))
    }

    @Test
    fun `a 70dp same-app tile shows its index badge`() {
        // labelSmall "#2" is about 14dp wide; with 12dp of padding it needs 26dp. The old floor was 72dp.
        assertTrue(MapGeometry.indexBadgeFits(tileWidth = 70f, badgeTextWidth = 14f))
        assertTrue(MapGeometry.indexBadgeFits(tileWidth = 26f, badgeTextWidth = 14f))
        assertFalse(MapGeometry.indexBadgeFits(tileWidth = 25.9f, badgeTextWidth = 14f))
    }

    @Test
    fun `one sliver among three windows is not a dense workspace`() {
        val tiled = ContainerTile(
            ContainerKind.SPLIT_H,
            listOf(0, 0, 10000, 10000),
            listOf(WindowTile(1, listOf(0, 0, 4950, 10000)), WindowTile(2, listOf(4950, 0, 100, 10000)), WindowTile(3, listOf(5050, 0, 4950, 10000))),
        )
        val plan = MapGeometry.place(Layout(listOf(2560, 1440), null, tiled, emptyList()), listOf(window(1), window(2), window(3)), 328f, 200f, 1f)
        assertEquals(listOf(2L), plan.smallWindowIds)
        assertTrue("1 of 3 is over 30 %", plan.denseRatio > MapGeometry.DENSE_RATIO)
        assertFalse(plan.isDense)
    }

    @Test
    fun `hit test takes containment first, then the nearest edge within 24dp`() {
        // A long thin tile next to a small square one: by nearest centre the square would win.
        val thin = tile(1, MapRect(0f, 0f, 30f, 200f), SizeClass.Small)
        val square = tile(2, MapRect(34f, 0f, 30f, 30f), SizeClass.Small)
        val sliver = tile(3, MapRect(0f, 210f, 200f, 10f), SizeClass.Sliver)
        val plan = plan(thin, square, sliver)
        assertEquals(1L, plan.hit(28f, 10f)?.windowId) // inside the thin tile, 6dp from the square
        assertEquals(2L, plan.hit(36f, 10f)?.windowId)
        assertEquals(1L, plan.hit(32f, 100f)?.windowId) // gap: 2dp from the thin tile's edge
        assertEquals(1L, plan.hit(54f, 100f)?.windowId) // 24dp from the thin tile, 70dp from the square
        assertNull(plan.hit(55f, 100f)) // 25dp: too far
        assertNull(plan.hit(100f, 215f)) // on a sliver: nothing
    }

    @Test
    fun `the topmost tile wins and a dimmed tile is never hit`() {
        val below = tile(1, MapRect(0f, 0f, 100f, 100f), SizeClass.Medium)
        val above = tile(2, MapRect(40f, 40f, 30f, 30f), SizeClass.Small, layer = TileLayer.Floating)
        assertEquals(2L, plan(below, above).hit(50f, 50f)?.windowId)
        assertEquals(1L, plan(below, above).hit(10f, 10f)?.windowId)
        assertNull(plan(below.copy(dimmed = true)).hit(10f, 10f))
    }

    @Test
    fun `same-app windows get tree-order indexes, hidden tabs included`() {
        val plan = planOf(30)
        // Tree order: 41 (hidden tab), 42, 43, 44, 45 (hidden stacked row), then floats 46, 47.
        assertEquals(mapOf(41L to 1, 42L to 2, 45L to 3), plan.sameAppIndex)
        val dense = planOf(32)
        assertEquals((1..10).associate { (61L + it) to it }, dense.sameAppIndex)
    }

    @Test
    fun `tabbed and stacked containers get a header strip over the shown child`() {
        val plan = planOf(30)
        val tabbed = plan.groups.single { it.kind == GroupKind.Tabbed }
        assertEquals(listOf(listOf(41L), listOf(42L)), tabbed.tabs)
        assertEquals(1, tabbed.shown)
        // Map 328 x 181.51 at y 9.2448; left half inset by 2dp, strip 20dp.
        assertRect(MapRect(2f, 11.2448f, 160f, 20f), tabbed.header)
        assertTrue(tabbed.segmentsTappable) // 80dp per segment
        val shownTab = plan.tiles.single { it.windowId == 42L }
        assertRect(MapRect(2f, 33.2448f, 160f, 155.5104f), shownTab.rect)
        assertFalse(plan.tiles.any { it.windowId == 41L })

        val stacked = plan.groups.single { it.kind == GroupKind.Stacked }
        assertEquals(0, stacked.shown)
        assertEquals(2, stacked.stackedRowsShown)
        assertEquals(16f, stacked.header.height, EPS)
        assertFalse(stacked.segmentsTappable)
        assertEquals(listOf(42L, 43L, 44L, 46L, 47L), plan.tiles.map { it.windowId })
        assertEquals(MapKind.SplitH, plan.kind)
        assertEquals(7, plan.windowCount)
    }

    @Test
    fun `floating windows draw last and hide with the chip`() {
        val plan = planOf(30)
        assertEquals(listOf(TileLayer.Floating, TileLayer.Floating), plan.tiles.takeLast(2).map { it.layer })
        assertEquals(2, plan.floatingCount)
        // Two 61.6 x 32.3dp floats on a 328 x 181.5dp map: 6.7 %, sampled on a 64 x 64 grid.
        assertEquals(0.067f, plan.floatingCoverage, 0.005f)
        assertFalse(plan.showsFloatingChip)
        val hidden = planOf(30, showFloating = false)
        assertFalse(hidden.tiles.any { it.layer == TileLayer.Floating })
        assertEquals(plan.floatingCoverage, hidden.floatingCoverage, EPS)
        // 90 % x 90 % less the inset: the chip shows.
        assertTrue(planOf(31).showsFloatingChip)
    }

    @Test
    fun `a fullscreen window covers the map and dims the rest`() {
        val plan = planOf(31)
        val full = plan.tiles.last()
        assertEquals(52L, full.windowId)
        assertEquals(TileLayer.Fullscreen, full.layer)
        assertRect(plan.map.inset(2f), full.rect)
        assertEquals(MapPosition.Whole, full.position)
        assertTrue(plan.tiles.dropLast(1).all { it.dimmed })
        assertEquals(listOf(51L, 53L), plan.tiles.dropLast(1).map { it.windowId })
        assertEquals(52L, plan.hit(10f, 100f)?.windowId)
    }

    @Test
    fun `dense ratio counts tiles under 24dp`() {
        val dense = planOf(32)
        // 10 rows of 18.15dp each beside one wide tile.
        assertEquals(10f / 11f, dense.denseRatio, EPS)
        assertTrue(dense.isDense)
        assertEquals((62L..71L).toList(), dense.smallWindowIds)
        assertFalse(planOf(30).isDense)
        assertEquals(0f, planOf(30).denseRatio, EPS)
    }

    @Test
    fun `same-app tiles under the large class show a title fragment when 120 x 56 fits`() {
        val tiled = ContainerTile(
            ContainerKind.SPLIT_V,
            listOf(0, 0, 10000, 10000),
            listOf(WindowTile(1, listOf(0, 0, 10000, 5000)), WindowTile(2, listOf(0, 5000, 10000, 5000))),
        )
        // 300 x 120 map: two 296 x 56 tiles.
        val plan = MapGeometry.place(Layout(listOf(300, 120), null, tiled, emptyList()), listOf(window(1), window(2)), 300f, 120f, 1f)
        assertTrue(plan.tiles.all { it.sizeClass == SizeClass.Medium && it.showsTitleFragment })
    }

    @Test
    fun `positions name the part of the map`() {
        val map = MapRect(0f, 0f, 300f, 100f)
        assertEquals(MapPosition.Left, MapGeometry.positionOf(MapRect(0f, 0f, 150f, 100f), map))
        assertEquals(MapPosition.BottomRight, MapGeometry.positionOf(MapRect(150f, 50f, 150f, 50f), map))
        assertEquals(MapPosition.Top, MapGeometry.positionOf(MapRect(0f, 0f, 300f, 50f), map))
        assertEquals(MapPosition.Centre, MapGeometry.positionOf(MapRect(100f, 25f, 100f, 50f), map))
        assertEquals(MapPosition.Whole, MapGeometry.positionOf(map, map))
    }

    @Test
    fun `an unusable layout size gives an empty plan over the whole box`() {
        val plan = MapGeometry.place(Layout(listOf(0, 0), null, null, emptyList()), emptyList(), 100f, 50f, 1f)
        assertRect(MapRect(0f, 0f, 100f, 50f), plan.map)
        assertTrue(plan.tiles.isEmpty())
        assertEquals(MapKind.Empty, plan.kind)
    }

    private fun assertRect(expected: MapRect, actual: MapRect) {
        val message = "expected $expected, got $actual"
        assertEquals(message, expected.left, actual.left, EPS)
        assertEquals(message, expected.top, actual.top, EPS)
        assertEquals(message, expected.width, actual.width, EPS)
        assertEquals(message, expected.height, actual.height, EPS)
    }

    private fun tile(id: Long, rect: MapRect, sizeClass: SizeClass, layer: TileLayer = TileLayer.Tiled) =
        PlacedTile(id, rect, sizeClass, layer, treeOrder = id.toInt(), position = MapPosition.Centre, dimmed = false, showsTitleFragment = false)

    private fun plan(vararg tiles: PlacedTile) = MapPlan(
        map = MapRect(0f, 0f, 400f, 400f),
        tiles = tiles.toList(),
        groups = emptyList(),
        kind = MapKind.SplitH,
        sameAppIndex = emptyMap(),
        fullscreenWindowId = null,
        floatingCoverage = 0f,
        floatingCount = 0,
        windowCount = tiles.size,
    )

    private fun window(id: Long) = Window(
        id = id,
        workspaceId = 1,
        appId = "app",
        title = "title",
        focused = false,
        urgent = false,
        floating = false,
        fullscreen = false,
    )
}
