package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Desktop

/** The five areas of the pill navigation (docs/UX.md 2). A plain `when` renders them; no navigation library. */
enum class Area { Desktop, Audio, Launch, Keys, More }

/** How a workspace page shows its windows (docs/UX.md 5.6). */
enum class ViewMode { Map, List }

/**
 * The Drive / Browse pill (docs/UX.md 5.5). Drive is the user's choice; whether a settle really sends
 * `workspace.focus` also needs the op to be available ([isDriving]).
 */
enum class DriveMode { Drive, Browse }

/** The one open sheet (docs/UX.md 17.7). M1 kinds only; M2a adds its own. */
sealed interface Sheet {
    /** Long press on a map tile or a window card. */
    data class WindowActions(val windowId: Long) : Sheet

    /** Long press on a workspace card. */
    data class WorkspaceActions(val workspaceId: Long) : Sheet

    /** "+N small windows" on the map of [workspaceId]. */
    data class SmallWindows(val workspaceId: Long) : Sheet

    /** A tap on a tab or stack strip whose segments are too narrow to hit. */
    data class TabGroup(val workspaceId: Long, val windowIds: List<Long>) : Sheet

    /** What the owner runs at the desktop to grant [scope] (docs/UX.md 10.4). */
    data class Locked(val scope: String) : Sheet

    /** Server name, fingerprint, scopes, Retry now, Forget (docs/UX.md 10.6). */
    data object Computer : Sheet
}

/** False when the sheet names a window or workspace that [desktop] no longer has: such a sheet closes. */
internal fun Sheet.fits(desktop: Desktop): Boolean {
    val workspaceIds = desktop.workspaces.map { it.id }.toSet()
    val liveWindowIds = desktop.windows.map { it.id }.toSet()
    return when (this) {
        is Sheet.WindowActions -> windowId in liveWindowIds
        is Sheet.WorkspaceActions -> workspaceId in workspaceIds
        is Sheet.SmallWindows -> workspaceId in workspaceIds
        is Sheet.TabGroup -> workspaceId in workspaceIds && windowIds.any { it in liveWindowIds }
        is Sheet.Locked, Sheet.Computer -> true
    }
}
