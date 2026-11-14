package com.swayconnect.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The client rules of docs/PROTOCOL.md 17.13 that the vectors cannot express. */
class InputTest {
    @Test
    fun `an unpaired surrogate is not text`() {
        assertFalse(InputText.isValid("a\uD83D"))
        assertFalse(InputText.isValid("\uDC00b"))
        assertTrue(InputText.isValid("😀"))
    }

    @Test
    fun `keys are the closed list of input_key`() {
        listOf("Escape", "Page_Down", "space", "F1", "F12", "a", "z", "0", "9").forEach { assertEquals(it, InputKey.of(it)?.wire) }
        listOf("", " ", "A", "F0", "F13", "Enter", "return", "XF86Switch_VT_1", "Super_L", "ab", "é").forEach { assertNull(it, InputKey.of(it)) }
        assertTrue(InputKey.of("F7")!!.isFunctionKey)
        assertFalse(InputKey.RETURN.isFunctionKey)
    }

    @Test
    fun `only input refusals are read, and their detail must have its shape`() {
        assertNull(InputRefusal.of(failure("forbidden", """{"scope":"input.text"}""")))
        listOf(
            failure("locked", null),
            failure("locked", "{}"),
            failure("keyboard_grabbed", """{"delivered_chars":-1}"""),
            failure("launcher_closed", """{"delivered_chars":"0"}"""),
            failure("focus_changed", """{"delivered_chars":0}"""),
            failure("focus_changed", """{"delivered_chars":0,"focused_window_id":"7"}"""),
        ).forEach { assertThrows(it.code, MalformedMessageException::class.java) { InputRefusal.of(it) } }
    }

    @Test
    fun `focused_window_id is read only for focus_changed`() {
        assertNull(InputRefusal.of(failure("locked", """{"delivered_chars":3,"focused_window_id":7}"""))!!.focusedWindowId)
        assertEquals(7L, InputRefusal.of(failure("focus_changed", """{"delivered_chars":3,"focused_window_id":7}"""))!!.focusedWindowId)
    }

    private fun failure(code: String, detail: String?): RequestFailedException =
        RequestFailedException(code, "test", detail?.let(::parseObject))
}
