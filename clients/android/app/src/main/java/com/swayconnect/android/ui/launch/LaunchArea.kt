package com.swayconnect.android.ui.launch

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.common.StateLine
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.App
import com.swayconnect.protocol.replaceUnsafe

private const val STALE_ALPHA = 0.6f
private val CELL_MIN = 80.dp

/**
 * The Launch area (docs/UX.md 8): a pinned search field, then Recent and All apps in one adaptive grid. A tap
 * starts an app on [targetNumber], the workspace of the context pill (null: the computer's focused workspace); the
 * view model then returns to Desktop, where the ghost tile waits for its window.
 */
@Composable
fun LaunchArea(state: UiState, viewModel: MainViewModel, contentPadding: PaddingValues, targetNumber: Int?) {
    val search = rememberTextFieldState()
    val query = search.text.toString()
    val part = launchPart(state, query)
    var confirming by remember { mutableStateOf<App?>(null) }
    val actions = CellActions(
        onStart = { app -> viewModel.launchApp(app.entryId, targetNumber) },
        onStartOn = { app, number -> viewModel.launchApp(app.entryId, number) },
        onConfirmRequest = { app -> confirming = app },
        onLocked = { scope -> viewModel.openSheet(Sheet.Locked(scope)) },
    )
    val direction = LocalLayoutDirection.current
    val sides = Modifier.padding(start = contentPadding.calculateStartPadding(direction), end = contentPadding.calculateEndPadding(direction))
    Column(Modifier.fillMaxSize().padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (part is LaunchPart.Shown) SearchField(search, sides)
        for (line in stateLines(state, part)) StateLine(line, sides, announce = line == state.notice)
        when (part) {
            is LaunchPart.Shown -> AppGrid(part, query, launchTargets(state.desktop), actions, contentPadding)
            is LaunchPart.Locked ->
                DashedCard(stringResource(R.string.launch_locked), sides, icon = SwayIcons.Lock.outline, onClick = { actions.onLocked(part.scope) })
            LaunchPart.Waiting, LaunchPart.Hidden -> Unit
        }
    }
    RiskyConfirmDialog(confirming, part, onDismiss = { confirming = null }) { app ->
        confirming = null
        actions.onStart(app)
    }
    RejectOnNotice(state.notice)
}

/** UX 17.8: one line, the Search key closes the keyboard, and the keyboard opens only when the field is tapped. */
@Composable
private fun SearchField(state: TextFieldState, modifier: Modifier) {
    val keyboard = LocalSoftwareKeyboardController.current
    TextField(
        state = state,
        modifier = modifier.fillMaxWidth(),
        placeholder = { Text(stringResource(R.string.launch_search)) },
        leadingIcon = { Icon(SwayIcons.Search.outline, contentDescription = null) },
        lineLimits = TextFieldLineLimits.SingleLine,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search, showKeyboardOnFocus = true),
        onKeyboardAction = { keyboard?.hide() },
        shape = MaterialTheme.shapes.extraLarge,
        // A pill: its fill is the edge, so no underline is clipped by the round corners.
        colors = TextFieldDefaults.colors(
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
            disabledIndicatorColor = Color.Transparent,
        ),
    )
}

@Composable
private fun AppGrid(part: LaunchPart.Shown, query: String, targets: List<Int>, actions: CellActions, contentPadding: PaddingValues) {
    // The cell grows with the font scale, so a name like "Alacritty" is never cut mid-word (UX 15).
    val minCell = CELL_MIN * LocalDensity.current.fontScale.coerceAtLeast(1f)
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minCell),
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        // A stale list stays, dimmed; its cells take no touch (UX 13, 16).
        modifier = Modifier.fillMaxSize().graphicsLayer { alpha = if (part.live) 1f else STALE_ALPHA },
    ) {
        if (part.recent.isNotEmpty()) {
            header("recent-header", R.string.launch_recent)
            items(part.recent, key = { "recent:${it.app.entryId}" }) { cell -> AppCellView(cell, targets, actions, Modifier.animateItem()) }
            header("all-header", R.string.launch_all)
        }
        items(part.all, key = { "all:${it.app.entryId}" }) { cell -> AppCellView(cell, targets, actions, Modifier.animateItem()) }
        if (part.all.isEmpty()) {
            item(key = "empty", span = { GridItemSpan(maxLineSpan) }) {
                val text = if (query.isBlank()) stringResource(R.string.launch_no_apps) else stringResource(R.string.launch_no_match, query.trim())
                DashedCard(text, icon = SwayIcons.Search.outline)
            }
        }
    }
}

private fun LazyGridScope.header(key: String, text: Int) {
    item(key = key, span = { GridItemSpan(maxLineSpan) }) {
        Text(
            stringResource(text),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(top = 4.dp).semantics { heading() },
        )
    }
}

/** Waiting, nothing offered, the last refusal or launcher line, a cut list and read-only (UX 8.2, 16). */
@Composable
private fun stateLines(state: UiState, part: LaunchPart): List<String> =
    listOfNotNull(
        stringResource(R.string.launch_waiting).takeIf { part == LaunchPart.Waiting },
        stringResource(R.string.launch_none).takeIf { part == LaunchPart.Hidden },
        state.notice,
        stringResource(R.string.launch_truncated).takeIf { (part as? LaunchPart.Shown)?.truncated == true },
        stringResource(R.string.launch_read_only).takeIf { state.granted?.readOnly == true },
    )

/**
 * The TalkBack way of the risky hold (UX 15). It closes by itself when the app left the list or no longer takes a
 * hold (scope removed, read-only, connection lost).
 */
@Composable
private fun RiskyConfirmDialog(app: App?, part: LaunchPart, onDismiss: () -> Unit, onConfirm: (App) -> Unit) {
    if (app == null) return
    val cell = (part as? LaunchPart.Shown)?.let { it.recent + it.all }?.firstOrNull { it.app.entryId == app.entryId }
    if (cell == null || cell.tap() != LaunchTap.Hold) {
        LaunchedEffect(Unit) { onDismiss() }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.launch_confirm_title, replaceUnsafe(app.name))) },
        text = { Text(stringResource(R.string.launch_confirm_text)) },
        confirmButton = { TextButton(onClick = { onConfirm(cell.app) }) { Text(stringResource(R.string.launch_start)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** `Reject` when a refusal line appears (UX 14). */
@Composable
private fun RejectOnNotice(notice: String?) {
    val haptics = LocalHaptics.current
    var shown by remember { mutableStateOf(notice) }
    LaunchedEffect(notice) {
        if (notice != null && notice != shown) haptics.perform(SwayHaptic.Reject)
        shown = notice
    }
}
