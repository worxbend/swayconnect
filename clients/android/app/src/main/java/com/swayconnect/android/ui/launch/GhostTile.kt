package com.swayconnect.android.ui.launch

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.state.PendingLaunch
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.protocol.replaceUnsafe

/** Half of the 1 Hz pulse of UX 8.2. */
private const val PULSE_HALF_MS = 500
private const val PULSE_LOW = 0.35f

/** A ghost that becomes visible later than this after the `ok` is a page coming back, not the server's answer. */
private const val FRESH_MS = 1_000L

/**
 * The ghost tile of an app the computer started that has no window yet (docs/UX.md 8.2): "Opening Firefox…" with
 * its icon pulsing at 1 Hz (no pulse under reduce motion, UX 13). The state ends it when a new window appears on
 * the target workspace, or after 10 s with "Firefox did not open a window yet.". The `Confirm` haptic of the
 * server's `ok` (UX 14) fires here: the `ok` switches to Desktop, so this tile is the first thing that sees it.
 */
@Composable
fun GhostTile(launch: PendingLaunch, modifier: Modifier = Modifier) {
    val name = replaceUnsafe(launch.name)
    val haptics = LocalHaptics.current
    LaunchedEffect(launch.startedAtMs) {
        if (System.nanoTime() / 1_000_000 - launch.startedAtMs < FRESH_MS) haptics.perform(SwayHaptic.Confirm)
    }
    // Read in the draw phase: the pulse redraws the icon without recomposing the tile.
    val pulse = if (LocalReduceMotion.current) {
        null
    } else {
        rememberInfiniteTransition(label = "ghost").animateFloat(
            initialValue = 1f,
            targetValue = PULSE_LOW,
            animationSpec = infiniteRepeatable(tween(PULSE_HALF_MS), RepeatMode.Reverse),
            label = "ghost-pulse",
        )
    }
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        modifier = modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
    ) {
        Row(
            modifier = Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AppIcon(appId = name, iconHash = launch.icon, modifier = Modifier.graphicsLayer { alpha = pulse?.value ?: 1f })
            Text(stringResource(R.string.launch_opening, name), style = MaterialTheme.typography.bodyMedium)
        }
    }
}
