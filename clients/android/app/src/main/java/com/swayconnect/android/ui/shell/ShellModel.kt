package com.swayconnect.android.ui.shell

import com.swayconnect.android.ui.audio.NowPlaying
import com.swayconnect.android.ui.more.KeepAwakeFace
import com.swayconnect.android.ui.more.keepAwakeTile
import com.swayconnect.android.ui.more.minutesUp
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.Screen
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.capsulePlayer
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Device
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/** The word of the status line and the shape of its dot (docs/UX.md 4.1). One value per row of its table. */
enum class StatusKind {
    Ready,
    Connecting,
    CheckingKey,
    Offline,
    SwayOffline,

    /** Topic `session` says the screen is locked. */
    Locked,
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
        state.session?.state?.locked == true -> StatusKind.Locked
        desktop != null && screensOff(desktop) -> StatusKind.ScreensOff
        state.granted?.readOnly == true -> StatusKind.ReadOnly
        else -> StatusKind.Ready
    }
}

/** The `Awake N min` pill of the status line (UX 4.1, 10.2). [minutesLeft] is null when the server sent no time. */
data class AwakePill(val minutesLeft: Int?)

/** The Awake pill while keep-awake is on, from any device; null otherwise. */
fun awakePill(state: UiState): AwakePill? =
    (keepAwakeTile(state)?.face as? KeepAwakeFace.On)?.let { on -> AwakePill(on.leftMs?.let(::minutesUp)) }

/** True while a remote keyboard session is armed on the computer: the `Typing` pill (UX 4.1, 9.1). */
fun typingPill(state: UiState): Boolean = state.session?.state?.input?.active == true

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

/** True when [area] shows the now-playing capsule (UX 4.3): not Audio (it has the full card), not Keys (the IME needs the room). */
fun showsCapsule(area: Area): Boolean = area == Area.Desktop || area == Area.Launch || area == Area.More

/** The now-playing capsule of [state] (UX 4.3), or null when it is hidden. Its buttons follow the `media.*` op state. */
fun capsuleOf(state: UiState): NowPlaying? {
    if (!showsCapsule(state.area)) return null
    val player = state.capsulePlayer ?: return null
    val access = state.granted?.availabilityOf(Op.MEDIA_PLAY_PAUSE) ?: return null
    return NowPlaying(player, access, live = state.media?.live == true, pending = state.pendingControl == PendingControl.Player(player.id))
}

/** A phone volume key (UX 7.2, 17.8). */
enum class VolumeKey { Up, Down, Mute }

/** One volume key press moves the default output by this much (UX 7.2). */
const val VOLUME_KEY_STEP_PCT = 5

/**
 * The output a phone volume key acts on, or null when the key stays the phone's own (UX 7.2): only while the Audio
 * area is shown and the phone may change the default output now. Anything else (no default output, a stale state,
 * a lock, read-only) leaves the key to the phone, so it never silently does nothing.
 */
fun volumeKeyTarget(state: UiState, key: VolumeKey): Device? {
    if (state.screen != Screen.Desktop || state.unpaired || state.wrongKey || state.area != Area.Audio) return null
    val audio = state.audio?.takeIf { it.live } ?: return null
    val sink = audio.defaultSink ?: return null
    val op = if (key == VolumeKey.Mute) Op.AUDIO_SET_MUTE else Op.AUDIO_SET_VOLUME
    return sink.takeIf { state.granted?.availabilityOf(op) == OpAvailability.Available }
}
