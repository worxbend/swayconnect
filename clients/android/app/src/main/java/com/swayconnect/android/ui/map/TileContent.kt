package com.swayconnect.android.ui.map

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.tabular
import com.swayconnect.protocol.Window

private const val TITLE_FRAGMENT_CHARS = 12
private const val PULSE_LOW = 0.4f
private const val PULSE_STEP_MS = 150
private const val PULSE_CYCLES = 2
private val DOT_SIZE = 14.dp
private val DOT_KEYLINE = 2.dp
private val SPINNER_SIZE = 16.dp

/** What one tile shows besides its rect. */
internal data class TileLook(
    val tile: PlacedTile,
    val window: Window,
    val focused: Boolean,
    val pending: Boolean,
    val sameAppIndex: Int?,
    /** False on the map's first frame: a mark that is already there does not pulse. */
    val pulseNewMarks: Boolean,
)

/**
 * The look of one map tile (UX 6.2 to 6.7), filling the rect it is measured to. A focused tile is see-through:
 * the shared focus node under it carries the ink fill and slides between tiles.
 */
@Composable
internal fun TileBody(look: TileLook, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val tile = look.tile
    val shape = MaterialTheme.shapes.extraSmall
    val floating = tile.layer != TileLayer.Tiled
    val fill = when {
        look.focused -> Color.Transparent
        floating -> scheme.surface
        else -> LocalSwayTokens.current.washFor(look.window.appId.orEmpty())
    }
    val dark = scheme.surface.luminance() < 0.5f
    val border = when {
        look.window.urgent -> BorderStroke(2.dp, scheme.error)
        floating && dark -> BorderStroke(1.dp, scheme.onSurfaceVariant)
        else -> BorderStroke(1.dp, scheme.outline)
    }
    if (tile.sizeClass == SizeClass.Sliver) {
        Sliver(look, modifier)
        return
    }
    val lifted = if (floating && !dark && !look.focused) Modifier.shadow(6.dp, shape) else Modifier
    val content = if (look.focused) scheme.onPrimary else scheme.onSurface
    Box(modifier.then(lifted).background(fill, shape).border(border, shape)) {
        CompositionLocalProvider(LocalContentColor provides content) {
            when (tile.sizeClass) {
                SizeClass.Large -> LargeContent(look)
                SizeClass.Medium -> MediumContent(look)
                SizeClass.Small -> SmallContent(look)
                SizeClass.Sliver -> Unit
            }
        }
        val large = tile.sizeClass == SizeClass.Large || tile.sizeClass == SizeClass.Medium
        if (look.pending && large) {
            CircularProgressIndicator(Modifier.align(Alignment.TopEnd).padding(4.dp).size(SPINNER_SIZE), color = content, strokeWidth = 2.dp)
        } else if (look.window.urgent && large) {
            UrgentDot(look.pulseNewMarks, Modifier.align(Alignment.TopEnd).padding(2.dp))
        }
        if (tile.layer == TileLayer.Fullscreen && large) {
            StatePill(stringResource(R.string.window_mark_fullscreen), PillTone.Tinted, Modifier.align(Alignment.BottomStart).padding(8.dp))
        }
    }
}

/**
 * A window under 24dp (UX 6.2): a plain wash bar, no border, no shadow, at most 2dp round. A bordered, shadowed
 * card that thin read as a scrollbar between its neighbours. The ink of focus still shows on it.
 */
@Composable
private fun Sliver(look: TileLook, modifier: Modifier) {
    val rect = look.tile.rect
    val radius = minOf(2f, rect.width / 2, rect.height / 2).dp
    val fill = when {
        look.focused -> MaterialTheme.colorScheme.primary
        look.window.urgent -> MaterialTheme.colorScheme.error
        else -> LocalSwayTokens.current.washFor(look.window.appId.orEmpty())
    }
    Box(modifier.background(fill, RoundedCornerShape(radius)))
}

@Composable
private fun LargeContent(look: TileLook) {
    val window = look.window
    Row(Modifier.fillMaxSize().padding(8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        IconOnDisc(window, look.focused, iconSize = 24.dp)
        Column(Modifier.weight(1f)) {
            Text(
                text = window.title ?: window.appId ?: stringResource(R.string.window_untitled),
                style = MaterialTheme.typography.labelLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                // Measured after the app id line: at a large font scale the title gives way, not the app id.
                modifier = Modifier.weight(1f, fill = false),
            )
            val appLine = listOfNotNull(
                window.appId.takeIf { window.title != null },
                look.sameAppIndex?.let { stringResource(R.string.window_same_app_index, it) },
            ).joinToString(" ")
            if (appLine.isNotEmpty()) {
                Text(text = appLine, style = MaterialTheme.typography.labelSmall.tabular(), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun MediumContent(look: TileLook) {
    val fragment = look.window.title?.take(TITLE_FRAGMENT_CHARS)?.takeIf { look.tile.showsTitleFragment && look.sameAppIndex != null }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.align(Alignment.Center).padding(4.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            IconOnDisc(look.window, look.focused, iconSize = 28.dp)
            if (fragment != null) {
                // The index follows the fragment on its line: a corner badge would overlap it at a large font scale.
                val line = fragment + " " + stringResource(R.string.window_same_app_index, look.sameAppIndex ?: 0)
                Text(text = line, style = MaterialTheme.typography.labelSmall.tabular(), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        val index = look.sameAppIndex
        if (index == null || fragment != null) return@Box
        val badge = stringResource(R.string.window_same_app_index, index)
        val style = MaterialTheme.typography.labelSmall.tabular()
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        val badgeWidth = remember(badge, style, density) { measurer.measure(badge, style, maxLines = 1).size.width / density.density }
        if (MapGeometry.indexBadgeFits(look.tile.rect.width, badgeWidth)) {
            Text(text = badge, style = style, modifier = Modifier.align(Alignment.BottomEnd).padding(horizontal = 6.dp, vertical = 2.dp))
        }
    }
}

@Composable
private fun SmallContent(look: TileLook) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        if (look.pending) {
            CircularProgressIndicator(Modifier.size(SPINNER_SIZE), color = LocalContentColor.current, strokeWidth = 2.dp)
        } else {
            AppIcon(appId = look.window.appId, iconHash = look.window.icon, size = 16.dp)
        }
    }
}

/**
 * UX 6.4: on a focused tile the icon sits on a 32dp `surface` disc, so it reads on the ink fill. A monogram (no
 * icon yet) always gets that `surface` disc: on its own wash it was a stray letter on a tile of the same wash.
 */
@Composable
private fun IconOnDisc(window: Window, focused: Boolean, iconSize: Dp) {
    val surface = MaterialTheme.colorScheme.surface
    val disc = if (focused) Modifier.background(surface, CircleShape) else Modifier
    Box(Modifier.size(maxOf(iconSize, 32.dp)).then(disc), contentAlignment = Alignment.Center) {
        AppIcon(appId = window.appId, iconHash = window.icon, size = iconSize, monogramDisc = surface)
    }
}

/**
 * The 14dp `error` dot with "!" inside a 2dp `surface` keyline (A1: dark `error` on the ink fill is under 3:1).
 * With [pulse] it pulses twice as it appears (UX 5.4, 6.4), unless reduce motion is on.
 */
@Composable
private fun UrgentDot(pulse: Boolean, modifier: Modifier) {
    val reduceMotion = LocalReduceMotion.current
    val alpha = remember { Animatable(1f) }
    LaunchedEffect(Unit) {
        if (!pulse || reduceMotion) return@LaunchedEffect
        repeat(PULSE_CYCLES) {
            alpha.animateTo(PULSE_LOW, tween(PULSE_STEP_MS))
            alpha.animateTo(1f, tween(PULSE_STEP_MS))
        }
    }
    val scheme = MaterialTheme.colorScheme
    val glyphSize = with(LocalDensity.current) { 10.dp.toSp() }
    Box(
        modifier
            .size(DOT_SIZE + DOT_KEYLINE * 2)
            .background(scheme.surface, CircleShape)
            .padding(DOT_KEYLINE)
            .graphicsLayer { this.alpha = alpha.value }
            .background(scheme.error, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        // The line height must match the glyph size, or the inherited 16sp line pushes the "!" out of the dot.
        Text(
            text = "!",
            color = scheme.onError,
            style = MaterialTheme.typography.labelSmall.copy(fontSize = glyphSize, lineHeight = glyphSize, fontWeight = FontWeight.Bold),
            maxLines = 1,
        )
    }
}

/** The header strip of a tabbed or stacked group (UX 6.5). [segment] adds the touch and semantics of segment i. */
@Composable
internal fun GroupStrip(group: PlacedGroup, windows: Map<Long, Window>, modifier: Modifier, segment: (Int) -> Modifier) {
    val scheme = MaterialTheme.colorScheme
    val shape = MaterialTheme.shapes.extraSmall
    Box(modifier.background(scheme.surfaceContainerHigh, shape).border(1.dp, scheme.outline, shape)) {
        when (group.kind) {
            GroupKind.Tabbed -> Row(Modifier.fillMaxSize()) {
                group.tabs.forEachIndexed { index, ids ->
                    Box(Modifier.weight(1f).fillMaxHeight().then(segment(index)), contentAlignment = Alignment.Center) {
                        Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                            ids.take(2).forEach { id -> windows[id]?.let { AppIcon(it.appId, it.icon, size = 14.dp) } }
                        }
                        if (index == group.shown) {
                            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(2.dp).background(scheme.primary))
                        }
                    }
                }
            }
            GroupKind.Stacked -> Column(Modifier.fillMaxSize()) {
                repeat(group.stackedRowsShown) { index ->
                    val color = if (index == group.shown) scheme.primary else Color.Transparent
                    Box(
                        Modifier.weight(1f).fillMaxWidth().then(segment(index))
                            .padding(horizontal = 4.dp, vertical = 2.dp)
                            .background(color, CircleShape),
                    )
                }
            }
        }
        val hidden = group.tabs.size - group.stackedRowsShown
        if (group.kind == GroupKind.Stacked && hidden > 0) {
            Text(
                text = stringResource(R.string.map_more_rows, hidden),
                style = MaterialTheme.typography.labelSmall.tabular(),
                modifier = Modifier.align(Alignment.CenterEnd).background(scheme.surfaceContainerHigh).padding(horizontal = 4.dp),
            )
        }
    }
}
