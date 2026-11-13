package com.swayconnect.android.ui.theme

import android.provider.Settings
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext

/** The theme setting (docs/UX.md 17.1). No "use wallpaper colours" option in v2. */
enum class ThemeMode { System, Light, Dark }

/**
 * The app theme: hand-set colour roles, [SwayShapes] and [SwayType], plus [LocalSwayTokens], [LocalSwayMotion],
 * [LocalReduceMotion] and [LocalHaptics]. The settings come from the caller; defaults follow the system.
 */
@Composable
fun SwayConnectTheme(
    mode: ThemeMode = ThemeMode.System,
    reduceMotionSetting: Boolean = false,
    hapticLevel: HapticLevel = HapticLevel.Full,
    content: @Composable () -> Unit,
) {
    val dark = when (mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Light -> false
        ThemeMode.Dark -> true
    }
    val reduceMotion = reduceMotionSetting || systemAnimationsOff()
    CompositionLocalProvider(
        LocalSwayTokens provides if (dark) DarkSwayTokens else LightSwayTokens,
        LocalReduceMotion provides reduceMotion,
        LocalSwayMotion provides if (reduceMotion) SwayMotionReduced else SwayMotion,
        LocalHaptics provides rememberHaptics(hapticLevel),
    ) {
        MaterialTheme(
            colorScheme = if (dark) swayDark() else swayLight(),
            shapes = SwayShapes,
            typography = SwayType,
            content = content,
        )
    }
}

/** Animator duration scale 0 ("Remove animations"). Read once per composition of the theme. */
@Composable
private fun systemAnimationsOff(): Boolean {
    val resolver = LocalContext.current.contentResolver
    return remember(resolver) {
        Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
}
