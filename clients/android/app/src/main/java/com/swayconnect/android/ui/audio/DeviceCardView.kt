package com.swayconnect.android.ui.audio

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.HoldButton
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcon
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.DeviceKind
import com.swayconnect.protocol.replaceUnsafe
import kotlin.math.roundToInt

/** Above this font scale the header stacks its pill under the name (UX 15). */
private const val LARGE_FONT_SCALE = 1.5f

private const val DISABLED_ALPHA = 0.38f
private const val STALE_ALPHA = 0.6f
private val BADGE = 44.dp

/** What the device cards call. The view model throttles the slider; the cards never do (UX 7.2, 17.8). */
internal class DeviceActions(
    val onSetDefault: (Long) -> Unit,
    val onSetMuted: (deviceId: Long, muted: Boolean) -> Unit,
    /** TalkBack, Switch Access or a keyboard activated the unmute hold: ask in a dialog instead (UX 7.2, 15). */
    val onConfirmUnmute: (Long) -> Unit,
    val onDrag: (deviceId: Long, pct: Int) -> Unit,
    val onFinish: (deviceId: Long, pct: Int) -> Unit,
    val onLocked: (scope: String) -> Unit,
)

/**
 * One device card of docs/UX.md 7.1 in the card style of reference 2: badge, name, kind line and a pill. The default
 * device adds its mute button, slider and percent; the row grows to it with the spatial spring.
 */
@Composable
internal fun DeviceCardView(card: DeviceCard, actions: DeviceActions, modifier: Modifier = Modifier) {
    val name = replaceUnsafe(card.device.description)
    val alpha = when {
        card.unplugged && !card.isDefault -> DISABLED_ALPHA
        !card.live -> STALE_ALPHA
        else -> 1f
    }
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surface,
        modifier = modifier.fillMaxWidth().alpha(alpha).animateContentSize(LocalSwayMotion.current.defaultSpatialSpec()),
    ) {
        Column {
            DeviceHeader(card, name, actions)
            if (card.isDefault) DeviceLevel(card, name, actions)
        }
    }
}

@Composable
private fun DeviceHeader(card: DeviceCard, name: String, actions: DeviceActions) {
    val haptics = LocalHaptics.current
    val tap = card.tap()
    val onClick: (() -> Unit)? = when (tap) {
        AudioTap.Send -> ({ actions.onSetDefault(card.device.id) })
        is AudioTap.ShowLocked -> ({
            haptics.perform(SwayHaptic.Reject)
            actions.onLocked(tap.scope)
        })
        AudioTap.Hold, AudioTap.None -> null
    }
    val clickLabel = stringResource(if (tap is AudioTap.ShowLocked) R.string.audio_show_how else R.string.audio_make_default)
    val spoken = headerStateWords(card)
    val clickable = if (onClick != null) Modifier.clickable(role = Role.Button, onClickLabel = clickLabel, onClick = onClick) else Modifier
    Row(
        modifier = Modifier.fillMaxWidth().then(clickable).semantics(mergeDescendants = true) { if (spoken.isNotEmpty()) stateDescription = spoken }.padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        DeviceBadge(card)
        // Above a large font scale the pill goes under the name and the state line may wrap, so the state word
        // (the non-colour sign of UX 7.1) is never cut off.
        val large = LocalDensity.current.fontScale > LARGE_FONT_SCALE
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(name, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(
                kindLine(card),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = if (large) 2 else 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (large) HeaderMark(card, tap)
        }
        if (!large) HeaderMark(card, tap)
    }
}

/** Default or Unplugged as a pill, or the lock of a device this phone may not change. */
@Composable
private fun HeaderMark(card: DeviceCard, tap: AudioTap) {
    when {
        card.isDefault -> StatePill(stringResource(R.string.audio_default), PillTone.Ok)
        card.unplugged -> StatePill(stringResource(R.string.audio_unplugged), PillTone.Tinted)
        tap is AudioTap.ShowLocked -> Icon(SwayIcons.Lock.outline, contentDescription = null, modifier = Modifier.size(20.dp))
    }
}

/**
 * Kind, then the state as a word: "Microphone · Live", "Speaker · Muted" (UX 7.1, colour never alone). An unplugged
 * input is never "Live"; an unplugged default device says so here, because its pill reads "Default".
 */
@Composable
private fun kindLine(card: DeviceCard): String {
    val kind = stringResource(kindWord(card.device.kind))
    val unplugged = stringResource(R.string.audio_unplugged).takeIf { card.unplugged && card.isDefault }
    val state = when {
        card.device.muted -> stringResource(R.string.audio_muted)
        card.role == DeviceRole.Input && !card.unplugged -> stringResource(R.string.audio_live)
        else -> null
    }
    return listOfNotNull(kind, unplugged, state).joinToString(" · ")
}

@Composable
private fun headerStateWords(card: DeviceCard): String =
    listOfNotNull(
        stringResource(if (card.role == DeviceRole.Output) R.string.audio_spoken_default else R.string.audio_spoken_default_input).takeIf { card.isDefault },
        stringResource(R.string.audio_spoken_unplugged).takeIf { card.unplugged },
    ).joinToString(", ")

private fun kindWord(kind: DeviceKind): Int =
    when (kind) {
        DeviceKind.SPEAKER -> R.string.audio_kind_speaker
        DeviceKind.HEADPHONES -> R.string.audio_kind_headphones
        DeviceKind.HDMI -> R.string.audio_kind_hdmi
        DeviceKind.BLUETOOTH -> R.string.audio_kind_bluetooth
        DeviceKind.USB -> R.string.audio_kind_usb
        DeviceKind.MICROPHONE -> R.string.audio_kind_microphone
        DeviceKind.OTHER -> R.string.audio_kind_other
    }

private fun DeviceCard.icon(): SwayIcon =
    when (role) {
        DeviceRole.Output -> if (device.muted) SwayIcons.VolumeOff else SwayIcons.Volume
        DeviceRole.Input -> if (device.muted) SwayIcons.MicOff else SwayIcons.Mic
    }

@Composable
private fun DeviceBadge(card: DeviceCard) {
    Box(Modifier.size(BADGE).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
        if (card.pending) {
            CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
        } else {
            Icon(card.icon().outline, contentDescription = null, modifier = Modifier.size(22.dp))
        }
    }
}

@Composable
private fun DeviceLevel(card: DeviceCard, name: String, actions: DeviceActions) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(start = 8.dp, end = 16.dp, bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MuteButton(card, name, actions)
        VolumeSlider(card, name, actions, Modifier.weight(1f))
        Column(Modifier.widthIn(min = 48.dp), horizontalAlignment = Alignment.End) {
            val numbers = MaterialTheme.typography.labelLarge.copy(fontFeatureSettings = "tnum")
            Text(stringResource(R.string.audio_percent, card.volume), style = numbers, textAlign = TextAlign.End, maxLines = 1)
            if (card.device.muted) {
                Text(stringResource(R.string.audio_muted), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            }
        }
        card.sliderLockedScope?.let { scope ->
            IconButton(onClick = { actions.onLocked(scope) }) {
                Icon(SwayIcons.Lock.outline, contentDescription = stringResource(R.string.audio_level_locked), modifier = Modifier.size(20.dp))
            }
        }
    }
}

/**
 * Mute as a 48dp toggle (UX 7.2). Unmuting a microphone with `audio.capture` is a 600 ms hold instead; without the
 * scope the button shows the lock and opens the Locked sheet.
 */
@Composable
private fun MuteButton(card: DeviceCard, name: String, actions: DeviceActions) {
    val haptics = LocalHaptics.current
    val muted = card.device.muted
    val label = stringResource(if (muted) R.string.audio_unmute else R.string.audio_mute, name)
    when (val tap = card.muteTap()) {
        AudioTap.Hold -> HoldButton(
            label = stringResource(R.string.audio_hold_unmute),
            icon = SwayIcons.MicOff.outline,
            onHeld = { actions.onSetMuted(card.device.id, false) },
            onConfirmRequest = { actions.onConfirmUnmute(card.device.id) },
            spoken = label,
            iconOnly = true,
        )
        else -> IconToggleButton(
            checked = muted,
            onCheckedChange = { mute ->
                when (tap) {
                    AudioTap.Send -> {
                        haptics.perform(if (mute) SwayHaptic.ToggleOn else SwayHaptic.ToggleOff)
                        actions.onSetMuted(card.device.id, mute)
                    }
                    is AudioTap.ShowLocked -> actions.onLocked(tap.scope)
                    AudioTap.Hold, AudioTap.None -> Unit
                }
            },
            enabled = tap != AudioTap.None,
            modifier = Modifier.semantics { contentDescription = label },
        ) {
            val icon = if (tap is AudioTap.ShowLocked) SwayIcons.Lock.outline else card.icon().outline
            Icon(icon, contentDescription = null)
        }
    }
}

/**
 * The value/onValueChange slider (UX 7.2, 17.8). It shows the view model's draft while the finger moves. A tick every
 * 5 % and a threshold at 0 and at the top; repeated values of one gesture are not passed on.
 */
@Composable
private fun VolumeSlider(card: DeviceCard, name: String, actions: DeviceActions, modifier: Modifier) {
    val haptics = LocalHaptics.current
    val shown by rememberUpdatedState(card.volume)
    // The value of the gesture in progress. The accessibility path calls onValueChange and then
    // onValueChangeFinished before any recomposition, so the final value cannot come from [shown].
    var gestureValue by remember { mutableStateOf<Int?>(null) }
    val label = stringResource(if (card.role == DeviceRole.Output) R.string.audio_volume_of else R.string.audio_level_of, name)
    val spoken = pluralStringResource(if (card.device.muted) R.plurals.audio_spoken_volume_muted else R.plurals.audio_spoken_volume, card.volume, card.volume)
    val id = card.device.id
    val max = card.maxVolume
    Slider(
        value = card.volume.toFloat(),
        onValueChange = { raw ->
            val pct = raw.roundToInt().coerceIn(0, max)
            val from = gestureValue ?: shown
            gestureValue = pct
            if (pct != from) {
                when (sliderHapticOf(from, pct, max)) {
                    SliderHaptic.Threshold -> haptics.perform(SwayHaptic.GestureThresholdActivate)
                    SliderHaptic.Tick -> haptics.perform(SwayHaptic.SegmentFrequentTick)
                    null -> Unit
                }
                actions.onDrag(id, pct)
            }
        },
        onValueChangeFinished = {
            val final = gestureValue ?: shown
            gestureValue = null
            actions.onFinish(id, final)
        },
        enabled = card.sliderEnabled,
        valueRange = 0f..max.toFloat(),
        modifier = modifier.semantics {
            contentDescription = label
            stateDescription = spoken
        },
    )
}
