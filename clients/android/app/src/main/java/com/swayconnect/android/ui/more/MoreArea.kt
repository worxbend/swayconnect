package com.swayconnect.android.ui.more

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.StateLine
import com.swayconnect.android.ui.state.DriveMode
import com.swayconnect.android.ui.state.ServerSummary
import com.swayconnect.android.ui.state.ServerTarget
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/**
 * The More area (docs/UX.md 10.1): quick-settings tiles in two columns, keep awake first, then the computer.
 * Mic and Launcher tiles come with M2b; the settings rows wait for a settings store.
 */
@Composable
fun MoreArea(state: UiState, viewModel: MainViewModel, contentPadding: PaddingValues) {
    val haptics = LocalHaptics.current
    // A tap in the cooldown sends nothing: the line is the phone's own (UX 10.2), shown while the cooldown lasts.
    var cooldownTapped by remember { mutableStateOf(false) }
    val keepAwake = keepAwakeTile(state)
    val inCooldown = keepAwake?.face is KeepAwakeFace.Cooldown
    LaunchedEffect(inCooldown) { if (!inCooldown) cooldownTapped = false }
    val onKeepAwake: (KeepAwakeAction) -> Unit = { action ->
        cooldownTapped = action is KeepAwakeAction.Refuse
        when (action) {
            KeepAwakeAction.TurnOn -> viewModel.keepAwakeOn()
            KeepAwakeAction.TurnOff -> viewModel.keepAwakeOff()
            KeepAwakeAction.ChooseDuration -> viewModel.openSheet(Sheet.KeepAwakeDuration)
            is KeepAwakeAction.Refuse -> haptics.perform(SwayHaptic.Reject)
            is KeepAwakeAction.ShowLocked -> viewModel.openSheet(Sheet.Locked(action.scope))
            KeepAwakeAction.None -> Unit
        }
    }
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(contentPadding).padding(top = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        TileGrid(quickTiles(state, viewModel, keepAwake, onKeepAwake))
        val cooldown = (keepAwake?.face as? KeepAwakeFace.Cooldown)?.takeIf { cooldownTapped }
        for (line in stateLines(state, cooldown)) StateLine(line, announce = line == state.notice)
        Text(
            stringResource(R.string.more_computer),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(top = 8.dp).semantics { heading() },
        )
        ComputerCard(state.server) { viewModel.openSheet(Sheet.Computer) }
    }
    MoreHaptics(state)
}

private typealias Tile = @Composable (Modifier) -> Unit

/**
 * Above this font scale the tiles stand in one column, so a word like "Scratchpad" is never cut (UX 15). At 1.3
 * (Android's fourth step) it already breaks in two columns on a 360dp phone; 1.15 still fits.
 */
private const val LARGE_FONT_SCALE = 1.2f

/** The tiles in the order of UX 10.1 that M2a has; a tile whose op the server does not offer is left out. */
@Composable
private fun quickTiles(state: UiState, viewModel: MainViewModel, keepAwake: KeepAwakeTile?, onKeepAwake: (KeepAwakeAction) -> Unit): List<Tile> {
    val granted = state.granted ?: return emptyList()
    val onLocked = { scope: String -> viewModel.openSheet(Sheet.Locked(scope)) }
    fun gate(op: String, opensSheet: Boolean = false, onAvailable: () -> Unit): GatedTile? =
        granted.availabilityOf(op).takeIf { it != OpAvailability.Unsupported }?.let { GatedTile(it, onAvailable, onLocked, opensSheet) }

    val stale = !state.desktopLive
    val driving = state.drive == DriveMode.Drive
    val drive = gate(Op.WORKSPACE_FOCUS) { viewModel.setDriveMode(if (driving) DriveMode.Browse else DriveMode.Drive) }
    val scratchpad = gate(Op.SCRATCHPAD_SHOW, opensSheet = true) { viewModel.openSheet(Sheet.Scratchpad) }
    val lock = gate(Op.SESSION_LOCK, onAvailable = viewModel::lockComputer)
    val hidden = state.desktop?.scratchpad?.size
    return listOfNotNull(
        keepAwake?.let { tile -> { modifier: Modifier -> KeepAwakeQuickTile(tile, onKeepAwake, modifier) } },
        drive?.let { gated -> { modifier: Modifier -> DriveQuickTile(gated, driving, modifier) } },
        scratchpad?.let { gated -> { modifier: Modifier -> ScratchpadQuickTile(gated, hidden, stale, modifier) } },
        lock?.let { gated -> { modifier: Modifier -> LockQuickTile(gated, state.pendingTargetId == ServerTarget.LOCK, stale, modifier) } },
    )
}

/** Two columns, one above a large font scale (UX 15); the tiles of one row share the taller one's height. */
@Composable
private fun TileGrid(tiles: List<Tile>) {
    val columns = if (LocalDensity.current.fontScale > LARGE_FONT_SCALE) 1 else 2
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        for (row in tiles.chunked(columns)) {
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                for (tile in row) tile(Modifier.weight(1f).fillMaxHeight())
                if (row.size < columns) Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** The last refusal, the cooldown line and the read-only line (UX 10.2, 16); "waiting" before `welcome`. */
@Composable
private fun stateLines(state: UiState, cooldown: KeepAwakeFace.Cooldown?): List<String> =
    listOfNotNull(
        stringResource(R.string.more_waiting).takeIf { state.granted == null },
        state.notice,
        cooldown?.let { stringResource(R.string.keep_awake_available_again, minutesUp(it.leftMs)) },
        stringResource(R.string.more_read_only).takeIf { state.granted?.readOnly == true },
    )

/** The computer as a card: badge, name, short fingerprint. A tap opens the Computer sheet (UX 10.6). */
@Composable
private fun ComputerCard(server: ServerSummary?, onClick: () -> Unit) {
    Surface(onClick = onClick, shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
                Icon(SwayIcons.Desktop.outline, contentDescription = null, modifier = Modifier.size(24.dp))
            }
            Column(Modifier.weight(1f)) {
                Text(server?.name ?: stringResource(R.string.more_computer), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                server?.let {
                    Text(
                        stringResource(R.string.computer_fingerprint, it.shortFingerprint),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

/**
 * Haptics of UX 14 for this area: `ToggleOn` / `ToggleOff` when keep-awake turns on or off after this phone
 * asked (the answer and the `session` state come in either order), `Confirm` when a lock or a Show is answered
 * without a refusal, `Reject` when a refusal line appears.
 */
@Composable
private fun MoreHaptics(state: UiState) {
    val haptics = LocalHaptics.current
    val on = keepAwakeTile(state)?.face is KeepAwakeFace.On
    var waited by remember { mutableStateOf<Long?>(null) }
    var toggleAsked by remember { mutableStateOf(false) }
    var wasOn by remember { mutableStateOf(on) }
    LaunchedEffect(state.pendingTargetId) {
        val ended = waited
        if (state.pendingTargetId == ServerTarget.KEEP_AWAKE) toggleAsked = true
        if (ended != null && ended != ServerTarget.KEEP_AWAKE && state.pendingTargetId == null && state.notice == null) {
            haptics.perform(SwayHaptic.Confirm)
        }
        waited = state.pendingTargetId
    }
    LaunchedEffect(on) {
        if (on != wasOn && toggleAsked) {
            haptics.perform(if (on) SwayHaptic.ToggleOn else SwayHaptic.ToggleOff)
            toggleAsked = false
        }
        wasOn = on
    }
    var shownNotice by remember { mutableStateOf(state.notice) }
    LaunchedEffect(state.notice) {
        if (state.notice != null && state.notice != shownNotice) {
            haptics.perform(SwayHaptic.Reject)
            toggleAsked = false
        }
        shownNotice = state.notice
    }
}
