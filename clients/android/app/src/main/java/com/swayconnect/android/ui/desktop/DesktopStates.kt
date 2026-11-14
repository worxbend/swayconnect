package com.swayconnect.android.ui.desktop

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.common.screenGutter
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import kotlinx.coroutines.delay

private const val SKELETON_LOW = 0.45f
private const val SKELETON_PULSE_MS = 900
private const val DEFAULT_ASPECT = 16f / 9f

/**
 * The connection line over a stale desktop (UX 5.9): "Connecting" or "Offline · last seen 12 s ago" (ticks each
 * second for a minute, then each minute; no live region), the reason of the last failure, and "Retry now"
 * while the app waits between attempts. Nothing while the desktop is live.
 */
@Composable
internal fun StaleBanner(state: UiState, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    if (state.desktopLive) return
    val connecting = state.connection == ConnectionStatus.Connecting || state.connection == ConnectionStatus.Authenticating
    val seen = state.lastSeenAtMs
    val text = when {
        connecting -> stringResource(R.string.desktop_stale_connecting)
        seen != null -> lastSeenText(seen)
        else -> null
    }
    val retry = state.connection == ConnectionStatus.Backoff
    if (text == null && state.connectionProblem == null && !retry) return
    FlowRow(
        modifier = modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        text?.let { StateLine(it, Modifier.padding(vertical = 12.dp)) }
        if (retry) {
            FilledTonalButton(
                onClick = onRetry,
                shape = CircleShape,
                colors = ButtonDefaults.filledTonalButtonColors(containerColor = MaterialTheme.colorScheme.surface, contentColor = MaterialTheme.colorScheme.onSurface),
                modifier = Modifier.heightIn(min = 48.dp),
            ) {
                Text(stringResource(R.string.desktop_retry))
            }
        }
        state.connectionProblem?.let { StateLine(it) }
    }
}

@Composable
private fun lastSeenText(seenAtMs: Long): String {
    val seen by produceState(LastSeen.of(System.currentTimeMillis(), seenAtMs), seenAtMs) {
        while (true) {
            value = LastSeen.of(System.currentTimeMillis(), seenAtMs)
            delay(value.nextTickMs)
        }
    }
    return if (seen.inMinutes) {
        stringResource(R.string.desktop_seen_minutes, seen.amount)
    } else {
        stringResource(R.string.desktop_seen_seconds, seen.amount)
    }
}

/** First connect without a cache (UX 5.9): three skeleton cards and a slab with two tiles. No pulse under reduce motion. */
@Composable
internal fun SkeletonDesktop(state: UiState, padding: PaddingValues, onRetry: () -> Unit) {
    val reduceMotion = LocalReduceMotion.current
    val pulse = if (reduceMotion) {
        1f
    } else {
        val transition = rememberInfiniteTransition(label = "skeleton")
        val value by transition.animateFloat(1f, SKELETON_LOW, infiniteRepeatable(tween(SKELETON_PULSE_MS), RepeatMode.Reverse), label = "pulse")
        value
    }
    val loading = stringResource(R.string.desktop_loading)
    val block = MaterialTheme.colorScheme.surface
    val tile = MaterialTheme.colorScheme.surfaceContainer
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(padding)) {
        StaleBanner(state, onRetry)
        Column(
            modifier = Modifier.graphicsLayer { alpha = pulse }.clearAndSetSemantics { contentDescription = loading },
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                repeat(3) { Box(Modifier.size(width = 72.dp, height = 88.dp).background(block, MaterialTheme.shapes.large)) }
            }
            Box(Modifier.fillMaxWidth().aspectRatio(DEFAULT_ASPECT).background(block, MaterialTheme.shapes.large).padding(8.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Box(Modifier.weight(3f).fillMaxHeight().background(tile, MaterialTheme.shapes.extraSmall))
                    Box(Modifier.weight(2f).fillMaxHeight().background(tile, MaterialTheme.shapes.extraSmall))
                }
            }
        }
    }
}

/**
 * No page to show. Sway offline (UX 5.9): an empty slab with the last output's aspect and the line "Sway is not
 * running on worxbox. The phone reconnects when it starts." Otherwise a plain line.
 */
@Composable
internal fun NoWorkspaces(state: UiState, desktop: Desktop, facts: PageFacts, padding: PaddingValues, onRetry: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(padding),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        StaleBanner(state, onRetry)
        facts.lines.forEach { StateLine(it) }
        if (desktop.sway.online) {
            StateLine(stringResource(R.string.desktop_no_workspaces))
        } else {
            val output = desktop.outputs.orEmpty().firstOrNull { it.active && it.rect.w > 0 && it.rect.h > 0 }
            val aspect = output?.let { it.rect.w.toFloat() / it.rect.h } ?: DEFAULT_ASPECT
            val name = state.server?.name ?: stringResource(R.string.desktop_unpaired_unknown)
            DashedCard(text = stringResource(R.string.desktop_sway_offline, name), modifier = Modifier.aspectRatio(aspect))
        }
    }
}

/**
 * UX 16 "Revoked" and "Wrong fingerprint", full screen: no nav, no Drive pill, no area of a computer this phone
 * can no longer trust. [wrongKey] false means revoked.
 */
@Composable
fun TrustLostPage(wrongKey: Boolean, serverName: String?, onPairAgain: () -> Unit) {
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(screenGutter(navAtStart = false))
                .padding(vertical = 48.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            TrustLostCard(wrongKey, serverName)
            Button(onClick = onPairAgain, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.desktop_pair_again)) }
        }
    }
}

/** The message of [TrustLostPage] on a white card, under a warning badge. */
@Composable
private fun TrustLostCard(wrongKey: Boolean, serverName: String?) {
    val scheme = MaterialTheme.colorScheme
    Surface(shape = MaterialTheme.shapes.medium, color = scheme.surface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.size(48.dp).background(scheme.error.copy(alpha = WARNING_BADGE_ALPHA), CircleShape), contentAlignment = Alignment.Center) {
                Icon(SwayIcons.Warning.filled, contentDescription = null, tint = scheme.error, modifier = Modifier.size(28.dp))
            }
            val title = if (wrongKey) {
                stringResource(R.string.desktop_wrong_key)
            } else {
                stringResource(R.string.desktop_unpaired, serverName ?: stringResource(R.string.desktop_unpaired_unknown))
            }
            Text(title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
            if (wrongKey) StateLine(stringResource(R.string.desktop_wrong_key_detail))
        }
    }
}

private const val WARNING_BADGE_ALPHA = 0.12f
