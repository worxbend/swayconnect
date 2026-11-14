package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Desktop

/**
 * Applies a live state from the server (docs/UX.md 5.5).
 *
 * The shown workspace is kept by id, so a page index found by [shownPageIndex] follows it when workspaces are
 * added or dropped (re-sync). When the shown workspace is gone, the phone lands on the focused one.
 * When the computer's focus moves to another workspace, the phone follows if [followFocus] (the guard of
 * [PagerFacts]) allows it; otherwise the "Computer moved to N" chip names the focused workspace.
 * Following changes only the phone's view: it never sends `workspace.focus`.
 */
fun UiState.withDesktop(next: Desktop, followFocus: Boolean, seenAtMs: Long): UiState {
    val shown = deduplicated(next)
    val focusedBefore = desktop?.focusedWorkspaceId()
    val focusedNow = shown.focusedWorkspaceId()
    val kept = selectedWorkspaceId?.takeIf { id -> shown.workspaces.any { it.id == id } }
    val focusMoved = focusedNow != null && focusedNow != focusedBefore && focusedNow != kept
    val selected =
        when {
            kept == null -> focusedNow ?: shown.workspaces.firstOrNull()?.id
            focusMoved && followFocus -> focusedNow
            else -> kept
        }
    val chipStays = followFocusChip != null && followFocusChip == focusedNow
    val chip = focusedNow.takeIf { it != selected && ((focusMoved && !followFocus) || chipStays) }
    return copy(
        desktop = shown,
        desktopLive = true,
        lastSeenAtMs = seenAtMs,
        selectedWorkspaceId = selected,
        followFocusChip = chip,
        sheet = sheet?.takeIf { it.fits(shown) },
        pendingTargetId = null,
    ).withLaunchSeen()
}

/** Shows a snapshot from the cache before the first connection (docs/UX.md 5.9: dimmed, taps disabled). */
fun UiState.withCachedDesktop(cached: Desktop, savedAtMs: Long): UiState {
    val shown = deduplicated(cached)
    return copy(
        desktop = shown,
        desktopLive = false,
        lastSeenAtMs = savedAtMs,
        selectedWorkspaceId = shown.focusedWorkspaceId() ?: shown.workspaces.firstOrNull()?.id,
    )
}

/** The connection ended: the last state stays on screen as stale, and nothing waits for an answer any more. */
fun UiState.withDesktopStale(): UiState =
    copy(granted = null, desktopLive = false, pendingTargetId = null, followFocusChip = null, drivePaused = false)

/** A tap on the "Computer moved to N" chip: the phone shows that workspace. It sends nothing. */
fun UiState.followChip(): UiState {
    val target = followFocusChip ?: return this
    return copy(selectedWorkspaceId = target, followFocusChip = null)
}

private fun Desktop.focusedWorkspaceId(): Long? = workspaces.firstOrNull { it.focused }?.id

/**
 * Pages and window lists use ids as keys, and Compose throws on a key that comes twice. Sway ids are unique;
 * a broken server must not be able to crash the app.
 */
private fun deduplicated(desktop: Desktop): Desktop =
    desktop.copy(workspaces = desktop.workspaces.distinctBy { it.id }, windows = desktop.windows.distinctBy { it.id })
