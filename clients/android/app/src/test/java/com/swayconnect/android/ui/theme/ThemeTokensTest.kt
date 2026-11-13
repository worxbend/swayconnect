package com.swayconnect.android.ui.theme

import androidx.compose.animation.core.SnapSpec
import androidx.compose.animation.core.SpringSpec
import androidx.compose.ui.graphics.vector.PathNode
import androidx.compose.ui.graphics.vector.VectorGroup
import androidx.compose.ui.graphics.vector.VectorPath
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeTokensTest {
    @Test
    fun `wash index is FNV-1a 32 of the UTF-8 app id modulo 8`() {
        // FNV-1a 32: "" -> 0x811C9DC5, "a" -> 0xE40C292C, "foobar" -> 0xBF9CF968 (reference vectors).
        assertEquals((0x811C9DC5u % 8u).toInt(), SwayTokens.washIndex(""))
        assertEquals((0xE40C292Cu % 8u).toInt(), SwayTokens.washIndex("a"))
        assertEquals((0xBF9CF968u % 8u).toInt(), SwayTokens.washIndex("foobar"))
        assertEquals(SwayTokens.washIndex("firefox"), SwayTokens.washIndex("firefox"))
        assertEquals(LightSwayTokens.washes[SwayTokens.washIndex("Alacritty")], LightSwayTokens.washFor("Alacritty"))
    }

    @Test
    fun `motion springs match UX 13 and reduce motion snaps only spatial specs`() {
        val spatial = SwayMotion.defaultSpatialSpec<Float>() as SpringSpec<Float>
        assertEquals(0.8f, spatial.dampingRatio)
        assertEquals(380f, spatial.stiffness)
        assertEquals(800f, (SwayMotion.fastSpatialSpec<Float>() as SpringSpec<Float>).stiffness)
        assertEquals(200f, (SwayMotion.slowSpatialSpec<Float>() as SpringSpec<Float>).stiffness)
        assertEquals(1600f, (SwayMotion.defaultEffectsSpec<Float>() as SpringSpec<Float>).stiffness)
        assertEquals(3800f, (SwayMotion.fastEffectsSpec<Float>() as SpringSpec<Float>).stiffness)
        assertEquals(500f, (SwayMotion.slowEffectsSpec<Float>() as SpringSpec<Float>).stiffness)

        assertTrue(SwayMotionReduced.defaultSpatialSpec<Float>() is SnapSpec)
        assertTrue(SwayMotionReduced.fastSpatialSpec<Float>() is SnapSpec)
        assertTrue(SwayMotionReduced.slowSpatialSpec<Float>() is SnapSpec)
        assertTrue(SwayMotionReduced.defaultEffectsSpec<Float>() is SpringSpec)
    }

    @Test
    fun `subtle haptics drop the tick types and off drops everything`() {
        assertFalse(HapticLevel.Subtle.allows(SwayHaptic.SegmentTick))
        assertFalse(HapticLevel.Subtle.allows(SwayHaptic.SegmentFrequentTick))
        assertTrue(HapticLevel.Subtle.allows(SwayHaptic.Reject))
        assertTrue(SwayHaptic.entries.all { HapticLevel.Full.allows(it) })
        assertTrue(SwayHaptic.entries.none { HapticLevel.Off.allows(it) })
    }

    @Test
    fun `about thirty glyphs, each a 24 grid vector with real paths, filled when it has a body`() {
        assertEquals(32, SwayIcons.all.size)
        assertEquals(SwayIcons.all.size, SwayIcons.all.map { it.outline.name }.toSet().size)
        for (icon in SwayIcons.all) {
            for (vector in listOf(icon.outline, icon.filled)) {
                assertEquals(24f, vector.viewportWidth)
                val paths = vector.root.allPaths()
                assertTrue(vector.name, paths.isNotEmpty())
                assertTrue(vector.name, paths.all { p -> p.pathData.first() is PathNode.MoveTo })
            }
        }
        val navAreas = listOf(SwayIcons.Desktop, SwayIcons.Audio, SwayIcons.Launch, SwayIcons.Keyboard, SwayIcons.More)
        navAreas.forEach { assertNotSame(it.outline.name, it.outline, it.filled) }
        assertSame(SwayIcons.Close.outline, SwayIcons.Close.filled)
    }

    private fun VectorGroup.allPaths(): List<VectorPath> = flatMap { node ->
        when (node) {
            is VectorPath -> listOf(node)
            is VectorGroup -> node.allPaths()
        }
    }
}
