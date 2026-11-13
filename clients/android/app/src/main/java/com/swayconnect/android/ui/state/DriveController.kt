package com.swayconnect.android.ui.state

/**
 * Decides when a pager settle drives the desktop (docs/UX.md 5.5, Drive mode). Pure: the caller passes the time.
 *
 * Only a user-initiated settle drives: one that follows a finger drag of the pager or the card row, or a card
 * tap. Follow-focus scrolls, re-sync jumps, keep-in-view scrolls and accessibility scroll actions never call
 * [userDrag] or [cardTap], so their settles send nothing. A settle must hold for [holdMs] before it is sent, and a fling
 * across several pages sends once.
 *
 * Not thread-safe: call it from one thread or under one lock.
 */
class DriveController(private val holdMs: Long = HOLD_MS) {
    private var userInitiated = false
    private var candidate: Long? = null
    private var settledAtMs = 0L

    /** The page shown when the drag began; a drag that springs back to it moved nothing. Null for a card tap. */
    private var gestureStartId: Long? = null

    /** A finger drag of the pager or the card row began while [shownId] was shown. A waiting settle is dropped. */
    fun userDrag(shownId: Long?) {
        userInitiated = true
        candidate = null
        gestureStartId = shownId
    }

    /** A card was tapped: an explicit request, so it drives even when it names the page already shown. */
    fun cardTap() {
        userInitiated = true
        candidate = null
        gestureStartId = null
    }

    /**
     * The pager settled on [workspaceId]. It becomes the candidate when a user gesture led here, or when it
     * replaces a candidate that still waits (the same fling settling once more). A drag that ends on the page it
     * started from is dropped: the user moved nothing on the phone.
     */
    fun settled(workspaceId: Long, nowMs: Long) {
        if (!userInitiated && candidate == null) return
        val springBack = userInitiated && gestureStartId != null && workspaceId == gestureStartId
        userInitiated = false
        gestureStartId = null
        if (springBack) {
            candidate = null
            return
        }
        candidate = workspaceId
        settledAtMs = nowMs
    }

    /**
     * The workspace to send `workspace.focus` for, once the candidate has held for [holdMs]; null otherwise.
     * Returns it at most once. Nothing is sent when the candidate is no longer the shown page or no longer a
     * page at all, or when it is already the focused workspace ([focusedId]).
     */
    fun due(nowMs: Long, shownId: Long?, focusedId: Long?, pageIds: Collection<Long>): Long? {
        val id = candidate ?: return null
        if (nowMs - settledAtMs < holdMs) return null
        candidate = null
        return id.takeIf { it == shownId && it != focusedId && it in pageIds }
    }

    companion object {
        const val HOLD_MS = 250L
    }
}

/**
 * The follow-focus guard (docs/UX.md 5.5): the pager follows the computer's focus to another workspace only
 * when no finger is down, no own request waits, the pager was not touched in the last [QUIET_MS] and touch
 * exploration (TalkBack) is off. Otherwise the "Computer moved to N" chip shows instead.
 */
data class PagerFacts(val fingerDown: Boolean = false, val lastTouchAtMs: Long? = null, val touchExploration: Boolean = false) {
    fun allowsFollow(ownRequestPending: Boolean, nowMs: Long): Boolean =
        !fingerDown && !ownRequestPending && !touchExploration && lastTouchAtMs.let { it == null || nowMs - it >= QUIET_MS }

    companion object {
        const val QUIET_MS = 1_500L
    }
}
