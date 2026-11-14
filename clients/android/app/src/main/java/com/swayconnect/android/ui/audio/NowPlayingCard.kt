package com.swayconnect.android.ui.audio

import androidx.compose.animation.core.animateIntAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.PlayerStatus
import com.swayconnect.protocol.replaceUnsafe

private const val DISABLED_ALPHA = 0.38f
private const val STALE_ALPHA = 0.6f
private val PLAY_BUTTON = 72.dp

/** Corner percent of the play button: a circle while paused, a rounded square while playing (UX 7.1 shape morph). */
private const val ROUND_PERCENT = 50
private const val PLAYING_PERCENT = 30

/**
 * The now-playing card of docs/UX.md 7.1: app icon, title and artist (or "Playing · Chromium" without titles),
 * previous, a 72dp play/pause button and next. Every string comes from an application: shown as plain text with
 * unsafe characters replaced.
 */
@Composable
internal fun NowPlayingCard(card: NowPlaying, onCommand: (MediaCommand) -> Unit, modifier: Modifier = Modifier) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surface,
        modifier = modifier.fillMaxWidth().alpha(if (card.live) 1f else STALE_ALPHA),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            TrackLines(card)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                MediaButton(SwayIcons.Previous.outline, stringResource(R.string.media_previous), card.canSend(MediaCommand.PREVIOUS)) {
                    onCommand(MediaCommand.PREVIOUS)
                }
                PlayPauseButton(card) { onCommand(MediaCommand.PLAY_PAUSE) }
                MediaButton(SwayIcons.Next.outline, stringResource(R.string.media_next), card.canSend(MediaCommand.NEXT)) {
                    onCommand(MediaCommand.NEXT)
                }
            }
        }
    }
}

@Composable
private fun TrackLines(card: NowPlaying) {
    val player = card.player
    val name = replaceUnsafe(player.name)
    val first = player.title?.let(::replaceUnsafe) ?: stringResource(statusWords(player.status), name)
    val artist = player.artist?.let(::replaceUnsafe)
    val second = when {
        card.titleWithheld -> null
        artist != null -> stringResource(R.string.media_artist_on, artist, name)
        else -> name
    }
    Row(
        modifier = Modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // No icon: a speaker on the disc, not a letter guessed from the app id. A play glyph here read as a second
        // transport button beside Pause.
        AppIcon(player.appId, player.icon, size = 44.dp, monogramDisc = MaterialTheme.colorScheme.surfaceContainer) {
            Icon(SwayIcons.Volume.outline, contentDescription = null, modifier = Modifier.size(22.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(first, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            second?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

private fun statusWords(status: PlayerStatus): Int =
    when (status) {
        PlayerStatus.PLAYING -> R.string.media_playing_on
        PlayerStatus.PAUSED -> R.string.media_paused_on
        PlayerStatus.STOPPED -> R.string.media_stopped_on
    }

@Composable
private fun MediaButton(icon: ImageVector, label: String, enabled: Boolean, onClick: () -> Unit) {
    IconButton(onClick = onClick, enabled = enabled, modifier = Modifier.size(48.dp)) {
        Icon(icon, contentDescription = label)
    }
}

@Composable
private fun PlayPauseButton(card: NowPlaying, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val enabled = card.canSend(MediaCommand.PLAY_PAUSE)
    val corner by animateIntAsState(if (card.playing) PLAYING_PERCENT else ROUND_PERCENT, LocalSwayMotion.current.defaultSpatialSpec(), label = "playShape")
    val label = stringResource(if (card.playing) R.string.media_pause else R.string.media_play)
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(percent = corner),
        color = scheme.primary,
        contentColor = scheme.onPrimary,
        modifier = Modifier.size(PLAY_BUTTON).alpha(if (enabled || card.pending) 1f else DISABLED_ALPHA).semantics { contentDescription = label },
    ) {
        Box(contentAlignment = Alignment.Center) {
            if (card.pending) {
                CircularProgressIndicator(modifier = Modifier.size(28.dp), color = scheme.onPrimary, strokeWidth = 3.dp)
            } else {
                Icon(if (card.playing) SwayIcons.Pause.filled else SwayIcons.Play.filled, contentDescription = null, modifier = Modifier.size(32.dp))
            }
        }
    }
}
