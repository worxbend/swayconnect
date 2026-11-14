package com.swayconnect.android.ui.shell

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.OpAvailability

private const val DISABLED_ALPHA = 0.38f

/**
 * The Drive / Browse context pill of Desktop (UX 4.1, 5.5), rendered from the availability of
 * `workspace.focus`. When the phone may not drive (scope missing, or the computer is set to look only) the app is
 * in Browse, so the pill says Browse, at full contrast, with a 12dp lock: a tap explains why ([onLocked] opens the
 * Locked sheet, [onReadOnly] the Computer sheet). "No connection yet" ([availability] null) shows the choice
 * disabled; unsupported hides it. [compact] draws the icon alone in a 48dp circle (the title needs the room); the
 * words stay in semantics.
 */
@Composable
internal fun DrivePill(
    drive: Boolean,
    availability: OpAvailability?,
    compact: Boolean,
    actions: DrivePillActions,
) {
    when (availability) {
        OpAvailability.Unsupported -> Unit
        is OpAvailability.Locked ->
            LockedBrowsePill(stringResource(R.string.drive_state_locked), compact) { actions.onLocked(availability.scope) }
        OpAvailability.ReadOnly -> LockedBrowsePill(stringResource(R.string.drive_state_read_only), compact, actions.onReadOnly)
        OpAvailability.Available, null ->
            DriveToggle(drive, enabled = availability == OpAvailability.Available, compact, actions.onDriveChange)
    }
}

/** What the Drive pill does: toggle, or explain why the phone may only browse. */
internal class DrivePillActions(
    val onDriveChange: (Boolean) -> Unit,
    val onLocked: (String) -> Unit,
    val onReadOnly: () -> Unit,
)

@Composable
private fun DriveToggle(drive: Boolean, enabled: Boolean, compact: Boolean, onDriveChange: (Boolean) -> Unit) {
    val haptics = LocalHaptics.current
    val label = stringResource(if (drive) R.string.drive_on else R.string.drive_off)
    val icon = if (drive) SwayIcons.Swap.outline else SwayIcons.Visibility.outline
    val spoken = stringResource(R.string.drive_spoken)
    val state = stringResource(if (drive) R.string.drive_state_on else R.string.drive_state_off)
    val toggle = Modifier
        .toggleable(value = drive, enabled = enabled, role = Role.Switch) { on ->
            haptics.perform(if (on) SwayHaptic.ToggleOn else SwayHaptic.ToggleOff)
            onDriveChange(on)
        }
        .semantics {
            contentDescription = spoken
            stateDescription = state
        }
    // Icon only, the word "Drive" is gone: the ink fill of the More tile then shows that Drive is on.
    PillFace(PillLook(icon, label, enabled, locked = false, compact, inked = compact && drive && enabled), toggle)
}

/** Browse with a lock: the only mode the phone has now. [why] completes "swipes only browse on the phone". */
@Composable
private fun LockedBrowsePill(why: String, compact: Boolean, onClick: () -> Unit) {
    val label = stringResource(R.string.drive_off)
    val state = stringResource(R.string.drive_state_off) + ", " + why
    val button = Modifier
        .clickable(role = Role.Button, onClickLabel = stringResource(R.string.drive_why), onClick = onClick)
        .semantics {
            contentDescription = label
            stateDescription = state
        }
    PillFace(PillLook(SwayIcons.Visibility.outline, label, enabled = true, locked = true, compact, inked = false), button)
}

/**
 * [modifier] carries the action and the words. The clearing node after it keeps them on one TalkBack node: without
 * it the emulator's accessibility tree showed the words and the action as two nodes of the same bounds.
 */
@Composable
private fun PillFace(look: PillLook, modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    Box(Modifier.clip(CircleShape).background(if (look.inked) scheme.primary else scheme.surface).then(modifier).clearAndSetSemantics {}) {
        PillContent(look)
    }
}

/** [inked]: the ink fill with `onPrimary` content instead of the card fill. */
private class PillLook(val icon: ImageVector, val label: String, val enabled: Boolean, val locked: Boolean, val compact: Boolean, val inked: Boolean)

@Composable
private fun PillContent(look: PillLook) {
    Row(
        modifier = Modifier
            .heightIn(min = 48.dp)
            .widthIn(min = 48.dp)
            .padding(horizontal = if (look.compact) 14.dp else 16.dp)
            .alpha(if (look.enabled) 1f else DISABLED_ALPHA)
            .clearAndSetSemantics {},
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box {
            val tint = if (look.inked) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface
            Icon(imageVector = look.icon, contentDescription = null, modifier = Modifier.size(20.dp), tint = tint)
            if (look.locked) LockBadge(Modifier.align(Alignment.BottomEnd).offset(x = 4.dp, y = 4.dp))
        }
        if (!look.compact) {
            Text(
                text = look.label,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** The 12dp lock of a locked feature (UX 4.2), on the pill's card colour so it reads over the icon. */
@Composable
private fun LockBadge(modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    Box(modifier.size(12.dp).background(scheme.surface, CircleShape), contentAlignment = Alignment.Center) {
        Icon(imageVector = SwayIcons.Lock.filled, contentDescription = null, tint = scheme.onSurface, modifier = Modifier.size(10.dp))
    }
}

/**
 * The 56dp ink round button of an area (UX 4.1). [onClick] null means this build has no action for the op yet:
 * an available op then hides the button instead of offering a tap that does nothing.
 */
@Composable
internal fun RoundButton(
    icon: ImageVector,
    label: String,
    availability: OpAvailability?,
    onClick: (() -> Unit)?,
    onLocked: (String) -> Unit,
) {
    when (availability) {
        null, OpAvailability.Unsupported -> Unit
        OpAvailability.Available -> if (onClick != null) RoundFace(icon, label, enabled = true, onClick)
        OpAvailability.ReadOnly -> RoundFace(icon, stringResource(R.string.action_read_only, label), enabled = false) {}
        is OpAvailability.Locked ->
            RoundFace(SwayIcons.Lock.outline, stringResource(R.string.action_locked, label), enabled = true) { onLocked(availability.scope) }
    }
}

@Composable
private fun RoundFace(icon: ImageVector, spoken: String, enabled: Boolean, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    // Disabled reads as an outline ring, not as faded ink: 38 % ink on greige looked like a broken button.
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = CircleShape,
        color = if (enabled) scheme.primary else Color.Transparent,
        contentColor = if (enabled) scheme.onPrimary else scheme.onSurfaceVariant,
        border = if (enabled) null else BorderStroke(1.dp, scheme.outline),
        modifier = Modifier.size(56.dp).semantics { contentDescription = spoken },
    ) {
        Row(horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
            Icon(imageVector = icon, contentDescription = null, modifier = Modifier.size(24.dp))
        }
    }
}
