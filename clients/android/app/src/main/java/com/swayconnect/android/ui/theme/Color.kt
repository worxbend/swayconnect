package com.swayconnect.android.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

// Colour tokens of docs/UX.md 3.1. Light follows reference 2, dark follows reference 1.
// Every role is set by hand so no baseline purple leaks into a component. Dynamic colour is off.

internal object LightPalette {
    val Greige = Color(0xFFEDEAE4)
    val White = Color(0xFFFFFFFF)
    val Badge = Color(0xFFF4F2EE)
    val Pressed = Color(0xFFE4E0D8)
    val Ink = Color(0xFF141414)
    val InkVariant = Color(0xFF66625A)
    val Lime = Color(0xFFE4E46C)
    val Coral = Color(0xFFA8402A)
    val CoralFill = Color(0xFFE8836A)
    val Ok = Color(0xFF3B6E2E)
    val OkContainer = Color(0xFFDCEBD3)
    val Outline = Color(0xFF8A857C)
    val OutlineVariant = Color(0xFFD3CEC4)

    // Eight hues 45 degrees apart at CIELAB L* 93, chroma 10, each about 15:1 with Ink (UX 3.1).
    val Washes = listOf(
        Color(0xFFFFE5E5), Color(0xFFFBE8DA), Color(0xFFECECD9), Color(0xFFDCF0E2),
        Color(0xFFD3F1F1), Color(0xFFD9EEFC), Color(0xFFE9EAFD), Color(0xFFF9E5F3),
    )
}

internal object DarkPalette {
    val Background = Color(0xFF0E0E12)
    val Card = Color(0xFF1B1B23)
    val Container = Color(0xFF24242E)
    val ContainerHigh = Color(0xFF2E2E3A)
    val Text = Color(0xFFF2F2F5)
    val TextVariant = Color(0xFFA3A3AE)
    val White = Color(0xFFFFFFFF)
    val InkText = Color(0xFF14141C)
    val Ink = Color(0xFF141414)
    val Lime = Color(0xFFE4E46C)
    val Olive = Color(0xFF5C5A00)
    val Coral = Color(0xFFFF9A80)
    val Ok = Color(0xFF8DBF7A)

    // A dark green at about the L* of ContainerHigh, so "Default" keeps the green of light mode (UX 3.1).
    val OkContainer = Color(0xFF243A2A)
    val Outline = Color(0xFF75757F)
    val OutlineVariant = Color(0xFF3A3A46)
    val NavContainer = Color(0xFF1B1B26)
    val NavIcon = Color(0xFF8E8E99)

    // The same eight hues at CIELAB L* 22, chroma 11, each about 11:1 with Text (UX 3.1).
    val Washes = listOf(
        Color(0xFF452F2F), Color(0xFF413226), Color(0xFF353625), Color(0xFF27392E),
        Color(0xFF1D393A), Color(0xFF223743), Color(0xFF323444), Color(0xFF40303C),
    )
}

/** Custom tokens that Material 3 has no role for (UX 3.1). Read through [LocalSwayTokens]. */
@Immutable
data class SwayTokens(
    /** Accent text on an ink ([ColorScheme.primary]) fill. Never on a light surface. */
    val accentOnInk: Color,
    /** "Ready" and "Default" dots. */
    val ok: Color,
    /** Fill behind an [ok] dot. */
    val okContainer: Color,
    val navContainer: Color,
    val navSelected: Color,
    val navOnSelected: Color,
    val navIcon: Color,
    /** Eight map tile washes; pick one with [washFor]. */
    val washes: List<Color>,
) {
    init {
        require(washes.size == WASH_COUNT) { "need $WASH_COUNT washes" }
    }

    /** The wash of an app: FNV-1a 32 bit of the UTF-8 `app_id`, modulo 8 (UX 3.1). */
    fun washFor(appId: String): Color = washes[washIndex(appId)]

    companion object {
        const val WASH_COUNT = 8

        fun washIndex(appId: String): Int = (fnv1a32(appId.encodeToByteArray()) % WASH_COUNT.toUInt()).toInt()

        private fun fnv1a32(bytes: ByteArray): UInt {
            var hash = 0x811C9DC5u
            for (byte in bytes) {
                hash = (hash xor byte.toUByte().toUInt()) * 0x01000193u
            }
            return hash
        }
    }
}

val LightSwayTokens = SwayTokens(
    accentOnInk = LightPalette.Lime,
    ok = LightPalette.Ok,
    okContainer = LightPalette.OkContainer,
    navContainer = LightPalette.White,
    navSelected = LightPalette.Ink,
    navOnSelected = LightPalette.White,
    navIcon = LightPalette.InkVariant,
    washes = LightPalette.Washes,
)

val DarkSwayTokens = SwayTokens(
    accentOnInk = DarkPalette.Olive,
    ok = DarkPalette.Ok,
    okContainer = DarkPalette.OkContainer,
    navContainer = DarkPalette.NavContainer,
    navSelected = DarkPalette.White,
    navOnSelected = DarkPalette.InkText,
    navIcon = DarkPalette.NavIcon,
    washes = DarkPalette.Washes,
)

val LocalSwayTokens = staticCompositionLocalOf { LightSwayTokens }

fun swayLight(): ColorScheme = with(LightPalette) {
    lightColorScheme(
        primary = Ink,
        onPrimary = White,
        primaryContainer = Pressed,
        onPrimaryContainer = Ink,
        inversePrimary = Lime,
        secondary = InkVariant,
        onSecondary = White,
        secondaryContainer = Pressed,
        onSecondaryContainer = Ink,
        tertiary = Lime,
        onTertiary = Ink,
        tertiaryContainer = Lime,
        onTertiaryContainer = Ink,
        background = Greige,
        onBackground = Ink,
        surface = White,
        onSurface = Ink,
        surfaceVariant = Badge,
        onSurfaceVariant = InkVariant,
        // Tonal elevation would tint white cards grey; the theme uses flat surfaces.
        surfaceTint = White,
        inverseSurface = Ink,
        inverseOnSurface = White,
        // White text, not ink: ink on #A8402A is 3.0:1 (spec note, see ThemeContrastTest).
        error = Coral,
        onError = White,
        errorContainer = CoralFill,
        onErrorContainer = Ink,
        outline = Outline,
        outlineVariant = OutlineVariant,
        scrim = Color.Black,
        surfaceBright = White,
        surfaceDim = Pressed,
        surfaceContainerLowest = White,
        surfaceContainerLow = White,
        surfaceContainer = Badge,
        surfaceContainerHigh = Pressed,
        surfaceContainerHighest = Pressed,
    )
}

fun swayDark(): ColorScheme = with(DarkPalette) {
    darkColorScheme(
        primary = White,
        onPrimary = InkText,
        primaryContainer = ContainerHigh,
        onPrimaryContainer = Text,
        inversePrimary = Ink,
        secondary = TextVariant,
        onSecondary = InkText,
        secondaryContainer = ContainerHigh,
        onSecondaryContainer = Text,
        tertiary = Lime,
        onTertiary = Ink,
        tertiaryContainer = Olive,
        onTertiaryContainer = Text,
        background = Background,
        onBackground = Text,
        surface = Card,
        onSurface = Text,
        surfaceVariant = Container,
        onSurfaceVariant = TextVariant,
        surfaceTint = Card,
        inverseSurface = Text,
        inverseOnSurface = InkText,
        error = Coral,
        onError = Ink,
        errorContainer = Coral,
        onErrorContainer = Ink,
        outline = Outline,
        outlineVariant = OutlineVariant,
        scrim = Color.Black,
        surfaceBright = ContainerHigh,
        surfaceDim = Background,
        surfaceContainerLowest = Card,
        surfaceContainerLow = Card,
        surfaceContainer = Container,
        surfaceContainerHigh = ContainerHigh,
        surfaceContainerHighest = ContainerHigh,
    )
}
