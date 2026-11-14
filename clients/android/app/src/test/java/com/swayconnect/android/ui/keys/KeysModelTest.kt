package com.swayconnect.android.ui.keys

import android.view.inputmethod.EditorInfo
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.withArea
import com.swayconnect.android.ui.state.withInputRefusal
import com.swayconnect.android.ui.state.withSession
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.ErrorCode
import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.InputKey
import com.swayconnect.protocol.InputRefusal
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.KeepAwake
import com.swayconnect.protocol.KeyModifier
import com.swayconnect.protocol.Launcher
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.SessionState
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Keys area rules of docs/UX.md 9. */
class KeysModelTest {
    private val typing = Granted(ops = setOf(Op.INPUT_TEXT, Op.INPUT_KEY, Op.WINDOW_FOCUS), scopes = listOf("input.text", "input.keys"), readOnly = false)
    private val keys = UiState(pair = PairForm(deviceName = "Pixel"), granted = typing, desktop = desktop(focused = 7)).withArea(Area.Keys)

    @Test
    fun `a read-only computer turns the composer placeholder into the reason typing is off`() {
        val look = ComposerLook(canType = true, canPressReturn = true, busy = false, terminal = false, privateMode = false, readOnly = false)
        assertEquals(R.string.keys_field, look.hint)
        assertEquals(R.string.keys_private_field, look.copy(privateMode = true).hint)
        val readOnly = look.copy(canType = false, canPressReturn = false, readOnly = true)
        assertEquals(R.string.keys_typing_off, readOnly.hint)
        assertEquals("private mode shows the reason too", R.string.keys_typing_off, readOnly.copy(privateMode = true).hint)
        assertEquals("locked or no target is not read-only", R.string.keys_field, look.copy(canType = false).hint)
    }

    @Test
    fun `the terminals of UX 9_2 by their Wayland app ids, exact matches only`() {
        for (id in listOf("Alacritty", "foot", "kitty", "org.wezfurlong.wezterm", "org.gnome.Terminal", "org.kde.konsole", "xterm")) {
            assertTrue(id, isKnownTerminal(id))
        }
        for (id in listOf("firefox", "alacritty-helper", "terminal", "")) assertFalse(id, isKnownTerminal(id))
        assertFalse(isKnownTerminal(null))
        assertTrue("the target window is foot", targetIsTerminal(keys))
        assertFalse("the launcher is no terminal", targetIsTerminal(keys.copy(keys = keys.keys?.copy(target = InputTarget.Launcher))))
    }

    @Test
    fun `the special keys row has no Enter and no function key, and three sticky modifiers`() {
        val wire = SPECIAL_KEYS.map {
            when (it) {
                is SpecialKey.Press -> it.key.wire
                is SpecialKey.Sticky -> it.modifier.wire
            }
        }
        assertEquals(listOf("Escape", "Tab", "ctrl", "alt", "super", "Left", "Up", "Down", "Right", "BackSpace"), wire)
        assertFalse(SPECIAL_KEYS.any { it is SpecialKey.Press && (it.key == InputKey.RETURN || it.key.isFunctionKey) })
    }

    @Test
    fun `copy and paste take shift in a terminal, Ctrl+C stays an interrupt`() {
        val ctrl = setOf(KeyModifier.CTRL)
        val ctrlShift = setOf(KeyModifier.CTRL, KeyModifier.SHIFT)
        assertEquals(KeyPress(InputKey.of("c")!!, ctrl), Shortcut.COPY.press(terminal = false))
        assertEquals(KeyPress(InputKey.of("c")!!, ctrlShift), Shortcut.COPY.press(terminal = true))
        assertEquals(KeyPress(InputKey.of("v")!!, ctrlShift), Shortcut.PASTE.press(terminal = true))
        assertEquals(KeyPress(InputKey.of("c")!!, ctrl), Shortcut.INTERRUPT.press(terminal = true))
        assertEquals(KeyPress(InputKey.of("l")!!, ctrl), Shortcut.CLEAR_SCREEN.press(terminal = true))
        assertEquals(KeyPress(InputKey.of("z")!!, ctrl), Shortcut.UNDO.press(terminal = false))
    }

    @Test
    fun `typing needs the op, a target and an unlocked screen`() {
        assertTrue(canSend(keys, Op.INPUT_TEXT))
        assertTrue(canSend(keys, Op.INPUT_KEY))
        assertFalse("nothing to type into", canSend(keys.copy(keys = keys.keys?.copy(target = null)), Op.INPUT_TEXT))
        val refusedLocked = keys.withInputRefusal(InputRefusal(ErrorCode.LOCKED, 0, null))
        assertTrue(keysLocked(refusedLocked))
        assertFalse(canSend(refusedLocked, Op.INPUT_KEY))
        val sessionLocked = keys.withSession(session(locked = true), 0)
        assertTrue("session says locked before any refusal", keysLocked(sessionLocked))
        assertFalse(canSend(sessionLocked, Op.INPUT_TEXT))
        val readOnly = keys.copy(granted = typing.copy(readOnly = true, availability = mapOf(Op.INPUT_TEXT to OpAvailability.ReadOnly)))
        assertFalse(canSend(readOnly, Op.INPUT_TEXT))
        val textLocked = keys.copy(granted = Granted(setOf(Op.INPUT_KEY), listOf("input.keys"), false, availability = mapOf(Op.INPUT_TEXT to OpAvailability.Locked("input.text"))))
        assertFalse(canSend(textLocked, Op.INPUT_TEXT))
        assertTrue("keys work without input.text", canSend(textLocked, Op.INPUT_KEY))
        assertEquals(KeysAccess(OpAvailability.Locked("input.text"), OpAvailability.Available), keysAccess(textLocked))
        assertEquals(null, keysAccess(keys.copy(granted = null)))
    }

    @Test
    fun `a send, a key or Back to waits as Typing`() {
        assertFalse(typingBusy(keys))
        assertTrue(typingBusy(keys.copy(pendingControl = PendingControl.Typing)))
    }

    @Test
    fun `the picker offers the window that has the focus now and the held launcher`() {
        assertEquals(emptyList<InputTarget>(), targetChoices(keys))
        val moved = keys.copy(desktop = desktop(focused = 8))
        assertEquals(listOf(InputTarget.Window(8)), targetChoices(moved))
        val refused = moved.withInputRefusal(InputRefusal(ErrorCode.FOCUS_CHANGED, 3, 8))
        assertEquals("the refusal card offers Type there instead", emptyList<InputTarget>(), targetChoices(refused))
        val held = keys.withSession(session(launcher = Launcher(open = true, holder = Holder.THIS_DEVICE)), 0)
        assertEquals(listOf(InputTarget.Launcher), targetChoices(held))
        val onLauncher = held.copy(keys = held.keys?.copy(target = InputTarget.Launcher))
        assertEquals(listOf(InputTarget.Window(7)), targetChoices(onLauncher))
        val otherDevice = keys.withSession(session(launcher = Launcher(open = true, holder = Holder.OTHER_DEVICE)), 0)
        assertEquals(emptyList<InputTarget>(), targetChoices(otherDevice))
    }

    @Test
    fun `the composer asks the keyboard not to learn and not to go full screen`() {
        val flags = noLearning(EditorInfo.IME_ACTION_SEND)
        assertEquals(EditorInfo.IME_ACTION_SEND, flags and EditorInfo.IME_MASK_ACTION)
        assertTrue(flags and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0)
        assertTrue(flags and EditorInfo.IME_FLAG_NO_EXTRACT_UI != 0)
    }

    private companion object {
        fun session(locked: Boolean? = false, launcher: Launcher? = null) =
            SessionState(KeepAwake(false, null, null, 3600, 7200, 14400, null), 0, locked, launcher = launcher)

        fun desktop(focused: Long) =
            Desktop(
                SwayStatus(true, null),
                listOf(Workspace(1, 1, "1", "DP-1", true, true, false)),
                listOf(7L to "foot", 8L to "firefox").map { (id, app) ->
                    Window(id, 1, app, null, focused = id == focused, urgent = false, floating = false, fullscreen = false)
                },
                truncated = false,
            )
    }
}
