package com.swayconnect.android.ui.keys

import com.swayconnect.android.ui.state.KeysRefusal
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.focusedWindowId
import com.swayconnect.android.ui.state.launcherOffered
import com.swayconnect.protocol.InputKey
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.KeyModifier
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import java.util.Locale

// Pure rules of the Keys area (docs/UX.md 9). No Compose import, so the JVM tests cover them.

/**
 * The app ids of the terminals UX 9.2 names (alacritty, foot, kitty, wezterm, gnome-terminal, konsole, xterm),
 * lower case, with the reverse-domain ids they report on Wayland. Exact matches only: a guess could miss a hold.
 */
private val TERMINAL_APP_IDS = setOf(
    "alacritty", "foot", "footclient", "kitty", "wezterm", "org.wezfurlong.wezterm", "gnome-terminal",
    "gnome-terminal-server", "org.gnome.terminal", "konsole", "org.kde.konsole", "xterm",
)

/** A known terminal: Enter runs commands, so "Send + ⏎" needs a 400 ms hold and the target card says so. */
fun isKnownTerminal(appId: String?): Boolean = appId?.lowercase(Locale.ROOT) in TERMINAL_APP_IDS

/** The target is a window of a known terminal. */
fun targetIsTerminal(state: UiState): Boolean {
    val target = state.keys?.target as? InputTarget.Window ?: return false
    return isKnownTerminal(state.desktop?.windows?.firstOrNull { it.id == target.windowId }?.appId)
}

/** One `input.key` request. */
data class KeyPress(val key: InputKey, val modifiers: Set<KeyModifier> = emptySet())

/** A chip of the special keys row. */
sealed interface SpecialKey {
    data class Press(val key: InputKey) : SpecialKey

    /** Ctrl, Alt, Super: armed until the next key goes (UX 9.2). */
    data class Sticky(val modifier: KeyModifier) : SpecialKey
}

private fun named(wire: String): InputKey = checkNotNull(InputKey.of(wire)) { "input.key takes $wire" }

/** The special keys row of UX 9.1, in its order. */
val SPECIAL_KEYS: List<SpecialKey> = listOf(
    SpecialKey.Press(InputKey.ESCAPE),
    SpecialKey.Press(InputKey.TAB),
    SpecialKey.Sticky(KeyModifier.CTRL),
    SpecialKey.Sticky(KeyModifier.ALT),
    SpecialKey.Sticky(KeyModifier.SUPER),
    SpecialKey.Press(named("Left")),
    SpecialKey.Press(named("Up")),
    SpecialKey.Press(named("Down")),
    SpecialKey.Press(named("Right")),
    SpecialKey.Press(InputKey.BACKSPACE),
)

/**
 * The shortcuts row of UX 9.1. Copy and Paste take Shift in a terminal, where Ctrl+C interrupts and Ctrl+V is
 * literal input; [INTERRUPT] is Ctrl+C everywhere.
 */
enum class Shortcut {
    COPY,
    PASTE,
    UNDO,
    INTERRUPT,
    CLEAR_SCREEN,
    ;

    fun press(terminal: Boolean): KeyPress {
        val ctrl = setOf(KeyModifier.CTRL)
        val clipboard = if (terminal) ctrl + KeyModifier.SHIFT else ctrl
        return when (this) {
            COPY -> KeyPress(named("c"), clipboard)
            PASTE -> KeyPress(named("v"), clipboard)
            UNDO -> KeyPress(named("z"), ctrl)
            INTERRUPT -> KeyPress(named("c"), ctrl)
            CLEAR_SCREEN -> KeyPress(named("l"), ctrl)
        }
    }
}

/** The 17.3 state of the two halves of Keys: the composer (`input.text`) and the key rows (`input.key`). */
data class KeysAccess(val text: OpAvailability, val keys: OpAvailability)

/** Null before `welcome`. */
fun keysAccess(state: UiState): KeysAccess? =
    state.granted?.let { KeysAccess(it.availabilityOf(Op.INPUT_TEXT), it.availabilityOf(Op.INPUT_KEY)) }

/** The screen is locked: by the last refusal, or by the newest `session` state. Nothing may be typed then. */
fun keysLocked(state: UiState): Boolean = state.keys?.refusal == KeysRefusal.Locked || state.session?.state?.locked == true

/** A send or key press may go now: the op is available, there is a target, the screen is not locked. */
fun canSend(state: UiState, op: String): Boolean =
    state.granted?.availabilityOf(op) == OpAvailability.Available && state.keys?.target != null && !keysLocked(state)

/** A send, key press or "Back to …" waits for its answer: the composer is read-only and Send spins. */
fun typingBusy(state: UiState): Boolean = state.pendingControl == PendingControl.Typing

/**
 * The other targets the picker offers: the window that has the focus now, when it is not the target, and the
 * launcher while this device holds it (UX 9.2). After `focus_changed` the refusal card offers "Type there" instead.
 */
fun targetChoices(state: UiState): List<InputTarget> {
    val keys = state.keys ?: return emptyList()
    val focused = state.focusedWindowId?.let(InputTarget::Window)
    return listOfNotNull(
        focused.takeIf { it != null && it != keys.target && keys.refusal !is KeysRefusal.FocusChanged },
        InputTarget.Launcher.takeIf { state.launcherOffered && keys.target != InputTarget.Launcher },
    )
}

/** The char index just after the first [deliveredChars] Unicode scalar values of [text]: the delivered part. */
fun deliveredEnd(text: String, deliveredChars: Long): Int {
    val count = text.codePointCount(0, text.length).toLong()
    return text.offsetByCodePoints(0, deliveredChars.coerceIn(0, count).toInt())
}
