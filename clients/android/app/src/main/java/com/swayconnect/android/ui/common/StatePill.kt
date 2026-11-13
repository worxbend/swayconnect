package com.swayconnect.android.ui.common

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.ui.theme.LocalSwayTokens

/** The fills of docs/UX.md 3.1 and 5.2. Each pill always carries a word; the colour never stands alone. */
enum class PillTone {
    /** Ink fill: "Focused", filled pills. */
    Ink,

    /** Coral fill with an ink dot: "Urgent". */
    Coral,

    /** Tinted fill: "Floating", "Tabbed · 2 of 3". */
    Tinted,

    /** Green dot on its light fill: "Ready", "Default". */
    Ok,

    /** Grey dot on a plain container: "Connecting", "Offline". */
    Neutral,
}

private class PillColors(val container: Color, val content: Color, val dot: Color?)

@Composable
private fun colorsOf(tone: PillTone): PillColors {
    val scheme = MaterialTheme.colorScheme
    val tokens = LocalSwayTokens.current
    return when (tone) {
        PillTone.Ink -> PillColors(scheme.primary, scheme.onPrimary, dot = null)
        PillTone.Coral -> PillColors(scheme.errorContainer, scheme.onErrorContainer, dot = scheme.onErrorContainer)
        PillTone.Tinted -> PillColors(scheme.secondaryContainer, scheme.onSecondaryContainer, dot = null)
        PillTone.Ok -> PillColors(tokens.okContainer, scheme.onSurface, dot = tokens.ok)
        PillTone.Neutral -> PillColors(scheme.surfaceContainer, scheme.onSurface, dot = scheme.onSurfaceVariant)
    }
}

/**
 * A status pill: a label, not a control. It grows with the font scale; a cut label keeps its full text in
 * semantics. [icon] replaces the dot of tones that have one.
 */
@Composable
fun StatePill(
    text: String,
    tone: PillTone,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    textStyle: TextStyle = MaterialTheme.typography.labelLarge,
    /** What TalkBack says instead of [text], when the short word needs more ("focused on computer"). */
    spokenText: String? = null,
) {
    val colors = colorsOf(tone)
    val spoken = if (spokenText != null) Modifier.clearAndSetSemantics { contentDescription = spokenText } else Modifier
    Row(
        modifier = modifier
            .then(spoken)
            .heightIn(min = 28.dp)
            .background(colors.container, CircleShape)
            .padding(horizontal = 12.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            icon != null -> Icon(imageVector = icon, contentDescription = null, tint = colors.content, modifier = Modifier.size(16.dp))
            colors.dot != null -> Box(Modifier.size(8.dp).background(colors.dot, CircleShape))
        }
        Text(text = text, color = colors.content, style = textStyle, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}
