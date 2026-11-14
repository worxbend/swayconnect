package com.swayconnect.android.ui.common

import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.swayconnect.android.data.IconCache
import com.swayconnect.android.ui.theme.LocalSwayTokens
import java.text.BreakIterator
import java.util.Locale

/** The icon cache of the app. Null (previews, tests, before wiring) shows monograms only. */
val LocalIconCache = staticCompositionLocalOf<IconCache<ImageBitmap>?> { null }

private const val CROSSFADE_MS = 120
private const val MONOGRAM_TEXT_RATIO = 0.45f

/**
 * An app's icon (docs/UX.md 17.6): the monogram of [appId] on its wash (or on [monogramDisc]) until the bitmap of
 * [iconHash] arrives, then a 120 ms crossfade at the same size. Decorative: the card or tile around it carries
 * the words. Only this icon recomposes when its own bitmap arrives, and a bitmap once shown stays, even when the
 * memory cache later evicts it.
 *
 * @param fallback drawn on the disc instead of the letter while there is no bitmap (a player's play glyph).
 */
@Composable
fun AppIcon(
    appId: String?,
    iconHash: String?,
    modifier: Modifier = Modifier,
    size: Dp = 28.dp,
    monogramDisc: Color? = null,
    fallback: (@Composable () -> Unit)? = null,
) {
    val cache = LocalIconCache.current
    val image by produceState(iconHash?.let { cache?.image(it) }, cache, iconHash) {
        if (cache == null || iconHash == null || value != null) return@produceState
        cache.want(iconHash)
        cache.loaded.collect { cache.image(iconHash)?.let { value = it } }
    }
    Crossfade(targetState = image, modifier = modifier.size(size), animationSpec = tween(CROSSFADE_MS), label = "icon") { bitmap ->
        if (bitmap != null) {
            Image(bitmap = bitmap, contentDescription = null, modifier = Modifier.fillMaxSize())
        } else {
            Monogram(appId, size, monogramDisc, fallback)
        }
    }
}

@Composable
private fun Monogram(appId: String?, size: Dp, disc: Color?, fallback: (@Composable () -> Unit)?) {
    val wash = disc ?: LocalSwayTokens.current.washFor(appId.orEmpty())
    Box(Modifier.fillMaxSize().background(wash, CircleShape), contentAlignment = Alignment.Center) {
        if (fallback != null) fallback() else MonogramLetter(appId, size)
    }
}

@Composable
private fun MonogramLetter(appId: String?, size: Dp) {
    // Sized from the circle, not the font scale: the letter must stay inside its disc at 200 %.
    val fontSize = with(LocalDensity.current) { (size * MONOGRAM_TEXT_RATIO).toSp() }
    Text(
        text = monogramOf(appId),
        color = MaterialTheme.colorScheme.onSurface,
        fontSize = fontSize,
        // The inherited body line height (48dp at font scale 2.0) would push the letter below a small disc.
        lineHeight = fontSize,
        fontWeight = FontWeight.SemiBold,
        maxLines = 1,
    )
}

/**
 * The first grapheme of [appId], upper-cased (docs/UX.md 17.6). A reverse-DNS id names the app in its last
 * segment, so `dev.vencord.Vesktop` gives "V", not "D". "?" for a missing or blank id.
 */
fun monogramOf(appId: String?): String {
    val id = appId?.trim()
    if (id.isNullOrEmpty()) return "?"
    val name = id.split('.').lastOrNull { it.isNotBlank() }?.trim() ?: id
    val graphemes = BreakIterator.getCharacterInstance(Locale.ROOT).apply { setText(name) }
    return name.substring(0, graphemes.next()).uppercase(Locale.ROOT)
}
