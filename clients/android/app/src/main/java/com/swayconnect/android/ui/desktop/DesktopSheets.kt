package com.swayconnect.android.ui.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.common.WindowCard
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.Window

/**
 * The Desktop area's sheets (docs/UX.md 5.8, 6, 17.7): Window actions, Move to, Workspace actions, Small windows
 * and Tab group, plus the close confirm dialog. The shell renders Computer and Locked. Each action closes the
 * sheet; a sheet whose window or workspace leaves the state is closed by the reducer.
 *
 * @param smallWindowIds the ids the map reported with "+N small windows"; empty after a process restart,
 *   then the sheet lists every window of the workspace.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DesktopSheet(sheet: Sheet?, desktop: Desktop, facts: PageFacts, actions: PageActions, smallWindowIds: List<Long>, onClose: () -> Unit) {
    // "Some tools (autotiling) may change this." shows in the first Window sheet of this run only (UX 5.8).
    val layoutNoteShown = rememberSaveable { mutableStateOf(false) }
    if (sheet is Sheet.ConfirmClose) {
        desktop.windows.firstOrNull { it.id == sheet.windowId }?.let { ConfirmCloseDialog(it, facts, actions, onCancel = onClose) }
        return
    }
    if (!isDesktopSheet(sheet)) return
    ModalBottomSheet(onDismissRequest = onClose, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(
            modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val closing = closingActions(actions, onClose)
            when (sheet) {
                is Sheet.WindowActions -> desktop.windows.firstOrNull { it.id == sheet.windowId }?.let { window ->
                    val showNote = remember(sheet.windowId) { !layoutNoteShown.value }
                    LaunchedEffect(Unit) { layoutNoteShown.value = true }
                    WindowSheet(window, desktop, facts, closing, showNote)
                }
                is Sheet.MoveTo -> desktop.windows.firstOrNull { it.id == sheet.windowId }?.let { MoveSheet(it, desktop, facts, closing) }
                is Sheet.WorkspaceActions -> WorkspaceSheet(sheet.workspaceId, desktop, facts, closing)
                is Sheet.SmallWindows -> {
                    val ids = smallWindowIds.toSet()
                    val list = desktop.windows.filter { it.workspaceId == sheet.workspaceId && (ids.isEmpty() || it.id in ids) }
                    WindowListSheet(stringResource(R.string.desktop_sheet_small), list, facts, closing)
                }
                is Sheet.TabGroup -> {
                    val byId = desktop.windows.associateBy { it.id }
                    WindowListSheet(stringResource(R.string.desktop_sheet_group), sheet.windowIds.mapNotNull(byId::get), facts, closing)
                }
                else -> Unit
            }
        }
    }
}

private fun isDesktopSheet(sheet: Sheet?): Boolean =
    sheet is Sheet.WindowActions || sheet is Sheet.MoveTo || sheet is Sheet.WorkspaceActions || sheet is Sheet.SmallWindows || sheet is Sheet.TabGroup

/** The same actions, each closing the sheet first. Opening another sheet replaces this one. */
private fun closingActions(actions: PageActions, onClose: () -> Unit) =
    PageActions(
        onFocusWindow = { id ->
            onClose()
            actions.onFocusWindow(id)
        },
        onWindowSheet = actions.onWindowSheet,
        onFullscreen = { window ->
            onClose()
            actions.onFullscreen(window)
        },
        onSetView = actions.onSetView,
        onShowOnComputer = { id ->
            onClose()
            actions.onShowOnComputer(id)
        },
        onFollowChip = actions.onFollowChip,
        onDismissListHint = actions.onDismissListHint,
        onOpenGroup = actions.onOpenGroup,
        onSmallWindows = actions.onSmallWindows,
        onOpenLaunch = {
            onClose()
            actions.onOpenLaunch()
        },
        onLocked = actions.onLocked,
        window = actions.window.closing(onClose),
    )

/** UX 5.8: header, Focus, Full, Float, Scratchpad, the move pills, the container's layout, Close. */
@Composable
private fun WindowSheet(window: Window, desktop: Desktop, facts: PageFacts, actions: PageActions, showLayoutNote: Boolean) {
    WindowSheetHeader(window, desktop)
    HorizontalDivider()
    WindowSheetActions(window, desktop, facts, actions, showLayoutNote)
}

/** The Focused card's Move (UX 5.7): the window, then the move pills. */
@Composable
private fun MoveSheet(window: Window, desktop: Desktop, facts: PageFacts, actions: PageActions) {
    WindowSheetHeader(window, desktop)
    HorizontalDivider()
    MoveSection(window, desktop, enabled = facts.live && facts.pendingId != window.id, facts, actions)
}

@Composable
private fun WindowSheetHeader(window: Window, desktop: Desktop) {
    val workspace = desktop.workspaces.firstOrNull { it.id == window.workspaceId }
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
            AppIcon(appId = window.appId, iconHash = window.icon, size = 28.dp)
        }
        Column(Modifier.weight(1f)) {
            Text(
                text = window.title ?: window.appId ?: stringResource(R.string.window_untitled),
                style = MaterialTheme.typography.titleMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading() },
            )
            val detail = listOfNotNull(
                window.appId,
                workspace?.let { stringResource(R.string.desktop_sheet_on_workspace, cardText(it).label()) },
                stringResource(if (window.floating) R.string.desktop_sheet_floating else R.string.desktop_sheet_tiled),
            ).joinToString(" · ")
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Long press on a workspace card: "Show on computer" and "Move focused window here" (UX 5.4). */
@Composable
private fun WorkspaceSheet(workspaceId: Long, desktop: Desktop, facts: PageFacts, actions: PageActions) {
    val workspace = desktop.workspaces.firstOrNull { it.id == workspaceId } ?: return
    Text(
        stringResource(R.string.desktop_sheet_workspace, cardText(workspace).label()),
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.semantics { heading() },
    )
    if (workspace.focused) {
        StateLine(stringResource(R.string.desktop_on_screen))
    } else {
        GatedButton(
            availability = facts.availabilityOf(Op.WORKSPACE_FOCUS),
            icon = SwayIcons.Desktop.outline,
            label = stringResource(R.string.desktop_show_on_computer),
            iconOnly = false,
            enabled = facts.live && facts.pendingId != workspace.id,
            onLocked = actions.onLocked,
            onClick = { actions.onShowOnComputer(workspace.id) },
        )
    }
    val focused = desktop.windows.firstOrNull { it.focused }
    if (focused != null && focused.workspaceId != workspace.id) {
        GatedButton(
            availability = facts.availabilityOf(Op.WINDOW_MOVE),
            icon = SwayIcons.Move.outline,
            label = stringResource(R.string.desktop_move_focused_here),
            iconOnly = false,
            enabled = facts.live && facts.pendingId != focused.id,
            onLocked = actions.onLocked,
            onClick = { actions.window.onMoveToWorkspace(focused.id, workspace.id) },
        )
    }
}

/** Small windows and Tab group: the windows as cards. Tap focuses, long press opens the Window sheet. */
@Composable
private fun WindowListSheet(title: String, windows: List<Window>, facts: PageFacts, closing: PageActions) {
    Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
    if (windows.isEmpty()) StateLine(stringResource(R.string.desktop_sheet_gone))
    for (window in windows) {
        WindowCard(
            window = window,
            onClick = { tapWindow(window.id, facts, closing) },
            onClickLabel = stringResource(R.string.desktop_focus_window),
            onLongClick = { closing.onWindowSheet(window.id) },
            enabled = facts.live,
            pending = facts.pendingId == window.id,
        )
    }
}
