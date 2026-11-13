package com.swayconnect.android.ui.common

import com.swayconnect.android.ui.theme.DarkSwayTokens
import com.swayconnect.android.ui.theme.LightSwayTokens
import com.swayconnect.protocol.Window
import org.junit.Assert.assertEquals
import org.junit.Test

class CommonComponentsTest {
    @Test
    fun `monogram is the first grapheme, upper-cased`() {
        assertEquals("F", monogramOf("firefox"))
        assertEquals("A", monogramOf("  Alacritty"))
        assertEquals("É", monogramOf("évince"))
        // A ZWJ family emoji is one grapheme of several chars.
        assertEquals("👨‍👩‍👧", monogramOf("👨‍👩‍👧app"))
        assertEquals("?", monogramOf(null))
        assertEquals("?", monogramOf("   "))
    }

    @Test
    fun `monogram colour is stable per app id`() {
        val ids = listOf("firefox", "Alacritty", "org.telegram.desktop", "")
        for (tokens in listOf(LightSwayTokens, DarkSwayTokens)) {
            for (id in ids) {
                assertEquals(tokens.washFor(id), tokens.washFor(id))
                assertEquals(tokens.washFor(id), tokens.copy().washFor(String(id.toCharArray())))
            }
        }
        // Pinned, so a change of the hash shows up as a failing test: FNV-1a 32 of "a" is 0xE40C292C.
        assertEquals(LightSwayTokens.washes[(0xE40C292Cu % 8u).toInt()], LightSwayTokens.washFor("a"))
    }

    @Test
    fun `window pills follow the order of UX 5_2`() {
        val all = window(focused = true, urgent = true, fullscreen = true, floating = true)
        val group = GroupPosition(GroupKind.Tabbed, position = 2, count = 3)
        assertEquals(
            listOf(WindowMark.Focused, WindowMark.Urgent, WindowMark.Fullscreen, WindowMark.Floating, WindowMark.InGroup(group)),
            windowMarks(all, group),
        )
        assertEquals(emptyList<WindowMark>(), windowMarks(window(), group = null))
        assertEquals(listOf(WindowMark.Urgent), windowMarks(window(urgent = true), group = null))
    }

    @Test(expected = IllegalArgumentException::class)
    fun `a group position outside its count is refused`() {
        GroupPosition(GroupKind.Stacked, position = 4, count = 3)
    }

    private fun window(
        focused: Boolean = false,
        urgent: Boolean = false,
        fullscreen: Boolean = false,
        floating: Boolean = false,
    ) = Window(
        id = 1,
        workspaceId = 1,
        appId = "firefox",
        title = "Docs",
        focused = focused,
        urgent = urgent,
        floating = floating,
        fullscreen = fullscreen,
    )
}
