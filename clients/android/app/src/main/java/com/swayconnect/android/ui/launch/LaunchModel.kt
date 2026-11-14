package com.swayconnect.android.ui.launch

import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.launchAvailability
import com.swayconnect.protocol.App
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.MAX_WORKSPACE_NUMBER
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.Workspace

// Pure rules of the Launch area (docs/UX.md 8). No Compose import, so the JVM tests cover them.

/** What a touch on an app cell does. */
sealed interface LaunchTap {
    /** A tap starts the app. */
    data object Send : LaunchTap

    /** A risky app with `launch.risky`: a 600 ms hold, or a confirm dialog under TalkBack (UX 8.2, 15). */
    data object Hold : LaunchTap

    /** Without `launch.risky` a risky app shows a lock and opens the Locked sheet. */
    data class ShowLocked(val scope: String) : LaunchTap

    /** Read-only, stale, or this app's start waits for its answer: the cell takes no touch. */
    data object None : LaunchTap
}

/** One app cell of UX 8.1. */
data class AppCell(
    val app: App,
    /** `app.launch` for this app: a risky app is [OpAvailability.Locked] without `launch.risky`. */
    val access: OpAvailability,
    /** False once the connection ended: the last list stays, dimmed. */
    val live: Boolean,
    /** This app's `app.launch` waits for its answer. */
    val pending: Boolean,
) {
    fun tap(): LaunchTap =
        when {
            access is OpAvailability.Locked -> LaunchTap.ShowLocked(access.scope)
            access != OpAvailability.Available || !live || pending -> LaunchTap.None
            app.risky -> LaunchTap.Hold
            else -> LaunchTap.Send
        }

    /**
     * Read-only (or the op otherwise not available): the cell takes no touch and looks disabled (UX 4.1). A stale
     * list is dimmed as a whole and a pending start shows its spinner, so neither dims the cell itself.
     */
    fun looksDisabled(): Boolean = live && !pending && tap() == LaunchTap.None
}

/** The body of the Launch area. */
sealed interface LaunchPart {
    /** Before `welcome`, or the server offers launching but sent no `apps` state yet. */
    data object Waiting : LaunchPart

    /** The server offers no `app.launch`. */
    data object Hidden : LaunchPart

    data class Locked(val scope: String) : LaunchPart

    /**
     * The grid. [recent] is empty while a search runs; [all] is the search result then. An empty [all] with a
     * non-blank query is "No app matches"; with a blank one the computer lists no apps.
     */
    data class Shown(val recent: List<AppCell>, val all: List<AppCell>, val truncated: Boolean, val live: Boolean) : LaunchPart
}

/** The Launch area of [state] for the search text [query]. */
fun launchPart(state: UiState, query: String): LaunchPart {
    val granted = state.granted ?: return LaunchPart.Waiting
    when (val access = granted.availabilityOf(Op.APP_LAUNCH)) {
        OpAvailability.Unsupported -> return LaunchPart.Hidden
        is OpAvailability.Locked -> return LaunchPart.Locked(access.scope)
        else -> Unit
    }
    val view = state.apps ?: return LaunchPart.Waiting
    val cell = { app: App ->
        AppCell(app, granted.launchAvailability(app), view.live, state.pendingControl == PendingControl.Launch(app.entryId))
    }
    val searching = query.isNotBlank()
    return LaunchPart.Shown(
        recent = if (searching) emptyList() else view.recentApps.map(cell),
        all = FuzzyMatch.filter(view.state.apps, query).map(cell),
        truncated = view.state.truncated,
        live = view.live,
    )
}

/**
 * The workspace numbers an app can be started on: those of the current workspaces that `app.launch` accepts
 * (0 to 999), in order. A named workspace without a number cannot be a target.
 */
fun launchTargets(desktop: Desktop?): List<Int> =
    desktop?.workspaces.orEmpty().mapNotNull { it.launchNumber() }.distinct().sorted()

/**
 * The number the context pill shows (UX 8.1 "On 3"): the picked [number], or for the default the computer's
 * focused workspace. Null when the default has no number to show.
 */
fun shownTarget(number: Int?, desktop: Desktop?): Int? =
    number ?: desktop?.workspaces?.firstOrNull { it.focused }?.launchNumber()

/** The number of the workspace shown on the phone, for Desktop's "+" (UX 4.1); null when it has none. */
fun shownPageTarget(state: UiState): Int? =
    state.desktop?.workspaces?.firstOrNull { it.id == state.selectedWorkspaceId }?.launchNumber()

private fun Workspace.launchNumber(): Int? = number?.takeIf { it in 0..MAX_WORKSPACE_NUMBER }?.toInt()
