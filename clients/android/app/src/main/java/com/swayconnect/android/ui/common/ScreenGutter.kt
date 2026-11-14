package com.swayconnect.android.ui.common

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.max

private val SCREEN_SIDE = 16.dp

/**
 * The side gutter every screen shares, inside the safe area (UX 5.5): 16dp, or the part of the back-gesture zone
 * the safe area leaves inside the content. [navAtStart]: the nav stands past that zone at the start (landscape),
 * so the start side keeps 16dp.
 */
@Composable
fun screenGutter(navAtStart: Boolean): PaddingValues {
    val direction = LocalLayoutDirection.current
    val safe = WindowInsets.safeDrawing.asPaddingValues()
    val gestures = WindowInsets.systemGestures.asPaddingValues()
    val start = if (navAtStart) SCREEN_SIDE else max(SCREEN_SIDE, gestures.calculateStartPadding(direction) - safe.calculateStartPadding(direction))
    val end = max(SCREEN_SIDE, gestures.calculateEndPadding(direction) - safe.calculateEndPadding(direction))
    return PaddingValues(start = start, end = end)
}
