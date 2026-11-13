package com.swayconnect.android.ui.theme

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Shapes of docs/UX.md 3.2. Pills use CircleShape at the call site.
val SwayShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(20.dp),
    large = RoundedCornerShape(24.dp),
    extraLarge = RoundedCornerShape(28.dp),
)

private const val TABULAR = "tnum"

private fun style(size: Int, lineHeight: Int, weight: FontWeight, tabular: Boolean = false) = TextStyle(
    fontFamily = FontFamily.Default,
    fontSize = size.sp,
    lineHeight = lineHeight.sp,
    fontWeight = weight,
    fontFeatureSettings = if (tabular) TABULAR else null,
)

// Type of docs/UX.md 3.3: system font, no bundled font. Styles the table does not name keep the M3 defaults.
val SwayType = Typography(
    headlineLarge = style(32, 40, FontWeight.Bold),
    titleLarge = style(26, 32, FontWeight.Bold, tabular = true),
    titleMedium = style(16, 24, FontWeight.SemiBold),
    bodyMedium = style(14, 20, FontWeight.Normal),
    labelLarge = style(14, 20, FontWeight.Medium),
    labelMedium = style(12, 16, FontWeight.Medium),
    labelSmall = style(11, 16, FontWeight.Medium),
)

/** Tabular figures for any style that shows a changing number (counts, percentages, countdowns). */
fun TextStyle.tabular(): TextStyle = copy(fontFeatureSettings = TABULAR)
