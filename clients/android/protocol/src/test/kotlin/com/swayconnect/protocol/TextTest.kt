package com.swayconnect.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TextTest {
    @Test
    fun `both ends of every range of the list are control or format characters`() {
        val ranges = listOf(
            0x0000..0x001F, 0x007F..0x009F, 0x00AD..0x00AD, 0x061C..0x061C, 0x180E..0x180E,
            0x200B..0x200F, 0x2028..0x202E, 0x2060..0x206F, 0xFEFF..0xFEFF, 0xFFF9..0xFFFB, 0xE0000..0xE007F,
        )
        for (range in ranges) {
            assertTrue(isControlOrFormat(range.first))
            assertTrue(isControlOrFormat(range.last))
        }
        // The neighbours of the ranges that touch no other range.
        for (outside in listOf(0x20, 0x7E, 0xA0, 0xAC, 0xAE, 0x200A, 0x2010, 0x2027, 0x202F, 0x205F, 0x2070, 0xFFF8, 0xFFFC, 0xE0080)) {
            assertFalse(Integer.toHexString(outside), isControlOrFormat(outside))
        }
    }

    @Test
    fun `replaceUnsafe keeps text and replaces each unsafe character with U+FFFD`() {
        val tag = String(Character.toChars(0xE0041))

        assertEquals("a�[2Jb�c�", replaceUnsafe("a\u001B[2Jb‮c$tag"))
        assertEquals("Example Domain — 😀", replaceUnsafe("Example Domain — 😀"))
    }

    @Test
    fun `display label is 1 to 64 bytes without unsafe characters`() {
        assertTrue(isDisplayLabel("Pixel 9"))
        assertTrue(isDisplayLabel("a".repeat(64)))
        assertFalse(isDisplayLabel(""))
        assertFalse(isDisplayLabel("a".repeat(65)))
        assertFalse("bytes, not characters", isDisplayLabel("—".repeat(22)))
        assertFalse(isDisplayLabel("Pixel‮9"))
    }
}
