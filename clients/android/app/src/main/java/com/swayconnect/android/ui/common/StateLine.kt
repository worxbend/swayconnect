package com.swayconnect.android.ui.common

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics

/**
 * One muted line of an area (docs/UX.md 16): waiting, a cut list, read-only, or the last refusal. A refusal
 * ([announce]) is a polite live region, so TalkBack hears what the `Reject` haptic means (UX 14, 15).
 */
@Composable
fun StateLine(text: String, modifier: Modifier = Modifier, announce: Boolean = false) {
    Text(
        text = text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = if (announce) modifier.semantics { liveRegion = LiveRegionMode.Polite } else modifier,
    )
}
