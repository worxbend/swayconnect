package com.swayconnect.android.ui.shell

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.audio.NowPlaying
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.MediaCommand
import com.swayconnect.protocol.replaceUnsafe

/** Height of the capsule (UX 4.3); the shell keeps [CAPSULE_GAP] between it and the nav. */
internal val CAPSULE_HEIGHT = 52.dp
internal val CAPSULE_GAP = 8.dp
private const val STALE_ALPHA = 0.6f

/** What the capsule does. [onOpenAudio]: a tap on the text. [onHide]: "Hide until next track". */
internal class CapsuleActions(
    val onCommand: (MediaCommand) -> Unit,
    val onOpenAudio: () -> Unit,
    val onHide: () -> Unit,
)

/**
 * The now-playing capsule of docs/UX.md 4.3: a floating 52dp pill in the card colour with the app icon, one line of
 * text, play/pause and next. A tap on the text opens Audio; a long press opens a menu with "Hide until next track",
 * which TalkBack offers as a custom action. No swipe. Every string comes from an application: shown as plain text
 * with unsafe characters replaced.
 */
@Composable
internal fun NowPlayingCapsule(card: NowPlaying, actions: CapsuleActions, modifier: Modifier = Modifier) {
    // Light: a shadow like the nav. Dark: a 1dp outline stroke instead (same rule as the pill nav).
    val dark = LocalSwayTokens.current.navContainer.luminance() < 0.5f
    Surface(
        modifier = modifier.fillMaxWidth().height(CAPSULE_HEIGHT).alpha(if (card.live) 1f else STALE_ALPHA),
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = if (dark) 0.dp else 6.dp,
        border = if (dark) BorderStroke(1.dp, MaterialTheme.colorScheme.outline) else null,
    ) {
        Row(Modifier.padding(start = 6.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            CapsuleText(card, actions, Modifier.weight(1f).fillMaxHeight())
            PlayPause(card) { actions.onCommand(MediaCommand.PLAY_PAUSE) }
            IconButton(
                onClick = { actions.onCommand(MediaCommand.NEXT) },
                enabled = card.canSend(MediaCommand.NEXT),
                modifier = Modifier.size(48.dp),
            ) {
                Icon(SwayIcons.Next.outline, contentDescription = stringResource(R.string.media_next))
            }
        }
    }
}

@Composable
private fun CapsuleText(card: NowPlaying, actions: CapsuleActions, modifier: Modifier) {
    val haptics = LocalHaptics.current
    var menuOpen by remember { mutableStateOf(false) }
    val hideLabel = stringResource(R.string.capsule_hide)
    Box(modifier) {
        Row(
            modifier = Modifier
                .fillMaxHeight()
                .clip(CircleShape)
                .combinedClickable(
                    role = Role.Button,
                    onClickLabel = stringResource(R.string.capsule_open_audio),
                    onLongClickLabel = hideLabel,
                    onLongClick = {
                        haptics.perform(SwayHaptic.LongPress)
                        menuOpen = true
                    },
                    hapticFeedbackEnabled = false,
                    onClick = actions.onOpenAudio,
                )
                .semantics(mergeDescendants = true) {
                    customActions = listOf(
                        CustomAccessibilityAction(hideLabel) {
                            actions.onHide()
                            true
                        },
                    )
                }
                .padding(end = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // A player's id often names a browser or a bus name, not the music: a speaker says more than a letter, and
            // names no action (a play glyph beside the pause control read as a second transport button).
            AppIcon(card.player.appId, card.player.icon, size = 40.dp, monogramDisc = MaterialTheme.colorScheme.surfaceContainer) {
                Icon(SwayIcons.Volume.filled, contentDescription = null, tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(20.dp))
            }
            Text(capsuleLine(card), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            DropdownMenuItem(
                text = { Text(hideLabel) },
                onClick = {
                    menuOpen = false
                    actions.onHide()
                },
            )
        }
    }
}

/** "Title · Artist", the title alone, or without a title (no `state.titles`) "Playing · Chromium" (UX 4.3). */
@Composable
private fun capsuleLine(card: NowPlaying): String {
    val player = card.player
    val title = player.title?.let(::replaceUnsafe)
    val artist = player.artist?.let(::replaceUnsafe)
    return when {
        title == null -> stringResource(if (card.playing) R.string.media_playing_on else R.string.media_paused_on, replaceUnsafe(player.name))
        artist == null -> title
        else -> stringResource(R.string.media_artist_on, title, artist)
    }
}

@Composable
private fun PlayPause(card: NowPlaying, onClick: () -> Unit) {
    if (card.pending) {
        Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 2.dp)
        }
        return
    }
    IconButton(onClick = onClick, enabled = card.canSend(MediaCommand.PLAY_PAUSE), modifier = Modifier.size(48.dp)) {
        Icon(
            if (card.playing) SwayIcons.Pause.filled else SwayIcons.Play.filled,
            contentDescription = stringResource(if (card.playing) R.string.media_pause else R.string.media_play),
        )
    }
}
