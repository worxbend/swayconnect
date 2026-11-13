package com.swayconnect.android.ui.shell

import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/**
 * The word of the status line and the shape of its dot (docs/UX.md 4.1). One value per row of the table that
 * M1 can show; Locked (`session.locked`) comes with M2a.
 */
enum class StatusKind {
    Ready,
    Connecting,
    CheckingKey,
    Offline,
    SwayOffline,
    ScreensOff,
    ReadOnly,

    /** The computer revoked this phone, or no longer knows it. */
    NotPaired,

    /** The computer presented another certificate than the paired one (docs/UX.md 16). */
    WrongKey,

    /** The last attempt was refused for a reason a retry does not change (for example a wrong computer key). */
    Refused,
}

/** The status of [state]. Problems of the computer outrank the read-only flag: they explain more. */
fun statusKind(state: UiState): StatusKind =
    when {
        state.unpaired -> StatusKind.NotPaired
        state.wrongKey -> StatusKind.WrongKey
        else -> when (state.connection) {
            ConnectionStatus.Connecting -> StatusKind.Connecting
            ConnectionStatus.Authenticating -> StatusKind.CheckingKey
            ConnectionStatus.Backoff -> StatusKind.Offline
            ConnectionStatus.Disconnected -> if (state.connectionProblem != null) StatusKind.Refused else StatusKind.Offline
            ConnectionStatus.Ready -> readyKind(state)
        }
    }

private fun readyKind(state: UiState): StatusKind {
    val desktop = state.desktop
    return when {
        desktop?.sway?.online == false -> StatusKind.SwayOffline
        desktop != null && screensOff(desktop) -> StatusKind.ScreensOff
        state.granted?.readOnly == true -> StatusKind.ReadOnly
        else -> StatusKind.Ready
    }
}

/** Every active output has `power == false`. False without outputs (an older server sends none). */
internal fun screensOff(desktop: Desktop): Boolean {
    val active = desktop.outputs.orEmpty().filter { it.active }
    return active.isNotEmpty() && active.none { it.power }
}

/** The op whose state locks a whole area in the nav (UX 4.2: a lock badge on the icon). Null: never locked. */
fun areaGateOp(area: Area): String? =
    when (area) {
        Area.Desktop, Area.More -> null
        Area.Audio -> "audio.set_volume"
        Area.Launch -> "app.launch"
        Area.Keys -> "input.text"
    }

/** The scope that locks [area], or null when it is not locked (or before `welcome`). */
fun lockedScopeOf(state: UiState, area: Area): String? {
    val op = areaGateOp(area) ?: return null
    return (state.granted?.availabilityOf(op) as? OpAvailability.Locked)?.scope
}

/** The op behind the round action button of each area (UX 4.1). Keys toggles a phone-side mode: no op. */
fun roundButtonOp(area: Area): String? =
    when (area) {
        Area.Desktop -> "app.launch"
        Area.Audio -> "audio.set_mute"
        Area.Launch -> "launcher.open"
        Area.Keys -> null
        Area.More -> "session.lock"
    }

/** The op that the Drive / Browse pill needs. */
const val DRIVE_OP = Op.WORKSPACE_FOCUS

/** True when any workspace needs attention: the Desktop nav item gets an `error` dot (UX 4.2). */
fun anyUrgent(state: UiState): Boolean = state.desktop?.workspaces?.any { it.urgent } == true

/**
 * Pill nav geometry in dp (UX 4.2). The selected pill is 16 + icon + 8 + label + 20 wide; every other item is
 * one 48dp square. The label shows only when all of it fits in the bar.
 */
object NavGeometry {
    const val BAR_HEIGHT = 64f
    const val ITEM = 48f
    const val ICON = 24f
    const val BAR_PADDING = 8f
    const val PILL_START = 16f
    const val ICON_GAP = 8f
    const val PILL_END = 20f

    /** Space between the items of the vertical bar. The horizontal bar spreads its items over the free width. */
    const val VERTICAL_GAP = 4f

    fun selectedWidth(labelWidth: Float): Float = PILL_START + ICON + ICON_GAP + labelWidth + PILL_END

    /** True when the selected pill with a label of [labelWidth] dp and the other items fit a bar of [barWidth] dp. */
    fun labelFits(barWidth: Float, labelWidth: Float, itemCount: Int): Boolean {
        val others = (itemCount - 1).coerceAtLeast(0)
        // Each 48dp item already holds 12dp of air around its icon, so the items may sit edge to edge.
        val needed = 2 * BAR_PADDING + selectedWidth(labelWidth) + others * ITEM
        return needed <= barWidth
    }
}
