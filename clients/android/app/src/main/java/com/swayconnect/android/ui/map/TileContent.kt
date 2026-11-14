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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.LastBaseline
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.common.GroupKind
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.common.appLine
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
    /** The app's name for the window's `app_id` (its desktop entry's name, else the id); null without one. */
    val appName: String?,
)

/** The corner of a map tile, and of the focus ring around it. */
internal val TILE_SHAPE = RoundedCornerShape(10.dp)

/** The title bar strip of large and medium tiles (UX 6.2): the tile's own fill, one step darker. */
private val TITLE_BAR = 4.dp
private const val TITLE_BAR_ALPHA = 0.08f

/**
 * The look of one map tile (UX 6.2 to 6.7), filling the rect it is measured to. A tiled window keeps its app
 * wash, focused or not (the shared focus ring over it marks focus); floating and fullscreen ones are white.
 * Large and medium tiles carry a thin title bar strip along the top, so a tile reads as a window.
 */
@Composable
internal fun TileBody(look: TileLook, modifier: Modifier = Modifier) {
    val tile = look.tile
    if (tile.sizeClass == SizeClass.Sliver) {
        Sliver(look, modifier)
        return
    }
    val scheme = MaterialTheme.colorScheme
    val floating = tile.layer != TileLayer.Tiled
    val fill = if (floating) scheme.surface else LocalSwayTokens.current.washFor(look.window.appId.orEmpty())
    val dark = scheme.surface.luminance() < 0.5f
    val border = when {
        look.window.urgent -> BorderStroke(2.dp, scheme.error)
        floating && dark -> BorderStroke(1.dp, scheme.onSurfaceVariant)
        // A white float on the white slab needs the full outline; a wash tile has its fill for an edge.
        floating -> BorderStroke(1.dp, scheme.outline)
        else -> BorderStroke(1.dp, scheme.outlineVariant)
    }
    val large = tile.sizeClass == SizeClass.Large || tile.sizeClass == SizeClass.Medium
    val lifted = if (floating && !dark) Modifier.shadow(6.dp, TILE_SHAPE) else Modifier
    val titleBar = if (large) Modifier.titleBar(scheme.onSurface.copy(alpha = TITLE_BAR_ALPHA).compositeOver(fill)) else Modifier
    Box(modifier.then(lifted).clip(TILE_SHAPE).background(fill).then(titleBar).border(border, TILE_SHAPE)) {
        CompositionLocalProvider(LocalContentColor provides scheme.onSurface) {
            when (tile.sizeClass) {
                SizeClass.Large -> LargeContent(look)
                SizeClass.Medium -> MediumContent(look)
                SizeClass.Small -> SmallContent(look)
                SizeClass.Sliver -> Unit
            }
        }
        if (look.pending && large) {
            CircularProgressIndicator(Modifier.align(Alignment.TopEnd).padding(4.dp).size(SPINNER_SIZE), color = scheme.onSurface, strokeWidth = 2.dp)
        } else if (look.window.urgent && large) {
            UrgentDot(look.pulseNewMarks, Modifier.align(Alignment.TopEnd).padding(2.dp))
        }
        if (tile.layer == TileLayer.Fullscreen && large) {
            StatePill(stringResource(R.string.window_mark_fullscreen), PillTone.Tinted, Modifier.align(Alignment.BottomStart).padding(8.dp))
        }
    }
}

private fun Modifier.titleBar(color: Color): Modifier = drawBehind { drawRect(color, size = Size(size.width, TITLE_BAR.toPx())) }

/** The window's title; without `state.titles` its app name, else "Untitled". Never invented. */
@Composable
private fun shownTitle(look: TileLook): String = look.window.title ?: look.appName ?: stringResource(R.string.window_untitled)

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
        IconOnDisc(window, iconSize = 24.dp)
        Column(Modifier.weight(1f)) {
            Text(
                text = shownTitle(look),
                style = MaterialTheme.typography.labelLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                // Measured after the app id line: at a large font scale the title gives way, not the app id.
                modifier = Modifier.weight(1f, fill = false),
            )
            val appLine = listOfNotNull(
                appLine(window.title, look.appName),
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
    if (MapGeometry.mediumShowsTitle(look.tile.rect, LocalDensity.current.fontScale)) {
        TitledMediumContent(look)
        return
    }
    val fragment = look.window.title?.take(TITLE_FRAGMENT_CHARS)?.takeIf { look.tile.showsTitleFragment && look.sameAppIndex != null }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.align(Alignment.Center).padding(4.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            IconOnDisc(look.window, iconSize = 28.dp)
            if (fragment != null) {
                // The index follows the fragment on its line: a corner badge would overlap it at a large font scale.
                // Two texts, so a fragment cut by the ellipsis never takes the index with it.
                val style = MaterialTheme.typography.labelSmall.tabular()
                Row(horizontalArrangement = Arrangement.spacedBy(INDEX_GAP)) {
                    Text(text = fragment, style = style, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                    Text(text = stringResource(R.string.window_same_app_index, look.sameAppIndex ?: 0), style = style, maxLines = 1)
                }
            }
        }
        val index = look.sameAppIndex
        if (index == null || fragment != null) return@Box
        val badge = stringResource(R.string.window_same_app_index, index)
        // Measured in the style the Text draws: the theme's text style under labelSmall (its letter spacing too).
        val style = LocalTextStyle.current.merge(MaterialTheme.typography.labelSmall.tabular())
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        val badgeWidth = remember(badge, style, density) { measurer.measure(badge, style, maxLines = 1).size.width / density.density }
        if (MapGeometry.indexBadgeFits(look.tile.rect.width, badgeWidth)) {
            Text(text = badge, style = style, modifier = Modifier.align(Alignment.BottomEnd).padding(horizontal = 6.dp, vertical = 2.dp))
        }
    }
}

private val TITLED_PADDING = 8.dp

/** Between a title and its `#N`: a gap, not a space glued into the string. */
private val INDEX_GAP = 4.dp

/** A medium tile tall enough for a title (UX 6.2): the icon toward the top, the title centred under it, then `#N`. */
@Composable
private fun TitledMediumContent(look: TileLook) {
    Column(
        Modifier.fillMaxSize().padding(TITLED_PADDING),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        IconOnDisc(look.window, iconSize = 28.dp)
        val title = shownTitle(look)
        // Measured in the style the Text draws: the theme's text style under labelSmall (its letter spacing too).
        val style = LocalTextStyle.current.merge(MaterialTheme.typography.labelSmall)
        val index = look.sameAppIndex?.let { stringResource(R.string.window_same_app_index, it) }
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        val lines = remember(title, index, style, density, look.tile.rect.width) {
            fun widthOf(text: String) = measurer.measure(text, style, maxLines = 1).size.width / density.density
            val longestWord = MapGeometry.titleWords(title).maxOfOrNull(::widthOf) ?: 0f
            val line = look.tile.rect.width - 2 * TITLED_PADDING.value - (index?.let { widthOf(it) + INDEX_GAP.value } ?: 0f)
            MapGeometry.mediumTitleLines(longestWord, line)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(INDEX_GAP)) {
            Text(
                text = title,
                style = style,
                textAlign = TextAlign.Center,
                maxLines = lines,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false).alignBy(LastBaseline),
            )
            // Its own text on the title's last line, so the ellipsis of a long title never cuts the index.
            index?.let {
                Text(
                    text = it,
                    style = style.tabular(),
                    maxLines = 1,
                    modifier = Modifier.alignBy(LastBaseline),
                )
            }
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
 * The tile's icon in a 32dp box. A monogram (no icon yet) sits on a `surface` disc: on its own wash it was a stray
 * letter on a tile of the same wash.
 */
@Composable
private fun IconOnDisc(window: Window, iconSize: Dp) {
    Box(Modifier.size(maxOf(iconSize, 32.dp)), contentAlignment = Alignment.Center) {
        AppIcon(appId = window.appId, iconHash = window.icon, size = iconSize, monogramDisc = MaterialTheme.colorScheme.surface)
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

/**
 * The header strip of a tabbed or stacked group (UX 6.5). [segment] adds the touch and semantics of segment i.
 * Tabbed: one segment per child, the shown one white with an ink underline. Stacked: one row per child in its
 * app's wash, the shown one marked by an ink bar at its start.
 */
@Composable
internal fun GroupStrip(group: PlacedGroup, windows: Map<Long, Window>, modifier: Modifier, segment: (Int) -> Modifier) {
    val scheme = MaterialTheme.colorScheme
    val tokens = LocalSwayTokens.current
    val shape = MaterialTheme.shapes.extraSmall
    // Clipped, so the underline and the row fills stay inside the rounded corners.
    Box(modifier.clip(shape).background(scheme.surfaceContainerHigh).border(1.dp, scheme.outline, shape)) {
        when (group.kind) {
            GroupKind.Tabbed -> Row(Modifier.fillMaxSize()) {
                group.tabs.forEachIndexed { index, ids ->
                    val shown = index == group.shown
                    val fill = if (shown) Modifier.background(scheme.surface) else Modifier
                    Box(Modifier.weight(1f).fillMaxHeight().then(fill).then(segment(index)), contentAlignment = Alignment.Center) {
                        Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                            ids.take(2).forEach { id -> windows[id]?.let { AppIcon(it.appId, it.icon, size = 16.dp) } }
                        }
                        if (shown) {
                            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(2.dp).background(scheme.primary))
                        }
                    }
                }
            }
            GroupKind.Stacked -> Column(Modifier.fillMaxSize()) {
                repeat(group.stackedRowsShown) { index ->
                    val appId = group.tabs[index].firstOrNull()?.let { windows[it]?.appId }
                    Box(
                        Modifier.weight(1f).fillMaxWidth().then(segment(index))
                            .padding(bottom = 1.dp)
                            .background(tokens.washFor(appId.orEmpty())),
                    ) {
                        if (index == group.shown) {
                            Box(Modifier.align(Alignment.CenterStart).fillMaxHeight().width(2.dp).background(scheme.primary))
                        }
                    }
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
