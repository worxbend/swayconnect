package com.swayconnect.android.ui.desktop

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.HoldButton
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.WindowLayout

/** Above this font scale the four layout choices stand in a column of radio rows (UX 15). */
private const val STACKED_FONT_SCALE = 1.5f

/** The M2a window operations of UX 5.7 and 5.8. Each goes to the view model; `window.close` only after a yes. */
internal class WindowOps(
    /** Float a tiled window, tile a floating one. */
    val onToggleFloating: (Window) -> Unit,
    val onOpenMove: (windowId: Long) -> Unit,
    val onMoveToWorkspace: (windowId: Long, workspaceId: Long) -> Unit,
    /** "+ new": sway creates the workspace with that number. */
    val onMoveToNumber: (windowId: Long, number: Int) -> Unit,
    val onLayout: (windowId: Long, WindowLayout) -> Unit,
    val onScratchpad: (windowId: Long) -> Unit,
    /** A completed hold: the hold is the user's yes (docs/PROTOCOL.md 17.7). */
    val onCloseHeld: (windowId: Long) -> Unit,
    /** TalkBack, Switch Access or a keyboard cannot hold: they get the confirm dialog. */
    val onAskClose: (windowId: Long) -> Unit,
    /** The yes of that dialog. */
    val onConfirmClose: () -> Unit,
)

/** The same operations, each closing the sheet first. Opening another sheet (Move, Confirm) replaces this one. */
internal fun WindowOps.closing(onClose: () -> Unit) =
    WindowOps(
        onToggleFloating = { window ->
            onClose()
            onToggleFloating(window)
        },
        onOpenMove = onOpenMove,
        onMoveToWorkspace = { id, workspaceId ->
            onClose()
            onMoveToWorkspace(id, workspaceId)
        },
        onMoveToNumber = { id, number ->
            onClose()
            onMoveToNumber(id, number)
        },
        onLayout = { id, layout ->
            onClose()
            onLayout(id, layout)
        },
        onScratchpad = { id ->
            onClose()
            onScratchpad(id)
        },
        onCloseHeld = onCloseHeld,
        onAskClose = onAskClose,
        onConfirmClose = onConfirmClose,
    )

/**
 * The buttons of the focused window (UX 5.7): Full, Float, Move, a 1dp divider, then Close, icon only. One row
 * that never wraps: when the labelled buttons do not fit the card's real width, or above font scale 1.5, every
 * button goes icon only.
 */
@Composable
internal fun FocusedButtons(window: Window, enabled: Boolean, facts: PageFacts, actions: PageActions) {
    val labels = listOf(stringResource(fullscreenLabel(window)), stringResource(floatLabel(window)), stringResource(R.string.desktop_move))
    // The style a button's label is drawn in: labelLarge over the theme's text style. labelLarge alone left out
    // the inherited 0.5sp letter spacing, so the budget ran about 2dp short per label and clipped "Move".
    val labelWidths = measuredWidths(labels, LocalTextStyle.current.merge(MaterialTheme.typography.labelLarge))
    val largeFont = LocalDensity.current.fontScale > LARGE_FONT_SCALE
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val iconOnly = largeFont || !FocusedRow.labelsFit(maxWidth.value, labelWidths)
        val gap = Arrangement.spacedBy(FocusedRow.GAP.dp)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = gap, verticalAlignment = Alignment.CenterVertically) {
            // Takes what is left, so the divider and Close keep one spot at the card's end edge ("Float" and
            // "Tile" differ in width). Same gap count as one flat row, so [FocusedRow.labelsFit] still holds.
            Row(Modifier.weight(1f), horizontalArrangement = gap, verticalAlignment = Alignment.CenterVertically) {
                FullscreenButton(window, iconOnly, enabled, facts, actions)
                FloatButton(window, iconOnly, enabled, facts, actions)
                GatedButton(
                    availability = facts.availabilityOf(Op.WINDOW_MOVE),
                    icon = SwayIcons.Move.outline,
                    label = labels[2],
                    spoken = stringResource(R.string.desktop_move_spoken),
                    iconOnly = iconOnly,
                    enabled = enabled,
                    onLocked = actions.onLocked,
                    onClick = { actions.window.onOpenMove(window.id) },
                )
            }
            if (facts.availabilityOf(Op.WINDOW_CLOSE) != OpAvailability.Unsupported) {
                VerticalDivider(Modifier.height(32.dp), thickness = FocusedRow.DIVIDER.dp, color = MaterialTheme.colorScheme.outline)
            }
            CloseButton(window, stringResource(R.string.desktop_close), iconOnly = true, enabled, facts, actions)
        }
    }
}

/** The width in dp of each of [texts] on one line in [style]. */
@Composable
private fun measuredWidths(texts: List<String>, style: TextStyle): List<Float> {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    return remember(texts, style, density) { texts.map { measurer.measure(it, style, maxLines = 1).size.width / density.density } }
}

internal fun floatLabel(window: Window): Int = if (window.floating) R.string.desktop_tile else R.string.desktop_float

@Composable
internal fun FloatButton(window: Window, iconOnly: Boolean, enabled: Boolean, facts: PageFacts, actions: PageActions) {
    GatedButton(
        availability = facts.availabilityOf(Op.WINDOW_FLOATING),
        icon = SwayIcons.Float.outline,
        label = stringResource(floatLabel(window)),
        spoken = stringResource(if (window.floating) R.string.desktop_tile_spoken else R.string.desktop_float_spoken),
        iconOnly = iconOnly,
        enabled = enabled,
        onLocked = actions.onLocked,
        onClick = { actions.window.onToggleFloating(window) },
    )
}

/**
 * Close (UX 5.7, 5.8): destructive tier, `error` colour, a 600ms hold. Locked, read-only and unsupported follow
 * [GatedButton]: without `destructive` it shows a lock and opens the Locked sheet instead of a hold. On a
 * read-only computer the locked Close is drawn muted like its disabled siblings: a grant would not make it work.
 */
@Composable
internal fun CloseButton(window: Window, label: String, iconOnly: Boolean, enabled: Boolean, facts: PageFacts, actions: PageActions) {
    val availability = facts.availabilityOf(Op.WINDOW_CLOSE)
    if (availability != OpAvailability.Available) {
        GatedButton(
            availability = availability,
            icon = SwayIcons.Close.outline,
            label = label,
            spoken = stringResource(R.string.desktop_close_spoken),
            iconOnly = iconOnly,
            enabled = enabled,
            onLocked = actions.onLocked,
            onClick = {},
            muted = facts.granted?.readOnly == true,
        )
        return
    }
    HoldButton(
        label = label,
        icon = SwayIcons.Close.outline,
        onHeld = { actions.window.onCloseHeld(window.id) },
        onConfirmRequest = { actions.window.onAskClose(window.id) },
        spoken = stringResource(R.string.desktop_close_spoken),
        iconOnly = iconOnly,
        enabled = enabled,
        color = MaterialTheme.colorScheme.error,
    )
}

/**
 * The Window sheet's actions (UX 5.8): Focus, Full, Float, Scratchpad, the move pills, the container's layout,
 * then Close after a divider. [showLayoutNote]: the autotiling note, once per run.
 */
@Composable
internal fun WindowSheetActions(window: Window, desktop: Desktop, facts: PageFacts, actions: PageActions, showLayoutNote: Boolean) {
    val enabled = facts.live && facts.pendingId != window.id
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        GatedButton(
            availability = facts.availabilityOf(Op.WINDOW_FOCUS),
            icon = SwayIcons.Check.outline,
            label = stringResource(R.string.desktop_focus_window),
            iconOnly = false,
            enabled = enabled && !window.focused,
            onLocked = actions.onLocked,
            onClick = { actions.onFocusWindow(window.id) },
        )
        FullscreenButton(window, iconOnly = false, enabled, facts, actions)
        FloatButton(window, iconOnly = false, enabled, facts, actions)
        GatedButton(
            availability = facts.availabilityOf(Op.WINDOW_SCRATCHPAD),
            icon = SwayIcons.Scratchpad.outline,
            label = stringResource(R.string.desktop_scratchpad),
            spoken = stringResource(R.string.desktop_scratchpad_spoken),
            iconOnly = false,
            enabled = enabled,
            onLocked = actions.onLocked,
            onClick = { actions.window.onScratchpad(window.id) },
        )
    }
    MoveSection(window, desktop, enabled, facts, actions)
    if (!window.floating) LayoutSection(window, desktop, enabled, facts, actions, showLayoutNote)
    if (facts.availabilityOf(Op.WINDOW_CLOSE) != OpAvailability.Unsupported) {
        HorizontalDivider()
        CloseButton(window, stringResource(R.string.desktop_close_window_hold), iconOnly = false, enabled, facts, actions)
    }
}

/** "Move to workspace": one pill per workspace (the window's own one filled, not a target) and "+ new" (UX 5.8). */
@Composable
internal fun MoveSection(window: Window, desktop: Desktop, enabled: Boolean, facts: PageFacts, actions: PageActions) {
    val availability = facts.availabilityOf(Op.WINDOW_MOVE)
    if (availability == OpAvailability.Unsupported) return
    SectionTitle(stringResource(R.string.desktop_move_title))
    if (availability is OpAvailability.Locked) {
        LockedSectionButton(availability, stringResource(R.string.desktop_move), actions)
        return
    }
    val sendable = enabled && availability == OpAvailability.Available
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (workspace in desktop.workspaces) {
            val text = cardText(workspace)
            val here = workspace.id == window.workspaceId
            val spoken = listOfNotNull(stringResource(R.string.desktop_card_spoken, text.number), text.name.takeIf { it.isNotEmpty() }).joinToString(", ")
            NumberPill(
                label = text.number,
                spoken = spoken,
                state = if (here) stringResource(R.string.desktop_move_here) else null,
                enabled = sendable && !here,
                filled = here,
            ) { actions.window.onMoveToWorkspace(window.id, workspace.id) }
        }
        lowestFreeNumber(desktop)?.let { number ->
            NumberPill(
                label = stringResource(R.string.desktop_move_new),
                spoken = stringResource(R.string.desktop_move_new_spoken, number),
                state = null,
                enabled = sendable,
                filled = false,
            ) { actions.window.onMoveToNumber(window.id, number) }
        }
    }
}

/** A 48dp round target with a number (UX 5.8). [filled] marks where the window is now. */
@Composable
private fun NumberPill(label: String, spoken: String, state: String?, enabled: Boolean, filled: Boolean, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = CircleShape,
        color = if (filled) scheme.primary else scheme.surface,
        contentColor = if (filled) scheme.onPrimary else scheme.onSurface,
        border = BorderStroke(1.dp, if (filled) scheme.primary else scheme.outline),
        modifier = Modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp).semantics {
            contentDescription = spoken
            if (state != null) stateDescription = state
        },
    ) {
        Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1)
        }
    }
}

/**
 * "Layout of its container" (UX 5.8): splith, splitv, tabbed, stacking, the current one marked from the layout
 * tree. A column of radio rows above font scale 1.5. Floating windows have no such container: not shown.
 */
@Composable
private fun LayoutSection(window: Window, desktop: Desktop, enabled: Boolean, facts: PageFacts, actions: PageActions, showNote: Boolean) {
    val availability = facts.availabilityOf(Op.WINDOW_LAYOUT)
    if (availability == OpAvailability.Unsupported) return
    SectionTitle(stringResource(R.string.desktop_layout_title))
    if (availability is OpAvailability.Locked) {
        LockedSectionButton(availability, stringResource(R.string.desktop_layout_title), actions)
        return
    }
    val current = desktop.workspaces.firstOrNull { it.id == window.workspaceId }?.let { containerLayoutOf(it, window.id) }
    val sendable = enabled && availability == OpAvailability.Available
    val choose = { layout: WindowLayout -> if (layout != current) actions.window.onLayout(window.id, layout) }
    val choices = WindowLayout.entries.map { LayoutChoice(it, stringResource(layoutLabel(it)), stringResource(layoutSpoken(it))) }
    if (LocalDensity.current.fontScale > STACKED_FONT_SCALE) {
        Column(Modifier.selectableGroup()) {
            for (choice in choices) LayoutRadioRow(choice, selected = choice.layout == current, sendable) { choose(choice.layout) }
        }
    } else {
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            choices.forEachIndexed { index, choice ->
                SegmentedButton(
                    selected = choice.layout == current,
                    onClick = { choose(choice.layout) },
                    shape = SegmentedButtonDefaults.itemShape(index, choices.size),
                    enabled = sendable,
                    icon = {},
                    modifier = Modifier.heightIn(min = 48.dp).semantics { contentDescription = choice.spoken },
                    label = { Text(choice.label, maxLines = 1) },
                )
            }
        }
    }
    if (showNote) StateLine(stringResource(R.string.desktop_layout_note))
}

private class LayoutChoice(val layout: WindowLayout, val label: String, val spoken: String)

@Composable
private fun LayoutRadioRow(choice: LayoutChoice, selected: Boolean, enabled: Boolean, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null, enabled = enabled)
        Text(choice.spoken, modifier = Modifier.padding(start = 12.dp), style = MaterialTheme.typography.bodyLarge)
    }
}

private fun layoutLabel(layout: WindowLayout): Int =
    when (layout) {
        WindowLayout.SPLIT_H -> R.string.desktop_layout_split_h
        WindowLayout.SPLIT_V -> R.string.desktop_layout_split_v
        WindowLayout.TABBED -> R.string.desktop_layout_tabbed
        WindowLayout.STACKING -> R.string.desktop_layout_stacked
    }

private fun layoutSpoken(layout: WindowLayout): Int =
    when (layout) {
        WindowLayout.SPLIT_H -> R.string.desktop_layout_split_h_spoken
        WindowLayout.SPLIT_V -> R.string.desktop_layout_split_v_spoken
        WindowLayout.TABBED -> R.string.desktop_layout_tabbed
        WindowLayout.STACKING -> R.string.desktop_layout_stacked
    }

@Composable
private fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.labelLarge)
}

/** A section whose op is locked: one button with the lock that opens the Locked sheet (UX 5.7). */
@Composable
private fun LockedSectionButton(locked: OpAvailability.Locked, label: String, actions: PageActions) {
    GatedButton(
        availability = locked,
        icon = SwayIcons.Lock.outline,
        label = label,
        iconOnly = false,
        enabled = true,
        onLocked = actions.onLocked,
        onClick = {},
    )
}

/**
 * The confirm dialog of `window.close` (docs/PROTOCOL.md 17.7, UX 15): what TalkBack, Switch Access and a
 * keyboard get instead of the hold. Its yes sends only while the op is available and the desktop is live.
 */
@Composable
internal fun ConfirmCloseDialog(window: Window, facts: PageFacts, actions: PageActions, onCancel: () -> Unit) {
    val sendable = facts.live && facts.pendingId != window.id && facts.availabilityOf(Op.WINDOW_CLOSE) == OpAvailability.Available
    val name = window.title ?: window.appId ?: stringResource(R.string.window_untitled)
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.desktop_close_confirm_title)) },
        text = { Text(stringResource(R.string.desktop_close_confirm_text, name)) },
        confirmButton = {
            TextButton(onClick = actions.window.onConfirmClose, enabled = sendable, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(R.string.desktop_close_confirm), color = if (sendable) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        dismissButton = {
            TextButton(onClick = onCancel, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.cancel)) }
        },
    )
}
