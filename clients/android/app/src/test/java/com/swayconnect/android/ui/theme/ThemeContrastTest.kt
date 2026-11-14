package com.swayconnect.android.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.atan2
import kotlin.math.cbrt
import kotlin.math.pow

/** Every pair of docs/UX.md 3.1 and 15, computed from the token values (WCAG 2.x relative luminance). */
class ThemeContrastTest {
    private class Theme(val name: String, val scheme: ColorScheme, val tokens: SwayTokens)

    private val light = Theme("light", swayLight(), LightSwayTokens)
    private val dark = Theme("dark", swayDark(), DarkSwayTokens)
    private val failures = mutableListOf<String>()

    private fun linear(channel: Float): Double =
        if (channel <= 0.04045f) channel / 12.92 else ((channel + 0.055) / 1.055).pow(2.4)

    private fun luminance(c: Color): Double = 0.2126 * linear(c.red) + 0.7152 * linear(c.green) + 0.0722 * linear(c.blue)

    private fun ratio(a: Color, b: Color): Double {
        val (hi, lo) = listOf(luminance(a), luminance(b)).sortedDescending()
        return (hi + 0.05) / (lo + 0.05)
    }

    private fun check(theme: Theme, pair: String, fg: Color, bg: Color, min: Double) {
        val r = ratio(fg, bg)
        if (r < min) failures += "${theme.name}: $pair is %.2f:1, needs %.1f:1".format(r, min)
    }

    private fun text(theme: Theme, pair: String, fg: Color, bg: Color) = check(theme, pair, fg, bg, TEXT)

    private fun mark(theme: Theme, pair: String, fg: Color, bg: Color) = check(theme, pair, fg, bg, MARK)

    private fun textPairs(t: Theme) = with(t.scheme) {
        text(t, "onBackground on background", onBackground, background)
        text(t, "onSurface on surface", onSurface, surface)
        text(t, "onSurface on surfaceContainer", onSurface, surfaceContainer)
        text(t, "onSurface on surfaceContainerHigh", onSurface, surfaceContainerHigh)
        text(t, "onSurfaceVariant on background", onSurfaceVariant, background)
        text(t, "onSurfaceVariant on surface", onSurfaceVariant, surface)
        text(t, "onSurfaceVariant on surfaceContainer", onSurfaceVariant, surfaceContainer)
        text(t, "onSurfaceVariant on surfaceContainerHigh", onSurfaceVariant, surfaceContainerHigh)
        text(t, "onPrimary on primary", onPrimary, primary)
        text(t, "accentOnInk on primary", t.tokens.accentOnInk, primary)
        text(t, "onSecondaryContainer on secondaryContainer", onSecondaryContainer, secondaryContainer)
        text(t, "onErrorContainer on errorContainer", onErrorContainer, errorContainer)
        text(t, "onError on error", onError, error)
        text(t, "error text on surface", error, surface)
        text(t, "error text on background", error, background)
        text(t, "onSurface on okContainer", onSurface, t.tokens.okContainer)
        text(t, "navOnSelected on navSelected", t.tokens.navOnSelected, t.tokens.navSelected)
    }

    private fun markPairs(t: Theme) = with(t.scheme) {
        mark(t, "outline on surface", outline, surface)
        mark(t, "outline on background", outline, background)
        mark(t, "ring (primary) on background", primary, background)
        mark(t, "urgent dot (error) on surface", error, surface)
        mark(t, "urgent dot (error) on background", error, background)
        // The urgent dot on a focused (ink) card sits in a `surface` keyline: see the test below.
        mark(t, "urgent keyline (surface) on primary", surface, primary)
        mark(t, "ok dot on okContainer", t.tokens.ok, t.tokens.okContainer)
        mark(t, "ok dot on surface", t.tokens.ok, surface)
        mark(t, "navIcon on navContainer", t.tokens.navIcon, t.tokens.navContainer)
        mark(t, "navSelected on navContainer", t.tokens.navSelected, t.tokens.navContainer)
        mark(t, "nav stroke (outline) on background", outline, background)
    }

    @Test
    fun `every text pair reaches 4_5 to 1 and every mark 3 to 1 in both themes`() {
        for (t in listOf(light, dark)) {
            textPairs(t)
            markPairs(t)
        }
        // Lime is an accent on dark surfaces only (UX 3.1).
        text(dark, "tertiary on background", dark.scheme.tertiary, dark.scheme.background)
        text(dark, "tertiary on surface", dark.scheme.tertiary, dark.scheme.surface)
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun `map washes keep their text contrast`() {
        light.tokens.washes.forEach { check(light, "ink on wash $it", light.scheme.onSurface, it, LIGHT_WASH) }
        dark.tokens.washes.forEach { check(dark, "text on wash $it", dark.scheme.onSurface, it, DARK_WASH) }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun `every wash keeps 7 to 1 with onSurface in both themes`() {
        for (t in listOf(light, dark)) {
            t.tokens.washes.forEach { check(t, "onSurface on wash $it", t.scheme.onSurface, it, AAA) }
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun `washes share one lightness and spread by hue`() {
        for (t in listOf(light, dark)) {
            val labs = t.tokens.washes.map(::lab)
            val lightness = labs.map { it[0] }
            assertTrue("${t.name} L* spread $lightness", lightness.max() - lightness.min() <= MAX_L_SPREAD)
            val hues = labs.map { Math.toDegrees(atan2(it[2], it[1])).mod(360.0) }.sorted()
            val gaps = hues.zipWithNext { a, b -> b - a } + (hues.first() + 360 - hues.last())
            assertTrue("${t.name} hue gaps $gaps", gaps.all { it >= MIN_HUE_GAP })
        }
    }

    /** CIELAB (D65) of an sRGB colour: [L*, a*, b*]. */
    private fun lab(c: Color): List<Double> {
        val (r, g, b) = listOf(c.red, c.green, c.blue).map(::linear)
        val x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047
        val y = 0.2126 * r + 0.7152 * g + 0.0722 * b
        val z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883
        fun f(v: Double) = if (v > 0.008856) cbrt(v) else 7.787 * v + 16.0 / 116
        return listOf(116 * f(y) - 16, 500 * (f(x) - f(y)), 200 * (f(y) - f(z)))
    }

    @Test
    fun `the values the UX spec quotes still hold`() {
        assertEquals(13.7, ratio(LightSwayTokens.accentOnInk, swayLight().primary), 0.05)
        assertEquals(7.2, ratio(DarkSwayTokens.accentOnInk, swayDark().primary), 0.05)
        assertEquals(6.1, ratio(swayLight().error, swayLight().surface), 0.05)
        assertEquals(5.1, ratio(swayLight().error, swayLight().background), 0.05)
        assertEquals(3.05, ratio(swayLight().outline, swayLight().background), 0.01)
        assertEquals(3.75, ratio(swayDark().outline, swayDark().surface), 0.01)
        assertEquals(4.2, ratio(swayDark().outline, swayDark().background), 0.05)
    }

    @Test
    fun `lime never reaches a light surface and coral never touches a dark ink fill directly`() {
        // Why accentOnInk exists (UX 3.1), and why the urgent dot needs a keyline on a focused dark card.
        assertTrue(ratio(LightPalette.Lime, swayLight().surface) < MARK)
        assertTrue(ratio(LightPalette.Lime, swayLight().background) < MARK)
        assertTrue(ratio(swayDark().error, swayDark().primary) < MARK)
    }

    private companion object {
        const val TEXT = 4.5
        const val MARK = 3.0
        const val LIGHT_WASH = 12.0
        const val DARK_WASH = 10.0
        const val AAA = 7.0
        const val MAX_L_SPREAD = 2.0
        const val MIN_HUE_GAP = 35.0
    }
}
