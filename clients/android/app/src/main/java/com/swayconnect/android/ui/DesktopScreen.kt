package com.swayconnect.android.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import com.swayconnect.protocol.replaceUnsafe

/** Screen 2 of docs/DESIGN.md 8.3. It renders server state only. */
@Composable
fun DesktopScreen(state: UiState, viewModel: MainViewModel) {
    // Not the default, the system bars: edge to edge also draws under a display cutout.
    Scaffold(contentWindowInsets = WindowInsets.safeDrawing) { innerPadding ->
        Column(modifier = Modifier.padding(innerPadding).fillMaxSize()) {
            ConnectionBanner(state, onForget = viewModel::forget)
            state.connectionProblem?.let { StatusLine(it) }
            state.controlHint()?.let { StatusLine(it, MaterialTheme.colorScheme.onSurfaceVariant) }
            Column(modifier = Modifier.weight(1f).fillMaxWidth()) {
                val desktop = state.desktop
                when {
                    state.unpaired -> UnpairedNotice(onPairAgain = viewModel::showPairScreen)
                    desktop != null -> DesktopContent(desktop, state, viewModel)
                    state.connection == ConnectionStatus.Backoff -> RetryNotice(onRetry = viewModel::retryNow)
                }
            }
            // Under the list: a line that comes and goes above it would move every tap target.
            state.notice?.let { StatusLine(it) }
        }
    }
}

/** One line of status. A screen reader announces it when it appears. */
@Composable
private fun StatusLine(text: String, color: Color = MaterialTheme.colorScheme.error) {
    Text(
        text,
        color = color,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun ConnectionBanner(state: UiState, onForget: () -> Unit) {
    var confirmingForget by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            state.bannerText(),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.weight(1f).padding(vertical = 12.dp).semantics { liveRegion = LiveRegionMode.Polite },
        )
        // A revoked device has no computer left to show or to forget.
        if (!state.unpaired) OverflowMenu(state, onForget = { confirmingForget = true })
    }
    if (confirmingForget) {
        ForgetDialog(
            onConfirm = {
                confirmingForget = false
                onForget()
            },
            onDismiss = { confirmingForget = false },
        )
    }
}

@Composable
private fun OverflowMenu(state: UiState, onForget: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }) { Text("Menu") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            // Plain text, not disabled menu items: those are too pale to read and are announced as disabled.
            for (line in overflowLines(state)) {
                Text(
                    line,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("Forget this computer") },
                onClick = {
                    open = false
                    onForget()
                },
            )
        }
    }
}

/** The facts of the overflow menu: server name, short fingerprint, granted scopes, the read-only flag. */
private fun overflowLines(state: UiState): List<String> {
    val server = state.server
    val granted = state.granted
    return listOfNotNull(
        server?.let { "Computer: ${it.name}" },
        server?.let { "Fingerprint: ${it.shortFingerprint} …" },
        granted?.let { "Scopes: ${it.scopes.joinToString(", ").ifEmpty { "none" }}" },
        granted?.let { if (it.readOnly) "Read-only: yes" else "Read-only: no" },
    )
}

@Composable
private fun ForgetDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onConfirm) { Text("Forget") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        title = { Text("Forget this computer?") },
        text = { Text("The pairing is removed from this phone. Revoke the device on the computer as well.") },
    )
}

@Composable
private fun UnpairedNotice(onPairAgain: () -> Unit) {
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("This device is no longer paired.")
        Button(onClick = onPairAgain) { Text("Pair again") }
    }
}

/** While the app waits between two attempts. The reason stands in the line above. */
@Composable
private fun RetryNotice(onRetry: () -> Unit) {
    Column(modifier = Modifier.padding(horizontal = 16.dp)) {
        Text("Trying again soon.", color = MaterialTheme.colorScheme.onSurfaceVariant)
        TextButton(onClick = onRetry) { Text("Retry now") }
    }
}

@Composable
private fun DesktopContent(desktop: Desktop, state: UiState, viewModel: MainViewModel) {
    val ops = state.granted?.ops.orEmpty()
    val windows = desktop.windows.filter { it.workspaceId == state.selectedWorkspaceId }
    var sheetWindowId by remember { mutableStateOf<Long?>(null) }

    WorkspaceChips(desktop, state, onSelect = viewModel::selectWorkspace)
    if (desktop.truncated) StatusLine("The computer shortened this list.")
    // An empty list alone looks like a failure to load.
    when {
        !desktop.sway.online -> StatusLine("Sway is not running on the computer.", MaterialTheme.colorScheme.onSurfaceVariant)
        windows.isEmpty() -> StatusLine("No windows on this workspace.", MaterialTheme.colorScheme.onSurfaceVariant)
    }
    LazyColumn(modifier = Modifier.fillMaxSize()) {
        items(windows, key = { it.id }) { window ->
            WindowRow(
                window = window,
                pending = state.pendingTargetId == window.id,
                // A control exists only when its operation is in welcome.ops.
                onFocus = if (Op.WINDOW_FOCUS in ops) ({ viewModel.focusWindow(window.id) }) else null,
                onLongPress = if (Op.WINDOW_FULLSCREEN in ops) ({ sheetWindowId = window.id }) else null,
            )
        }
    }

    val sheetWindow = desktop.windows.firstOrNull { it.id == sheetWindowId }
    if (sheetWindow != null) {
        FullscreenSheet(
            title = windowTitle(sheetWindow),
            onEnter = {
                sheetWindowId = null
                viewModel.enterFullscreen(sheetWindow.id)
            },
            onLeave = {
                sheetWindowId = null
                viewModel.leaveFullscreen(sheetWindow.id)
            },
            onDismiss = { sheetWindowId = null },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun WorkspaceChips(desktop: Desktop, state: UiState, onSelect: (Long) -> Unit) {
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        for (workspace in desktop.workspaces) {
            val markColor = workspaceMarkColor(workspace)
            FilterChip(
                selected = workspace.id == state.selectedWorkspaceId,
                onClick = { onSelect(workspace.id) },
                // The fill of a chip means "listed below", not "focused on the computer": the dot says that.
                leadingIcon = if (markColor != null) ({ MarkDot(markColor) }) else null,
                label = {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(workspaceLabel(workspace))
                        if (state.pendingTargetId == workspace.id) Spinner()
                    }
                },
            )
        }
    }
}

/** Urgent before focused. Each mark keeps its word in the label, so colour is never the only sign. */
@Composable
private fun workspaceMarkColor(workspace: Workspace): Color? =
    when {
        workspace.urgent -> MaterialTheme.colorScheme.error
        workspace.focused -> MaterialTheme.colorScheme.primary
        else -> null
    }

@Composable
private fun MarkDot(color: Color) {
    Box(modifier = Modifier.size(8.dp).background(color, CircleShape))
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun WindowRow(window: Window, pending: Boolean, onFocus: (() -> Unit)?, onLongPress: (() -> Unit)?) {
    val rowColor = if (window.focused) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent
    Row(
        modifier =
            Modifier.fillMaxWidth()
                .background(rowColor)
                .combinedClickable(
                    enabled = onFocus != null || onLongPress != null,
                    onClickLabel = "Focus this window",
                    onLongClickLabel = "Fullscreen options",
                    onLongClick = onLongPress,
                    onClick = { onFocus?.invoke() },
                )
                .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (window.urgent) MarkDot(MaterialTheme.colorScheme.error)
        Column(modifier = Modifier.weight(1f)) {
            Text(
                windowTitle(window),
                fontWeight = if (window.focused) FontWeight.SemiBold else null,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(windowDetail(window), style = MaterialTheme.typography.bodySmall)
        }
        if (pending) Spinner()
    }
}

@Composable
private fun Spinner() {
    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FullscreenSheet(title: String, onEnter: () -> Unit, onLeave: () -> Unit, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            TextButton(onClick = onEnter) { Text("Fullscreen on") }
            TextButton(onClick = onLeave) { Text("Fullscreen off") }
        }
    }
}

// Titles and application ids come from applications: plain text only, unsafe characters replaced.

private fun windowTitle(window: Window): String =
    replaceUnsafe(window.title ?: window.appId ?: "Window ${window.id}")

private fun windowDetail(window: Window): String =
    listOfNotNull(
        window.appId?.let(::replaceUnsafe),
        "focused".takeIf { window.focused },
        "urgent".takeIf { window.urgent },
        "floating".takeIf { window.floating },
        "fullscreen".takeIf { window.fullscreen },
    ).joinToString(" · ")
