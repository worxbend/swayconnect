package com.swayconnect.android.ui.state

import com.swayconnect.protocol.App
import com.swayconnect.protocol.AppEntryId
import com.swayconnect.protocol.AppsState
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/** Scope that launching a `risky` app needs as well (docs/PROTOCOL.md 17.11). */
const val SCOPE_LAUNCH_RISKY = "launch.risky"

/** Topic `apps` as the phone shows it (docs/UX.md 8). */
data class AppsView(
    val state: AppsState,
    /** False once the connection ended. */
    val live: Boolean = true,
) {
    /** The `recent` ids that name an app of the state, in the server's order. */
    val recentApps: List<App>
        get() {
            val byId = state.apps.associateBy { it.entryId }
            return state.recent.distinct().mapNotNull { byId[it] }
        }
}

/**
 * Applies an `apps` state. An entry id goes back to the server as it came, and `app.launch` refuses one of another form
 * before sending: such an entry is dropped here, so the grid never offers it. Ids are unique keys for the grid.
 */
fun UiState.withApps(next: AppsState): UiState {
    val apps = next.apps.filter { AppEntryId.isValid(it.entryId) }.distinctBy { it.entryId }
    return copy(apps = AppsView(next.copy(apps = apps)))
}

/** The connection ended: the last list stays, dimmed; no ghost tile waits for a window any more. */
fun UiState.withAppsStale(): UiState = copy(apps = apps?.copy(live = false), pendingLaunch = null)

/** How [app]'s cell shows (docs/UX.md 8.2): `app.launch` by the table of 17.3, and a risky app locked without `launch.risky`. */
fun Granted.launchAvailability(app: App): OpAvailability {
    val base = availabilityOf(Op.APP_LAUNCH)
    val needsRisky = app.risky && SCOPE_LAUNCH_RISKY !in scopes
    return if (base == OpAvailability.Available && needsRisky) OpAvailability.Locked(SCOPE_LAUNCH_RISKY) else base
}

/**
 * An app the computer started that has no window yet: the ghost tile "Opening Firefox…" (docs/UX.md 8.2). It ends when
 * a window id that was not there at the tap appears on the target workspace, or after [TIMEOUT_MS].
 */
data class PendingLaunch(
    val entryId: String,
    /** As the server sent it; the UI shows it through `replaceUnsafe`. */
    val name: String,
    val icon: String?,
    val target: LaunchTarget,
    val knownWindowIds: Set<Long>,
    /** Monotonic time of the `ok`; it also tells two launches of the same app apart. */
    val startedAtMs: Long,
) {
    companion object {
        const val TIMEOUT_MS = 10_000L
    }
}

/** Where a launched app is expected: the workspace number the user picked, or the computer's focused workspace. */
sealed interface LaunchTarget {
    /** Sway creates the workspace when none has the number, so its id may be unknown at the tap. */
    data class Number(val number: Int) : LaunchTarget

    data class Workspace(val workspaceId: Long) : LaunchTarget
}

/** The workspace id of [target] in [desktop]; null while no workspace has that number. */
fun LaunchTarget.workspaceIdIn(desktop: Desktop?): Long? =
    when (this) {
        is LaunchTarget.Workspace -> workspaceId
        is LaunchTarget.Number -> desktop?.workspaces?.firstOrNull { it.number == number.toLong() }?.id
    }

/** `app.launch` said `ok`: the app returns to Desktop on the target page and draws the ghost tile. */
fun UiState.withLaunchStarted(launch: PendingLaunch): UiState =
    copy(
        area = Area.Desktop,
        sheet = null,
        keys = null,
        pendingLaunch = launch,
        selectedWorkspaceId = launch.target.workspaceIdIn(desktop) ?: selectedWorkspaceId,
    ).withLaunchSeen()

/** Ends the ghost tile when the current desktop has a new window on the target workspace. */
fun UiState.withLaunchSeen(): UiState {
    val launch = pendingLaunch ?: return this
    val workspaceId = launch.target.workspaceIdIn(desktop) ?: return this
    val opened = desktop?.windows.orEmpty().any { it.workspaceId == workspaceId && it.id !in launch.knownWindowIds }
    return if (opened) copy(pendingLaunch = null) else this
}

/** [TIMEOUT_MS][PendingLaunch.TIMEOUT_MS] passed for [launch]: it ends with [notice] when it is still the shown one. */
fun UiState.withLaunchTimedOut(launch: PendingLaunch, notice: String): UiState =
    if (pendingLaunch == launch) copy(pendingLaunch = null, notice = notice) else this
