package com.swayconnect.android.ui.state

import com.swayconnect.protocol.ErrorCode
import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.InputRefusal
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.KeyModifier

/** The Keys area (docs/UX.md 9.2). Its text lives in the composer, never here. */
data class KeysView(
    /** Captured when Keys opens. Null when the computer had no focused window then: nothing to type into. */
    val target: InputTarget?,
    /** `BasicSecureTextField` and text dropped on rotation (UX 9.2). */
    val privateMode: Boolean = false,
    /** Ctrl, Alt, Super armed for the next key. */
    val sticky: Set<KeyModifier> = emptySet(),
    val refusal: KeysRefusal? = null,
    /** The newest text send, for the composer to drop the delivered part. */
    val typed: Typed? = null,
    /**
     * The launcher target went while no window had the focus: sway marks none while a layer surface such as fuzzel
     * holds the keyboard, and the desktop state that shows the focus again can come after. The next focused window
     * becomes the target ([withKeysFollowingDesktop]).
     */
    val awaitingFocus: Boolean = false,
)

/** Why typing stopped, as UX 9.2 shows it. */
sealed interface KeysRefusal {
    /** "Focus moved to Firefox." [focusedWindowId] is null when no window has the focus now. */
    data class FocusChanged(val focusedWindowId: Long?) : KeysRefusal

    /** The composer is disabled until a `session` state says the screen is unlocked. */
    data object Locked : KeysRefusal

    data object KeyboardGrabbed : KeysRefusal

    /** The target fell back to the focused window; nothing was sent again. */
    data object LauncherClosed : KeysRefusal
}

/**
 * The outcome of one Send. [serial] changes with every send, so the composer drops [deliveredChars] Unicode scalar
 * values from the front of its text exactly once; the rest stays in the field ("Sent 16 of 40 characters.").
 */
data class Typed(val serial: Long, val deliveredChars: Long, val totalChars: Long)

/** The window that has the focus on the computer, from the newest desktop state. */
val UiState.focusedWindowId: Long?
    get() = desktop?.windows?.firstOrNull { it.focused }?.id

/** Target "Launcher (fuzzel)" is offered only while this device holds a running launcher (17.19 item 19). */
val UiState.launcherOffered: Boolean
    get() = session?.state?.launcher?.let { it.open && it.holder == Holder.THIS_DEVICE } == true

/**
 * Shows [area]. Opening Keys captures its target: [windowId] when Desktop's "Type" picked one, else the focused
 * window. Leaving Keys drops its state, so the next visit captures a fresh target.
 */
fun UiState.withArea(area: Area, windowId: Long? = null): UiState {
    val next =
        when {
            area != Area.Keys -> null
            windowId != null -> KeysView(InputTarget.Window(windowId))
            this.area == Area.Keys -> keys
            else -> KeysView(focusedWindowId?.let(InputTarget::Window))
        }
    return copy(area = area, sheet = null, keys = next)
}

/** The target picker. The launcher only while [launcherOffered]. */
fun UiState.withKeysTarget(target: InputTarget): UiState {
    val view = keys ?: return this
    if (target == InputTarget.Launcher && !launcherOffered) return this
    return copy(keys = view.copy(target = target, refusal = null, awaitingFocus = false))
}

/** `launcher.open` made this device the holder: Keys opens with the launcher as its target (docs/UX.md 8.2). */
fun UiState.withLauncherTarget(): UiState = copy(area = Area.Keys, sheet = null, keys = KeysView(InputTarget.Launcher))

fun UiState.withPrivateMode(enabled: Boolean): UiState = keys?.let { copy(keys = it.copy(privateMode = enabled)) } ?: this

/** A tap on Ctrl, Alt or Super: arms it, or disarms it when armed. */
fun UiState.withModifierToggled(modifier: KeyModifier): UiState {
    val view = keys ?: return this
    val sticky = if (modifier in view.sticky) view.sticky - modifier else view.sticky + modifier
    return copy(keys = view.copy(sticky = sticky))
}

/** The next key was sent: the armed modifiers went with it. */
fun UiState.withModifiersReleased(): UiState = keys?.let { copy(keys = it.copy(sticky = emptySet())) } ?: this

/** One Send ended after [deliveredChars] of [totalChars]; [refusal] is why it stopped early, if it did. */
fun UiState.withTyped(deliveredChars: Long, totalChars: Long, refusal: InputRefusal?): UiState {
    val view = keys ?: return this
    val typed = Typed((view.typed?.serial ?: 0) + 1, deliveredChars, totalChars)
    val withTyped = copy(keys = view.copy(typed = typed))
    return if (refusal == null) withTyped else withTyped.withInputRefusal(refusal)
}

/** `input.text` or `input.key` was refused with a code of [InputRefusal.CODES]. */
fun UiState.withInputRefusal(refusal: InputRefusal): UiState {
    val view = keys ?: return this
    val next =
        when (refusal.code) {
            ErrorCode.FOCUS_CHANGED -> view.copy(refusal = KeysRefusal.FocusChanged(refusal.focusedWindowId))
            ErrorCode.LOCKED -> view.copy(refusal = KeysRefusal.Locked)
            ErrorCode.KEYBOARD_GRABBED -> view.copy(refusal = KeysRefusal.KeyboardGrabbed)
            ErrorCode.LAUNCHER_CLOSED -> view.copy(refusal = KeysRefusal.LauncherClosed).fallingBackTo(focusedWindowId)
            else -> return this
        }
    return copy(keys = next)
}

/** "Type there" after `focus_changed`: the window that has the focus now becomes the target. */
fun UiState.withTargetFromRefusal(): UiState {
    val view = keys ?: return this
    val moved = view.refusal as? KeysRefusal.FocusChanged ?: return this
    return copy(keys = view.copy(target = moved.focusedWindowId?.let(InputTarget::Window), refusal = null, awaitingFocus = false))
}

fun UiState.withKeysRefusalCleared(): UiState = keys?.let { copy(keys = it.copy(refusal = null)) } ?: this

/**
 * Follows a new `session` state: a launcher target that this device no longer holds falls back to the focused window,
 * and the locked card goes once the screen is known to be unlocked.
 */
fun UiState.withKeysFollowingSession(): UiState {
    val view = keys ?: return this
    val launcherGone = view.target == InputTarget.Launcher && session?.state?.launcher != null && !launcherOffered
    val unlocked = view.refusal == KeysRefusal.Locked && session?.state?.locked == false
    if (!launcherGone && !unlocked) return this
    val followed = if (launcherGone) view.fallingBackTo(focusedWindowId) else view
    return copy(keys = followed.copy(refusal = if (unlocked) null else view.refusal))
}

/** Follows a new `desktop` state: a target that waits for the focus after the launcher closed takes the focused window. */
fun UiState.withKeysFollowingDesktop(): UiState {
    val view = keys?.takeIf { it.awaitingFocus } ?: return this
    val focused = focusedWindowId ?: return this
    return copy(keys = view.copy(target = InputTarget.Window(focused), awaitingFocus = false))
}

/** The launcher target is gone: the focused window takes over, or the next one when none has the focus yet. */
private fun KeysView.fallingBackTo(focusedWindowId: Long?): KeysView =
    copy(target = focusedWindowId?.let(InputTarget::Window), awaitingFocus = focusedWindowId == null)
