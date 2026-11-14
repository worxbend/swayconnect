package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.ErrorCode
import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.InputRefusal
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.KeepAwake
import com.swayconnect.protocol.KeyModifier
import com.swayconnect.protocol.Launcher
import com.swayconnect.protocol.SessionState
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The Keys reducer of docs/UX.md 9.2. */
class KeysReducerTest {
    private val base = UiState(pair = PairForm(deviceName = "Pixel"), desktop = desktop(focused = 7))

    @Test
    fun `opening Keys captures the focused window, or the one Desktop's Type picked, and leaving drops it`() {
        val opened = base.withArea(Area.Keys)
        assertEquals(InputTarget.Window(7), opened.keys?.target)
        val focusMoved = opened.copy(desktop = desktop(focused = 8)).withArea(Area.Keys)
        assertEquals("a later focus change keeps the target", InputTarget.Window(7), focusMoved.keys?.target)
        assertEquals(InputTarget.Window(8), base.withArea(Area.Keys, windowId = 8).keys?.target)
        assertNull(opened.withArea(Area.Desktop).keys)
        assertNull("no focused window, no target", base.copy(desktop = null).withArea(Area.Keys).keys?.target)
    }

    @Test
    fun `the launcher target is offered only while this device holds a running launcher`() {
        val keys = base.withArea(Area.Keys)
        assertEquals(InputTarget.Window(7), keys.withKeysTarget(InputTarget.Launcher).keys?.target)
        val other = keys.withSession(session(Launcher(open = true, holder = Holder.OTHER_DEVICE)), 0)
        assertEquals(InputTarget.Window(7), other.withKeysTarget(InputTarget.Launcher).keys?.target)
        val held = keys.withSession(session(Launcher(open = true, holder = Holder.THIS_DEVICE)), 0)
        assertEquals(InputTarget.Launcher, held.withKeysTarget(InputTarget.Launcher).keys?.target)
    }

    @Test
    fun `a closed launcher sends the target back to the focused window`() {
        val held = base.withSession(session(Launcher(open = true, holder = Holder.THIS_DEVICE)), 0).withLauncherTarget()
        assertEquals(Area.Keys, held.area)
        assertEquals(InputTarget.Launcher, held.withKeysFollowingSession().keys?.target)
        val closed = held.withSession(session(Launcher(open = false, holder = null)), 1).withKeysFollowingSession()
        assertEquals(InputTarget.Window(7), closed.keys?.target)
    }

    @Test
    fun `a launcher that closes before sway shows the focus again hands the target to the next focused window`() {
        // Real sway 1.12 (M2c R5): while fuzzel holds the keyboard no window is focused, and the session state that
        // says the launcher closed can arrive before the desktop state that shows the focus again.
        val held = base.withSession(session(Launcher(open = true, holder = Holder.THIS_DEVICE)), 0).withLauncherTarget()
        val noFocus = held.copy(desktop = desktop(focused = 0))
        val closed = noFocus.withSession(session(Launcher(open = false, holder = null)), 1).withKeysFollowingSession()
        assertNull(closed.keys?.target)
        val refocused = closed.copy(desktop = desktop(focused = 7)).withKeysFollowingDesktop()
        assertEquals(InputTarget.Window(7), refocused.keys?.target)
        val refused = noFocus.withInputRefusal(refusal(ErrorCode.LAUNCHER_CLOSED))
        assertEquals(InputTarget.Window(7), refused.copy(desktop = desktop(focused = 7)).withKeysFollowingDesktop().keys?.target)
        val picked = closed.withKeysTarget(InputTarget.Window(8)).copy(desktop = desktop(focused = 7)).withKeysFollowingDesktop()
        assertEquals("a picked target stays", InputTarget.Window(8), picked.keys?.target)
        val opened = base.copy(desktop = desktop(focused = 0)).withArea(Area.Keys)
        assertNull("Keys opened with no focus keeps no target", opened.copy(desktop = desktop(focused = 7)).withKeysFollowingDesktop().keys?.target)
    }

    @Test
    fun `sticky modifiers arm, disarm and go with the next key`() {
        val armed = base.withArea(Area.Keys).withModifierToggled(KeyModifier.CTRL).withModifierToggled(KeyModifier.SUPER)
        assertEquals(setOf(KeyModifier.CTRL, KeyModifier.SUPER), armed.keys?.sticky)
        assertEquals(setOf(KeyModifier.SUPER), armed.withModifierToggled(KeyModifier.CTRL).keys?.sticky)
        assertEquals(emptySet<KeyModifier>(), armed.withModifiersReleased().keys?.sticky)
    }

    @Test
    fun `each send gets a new serial and records what arrived`() {
        val first = base.withArea(Area.Keys).withTyped(40, 40, null)
        assertEquals(Typed(1, 40, 40), first.keys?.typed)
        assertNull(first.keys?.refusal)
        assertEquals(Typed(2, 40, 40), first.withTyped(40, 40, null).keys?.typed)
    }

    @Test
    fun `focus_changed keeps the target until Type there`() {
        val stopped = base.withArea(Area.Keys).withTyped(16, 40, refusal(ErrorCode.FOCUS_CHANGED, delivered = 16, focused = 8))
        assertEquals(Typed(1, 16, 40), stopped.keys?.typed)
        assertEquals(KeysRefusal.FocusChanged(8), stopped.keys?.refusal)
        assertEquals(InputTarget.Window(7), stopped.keys?.target)

        val there = stopped.withTargetFromRefusal()
        assertEquals(InputTarget.Window(8), there.keys?.target)
        assertNull(there.keys?.refusal)
    }

    @Test
    fun `launcher_closed falls back to the focused window, keyboard_grabbed keeps the target`() {
        val launcher = base.withSession(session(Launcher(open = true, holder = Holder.THIS_DEVICE)), 0).withLauncherTarget()
        val closed = launcher.withInputRefusal(refusal(ErrorCode.LAUNCHER_CLOSED))
        assertEquals(KeysRefusal.LauncherClosed, closed.keys?.refusal)
        assertEquals(InputTarget.Window(7), closed.keys?.target)

        val grabbed = launcher.withInputRefusal(refusal(ErrorCode.KEYBOARD_GRABBED))
        assertEquals(KeysRefusal.KeyboardGrabbed, grabbed.keys?.refusal)
        assertEquals(InputTarget.Launcher, grabbed.keys?.target)
    }

    @Test
    fun `the locked card goes once the screen is known to be unlocked`() {
        val locked = base.withArea(Area.Keys).withInputRefusal(refusal(ErrorCode.LOCKED))
        assertEquals(KeysRefusal.Locked, locked.keys?.refusal)
        val unknown = locked.withSession(session(null, locked = null), 0).withKeysFollowingSession()
        assertEquals(KeysRefusal.Locked, unknown.keys?.refusal)
        assertNull(locked.withSession(session(null, locked = false), 0).withKeysFollowingSession().keys?.refusal)
    }

    @Test
    fun `private mode and every change without Keys open`() {
        assertEquals(true, base.withArea(Area.Keys).withPrivateMode(true).keys?.privateMode)
        assertEquals(base, base.withPrivateMode(true).withModifierToggled(KeyModifier.ALT).withTyped(1, 1, null).withTargetFromRefusal())
    }

    private companion object {
        fun refusal(code: String, delivered: Long = 0, focused: Long? = null) = InputRefusal(code, delivered, focused)

        fun session(launcher: Launcher?, locked: Boolean? = false) =
            SessionState(KeepAwake(false, null, null, 3600, 7200, 14400, null), 0, locked, launcher = launcher)

        fun desktop(focused: Long) =
            Desktop(
                SwayStatus(true, null),
                listOf(Workspace(1, 1, "1", "DP-1", true, true, false)),
                listOf(7L, 8L).map { Window(it, 1, "foot", null, focused = it == focused, urgent = false, floating = false, fullscreen = false) },
                truncated = false,
            )
    }
}
