package com.swayconnect.android.ui.keys

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.InputKey
import com.swayconnect.protocol.KeyModifier

private const val DISABLED_ALPHA = 0.38f

/**
 * The special keys row (UX 9.1, 9.2): 48dp tonal chips that scroll sideways. Ctrl, Alt and Super are sticky: filled
 * while armed, released by the next key, which the view model sends with them. Super carries a shield: its combos
 * reach sway bindings.
 */
@Composable
internal fun SpecialKeysRow(armed: Set<KeyModifier>, enabled: Boolean, onPress: (KeyPress) -> Unit, onToggle: (KeyModifier) -> Unit) {
    val haptics = LocalHaptics.current
    ChipRow {
        for (special in SPECIAL_KEYS) {
            val (label, spoken) = labelOf(special)
            when (special) {
                is SpecialKey.Press -> KeyChip(label, spoken, enabled) { onPress(KeyPress(special.key)) }
                is SpecialKey.Sticky -> {
                    val on = special.modifier in armed
                    StickyChip(label, spoken, on, enabled, shield = special.modifier == KeyModifier.SUPER) {
                        haptics.perform(if (on) SwayHaptic.ToggleOff else SwayHaptic.ToggleOn)
                        onToggle(special.modifier)
                    }
                }
            }
        }
    }
}

/** The shortcuts row (UX 9.1). [terminal]: Copy and Paste take Shift. */
@Composable
internal fun ShortcutsRow(terminal: Boolean, enabled: Boolean, onPress: (KeyPress) -> Unit) {
    ChipRow {
        for (shortcut in Shortcut.entries) {
            val (label, spoken) = labelOf(shortcut)
            KeyChip(label, spoken, enabled) { onPress(shortcut.press(terminal)) }
        }
    }
}

@Composable
private fun ChipRow(content: @Composable () -> Unit) {
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) { content() }
}

@Composable
private fun KeyChip(label: String, spoken: String, enabled: Boolean, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = CircleShape,
        color = scheme.surfaceContainer,
        contentColor = scheme.onSurface,
        border = BorderStroke(1.dp, scheme.outline),
        modifier = Modifier.keyChip(enabled).semantics { contentDescription = spoken },
    ) { ChipFace(label, shield = false) }
}

@Composable
private fun StickyChip(label: String, spoken: String, armed: Boolean, enabled: Boolean, shield: Boolean, onToggle: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val state = stringResource(if (armed) R.string.key_armed else R.string.key_not_armed)
    Surface(
        checked = armed,
        onCheckedChange = { onToggle() },
        enabled = enabled,
        shape = CircleShape,
        color = if (armed) scheme.primary else scheme.surfaceContainer,
        contentColor = if (armed) scheme.onPrimary else scheme.onSurface,
        border = BorderStroke(1.dp, scheme.outline),
        modifier = Modifier.keyChip(enabled).semantics {
            contentDescription = spoken
            stateDescription = state
        },
    ) { ChipFace(label, shield) }
}

private fun Modifier.keyChip(enabled: Boolean): Modifier = heightIn(min = 48.dp).widthIn(min = 48.dp).alpha(if (enabled) 1f else DISABLED_ALPHA)

@Composable
private fun ChipFace(label: String, shield: Boolean) {
    Row(
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (shield) Icon(SwayIcons.Shield.outline, contentDescription = null, modifier = Modifier.size(14.dp))
        Text(label, style = MaterialTheme.typography.labelLarge)
    }
}

@Composable
private fun labelOf(special: SpecialKey): Pair<String, String> =
    when (special) {
        is SpecialKey.Sticky -> when (special.modifier) {
            KeyModifier.CTRL -> pair(R.string.key_ctrl, R.string.key_ctrl_spoken)
            KeyModifier.ALT -> pair(R.string.key_alt, R.string.key_alt)
            KeyModifier.SUPER -> pair(R.string.key_super, R.string.key_super_spoken)
            KeyModifier.SHIFT -> error("Shift is not a chip of the special keys row")
        }
        is SpecialKey.Press -> when (special.key) {
            InputKey.ESCAPE -> pair(R.string.key_escape, R.string.key_escape_spoken)
            InputKey.TAB -> pair(R.string.key_tab, R.string.key_tab)
            InputKey.BACKSPACE -> pair(R.string.key_backspace, R.string.key_backspace_spoken)
            else -> arrowLabel(special.key.wire)
        }
    }

@Composable
private fun arrowLabel(wire: String): Pair<String, String> =
    when (wire) {
        "Left" -> pair(R.string.key_left, R.string.key_left_spoken)
        "Up" -> pair(R.string.key_up, R.string.key_up_spoken)
        "Down" -> pair(R.string.key_down, R.string.key_down_spoken)
        "Right" -> pair(R.string.key_right, R.string.key_right_spoken)
        else -> error("no label for the key $wire")
    }

@Composable
private fun labelOf(shortcut: Shortcut): Pair<String, String> =
    when (shortcut) {
        Shortcut.COPY -> pair(R.string.shortcut_copy, R.string.shortcut_copy)
        Shortcut.PASTE -> pair(R.string.shortcut_paste, R.string.shortcut_paste)
        Shortcut.UNDO -> pair(R.string.shortcut_undo, R.string.shortcut_undo)
        Shortcut.INTERRUPT -> pair(R.string.shortcut_interrupt, R.string.shortcut_interrupt_spoken)
        Shortcut.CLEAR_SCREEN -> pair(R.string.shortcut_clear, R.string.shortcut_clear_spoken)
    }

@Composable
private fun pair(label: Int, spoken: Int): Pair<String, String> = stringResource(label) to stringResource(spoken)
