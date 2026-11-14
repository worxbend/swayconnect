package com.swayconnect.android.ui.more

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayIcon
import com.swayconnect.android.ui.theme.SwayIcons

private val TILE_MIN_HEIGHT = 88.dp
private val BADGE = 40.dp
private const val DISABLED_ALPHA = 0.38f
private const val STALE_ALPHA = 0.6f
private const val BADGE_ON_INK_ALPHA = 0.16f

/** What a tile shows. [on] fills it with ink (UX 10.1); the first line then takes `accentOnInk`. */
internal class TileLook(
    val icon: SwayIcon,
    val title: String,
    val lines: List<String>,
    val on: Boolean = false,
    /** The scope is missing: a lock replaces the icon (UX 10.2 last row). */
    val locked: Boolean = false,
    val pending: Boolean = false,
)

/**
 * What TalkBack says. The tile is one node: [title] and [state] are spoken, the visible lines are not, so a
 * per-second countdown is never read out (UX 10.2, 15). [toggled] makes it a switch.
 */
internal class TileSpeech(
    val state: String?,
    val clickLabel: String?,
    val longClickLabel: String? = null,
    val toggled: Boolean? = null,
)

/**
 * How the tile takes input. [enabled] false shows it disabled; [stale] dims it as a state no connection
 * confirms (UX 13). A null [onClick] takes no tap.
 */
internal class TileInput(
    val onClick: (() -> Unit)?,
    val onLongClick: (() -> Unit)? = null,
    val enabled: Boolean = true,
    val stale: Boolean = false,
)

/** One quick-settings tile of docs/UX.md 10.1: 88dp or taller, an icon badge, a title and its lines. */
@Composable
internal fun QuickTile(look: TileLook, speech: TileSpeech, input: TileInput, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val effects = LocalSwayMotion.current.defaultEffectsSpec<Color>()
    val container by animateColorAsState(if (look.on) scheme.primary else scheme.surface, effects, label = "tile")
    val content by animateColorAsState(if (look.on) scheme.onPrimary else scheme.onSurface, effects, label = "tileContent")
    val alpha = when {
        !input.enabled -> DISABLED_ALPHA
        input.stale -> STALE_ALPHA
        else -> 1f
    }
    val clickable = input.onClick
    val interactive = if (clickable != null) {
        Modifier.combinedClickable(
            enabled = input.enabled,
            role = if (speech.toggled != null) Role.Switch else Role.Button,
            onClickLabel = speech.clickLabel,
            onLongClickLabel = speech.longClickLabel,
            onLongClick = input.onLongClick,
            indication = ripple(),
            interactionSource = null,
            onClick = clickable,
        )
    } else {
        Modifier
    }
    Row(
        modifier = modifier
            .heightIn(min = TILE_MIN_HEIGHT)
            .alpha(alpha)
            .clip(MaterialTheme.shapes.medium)
            .background(container)
            .then(interactive)
            .semantics {
                contentDescription = look.title
                speech.state?.let { stateDescription = it }
                speech.toggled?.let { toggleableState = ToggleableState(it) }
            }
            .clearAndSetSemantics {}
            .padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TileBadge(look, content)
        Column(Modifier.weight(1f).fillMaxHeight(), verticalArrangement = Arrangement.Center) {
            Text(look.title, style = MaterialTheme.typography.titleMedium, color = content, maxLines = 2, overflow = TextOverflow.Ellipsis)
            look.lines.forEachIndexed { index, line ->
                val accent = look.on && index == 0
                Text(
                    text = line,
                    style = MaterialTheme.typography.bodyMedium,
                    color = when {
                        accent -> LocalSwayTokens.current.accentOnInk
                        look.on -> content
                        else -> scheme.onSurfaceVariant
                    },
                )
            }
        }
    }
}

@Composable
private fun TileBadge(look: TileLook, content: Color) {
    val badge = if (look.on) content.copy(alpha = BADGE_ON_INK_ALPHA) else MaterialTheme.colorScheme.surfaceContainer
    Box(Modifier.size(BADGE).background(badge, CircleShape), contentAlignment = Alignment.Center) {
        when {
            look.pending -> CircularProgressIndicator(modifier = Modifier.size(20.dp), color = content, strokeWidth = 2.dp)
            look.locked -> Icon(SwayIcons.Lock.outline, contentDescription = null, tint = content, modifier = Modifier.size(22.dp))
            else -> Icon(if (look.on) look.icon.filled else look.icon.outline, contentDescription = null, tint = content, modifier = Modifier.size(22.dp))
        }
    }
}
