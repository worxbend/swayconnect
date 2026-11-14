package com.swayconnect.android.ui.keys

import com.swayconnect.protocol.InputText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** TextSplit of docs/UX.md 9.2: requests of at most 1024 bytes of UTF-8, cut between characters. */
class TextSplitTest {
    @Test
    fun `empty text sends nothing, short text is one part`() {
        assertEquals(TextSplit.Result.Empty, TextSplit.split(""))
        assertEquals(listOf("git status"), parts("git status"))
        assertEquals("spaces are text", listOf("   "), parts("   "))
    }

    @Test
    fun `1024 bytes is one part and 1025 bytes is two`() {
        assertEquals(listOf("a".repeat(1024)), parts("a".repeat(1024)))
        assertEquals(listOf("a".repeat(1024), "b"), parts("a".repeat(1024) + "b"))
        val long = "x".repeat(5000)
        val cut = parts(long)
        assertEquals(listOf(1024, 1024, 1024, 1024, 904), cut.map(::utf8Size))
        assertEquals(long, cut.joinToString(""))
    }

    @Test
    fun `a multi-byte character at the limit moves whole into the next part`() {
        assertEquals(listOf("a".repeat(1023), "é"), parts("a".repeat(1023) + "é"))
        assertEquals("a surrogate pair is never cut", listOf("a".repeat(1022), "😀"), parts("a".repeat(1022) + "😀"))
        assertEquals("three-byte character", listOf("a".repeat(1022), "€"), parts("a".repeat(1022) + "€"))
    }

    @Test
    fun `a base letter and its combining mark stay in one part`() {
        assertEquals(listOf("a".repeat(1023), "e\u0301"), parts("a".repeat(1023) + "e\u0301"))
    }

    @Test
    fun `ZWJ emoji text is allowed, parts stay in the limit and join back to the text`() {
        val family = "👩\u200D👩\u200D👧"
        val text = family.repeat(100)
        val cut = parts(text)
        assertTrue(cut.size > 1)
        assertTrue(cut.all { utf8Size(it) <= InputText.MAX_BYTES && InputText.isValid(it) })
        assertTrue("no part starts inside a sequence", cut.none { it.startsWith("\u200D") })
        assertEquals(text, cut.joinToString(""))
    }

    @Test
    fun `one character longer than the limit is cut between its code points`() {
        val zalgo = "e" + "\u0301".repeat(600)
        val cut = parts(zalgo)
        assertEquals(listOf(1023, 178), cut.map(::utf8Size))
        assertEquals(zalgo, cut.joinToString(""))
    }

    @Test
    fun `a line break, tab or format character anywhere makes the whole text unsendable`() {
        assertEquals(TextSplit.Result.Unsendable, TextSplit.split("ls\n"))
        assertEquals(TextSplit.Result.Unsendable, TextSplit.split("a\tb"))
        assertEquals(TextSplit.Result.Unsendable, TextSplit.split("left\u200Eright"))
        assertEquals("checked in later parts too", TextSplit.Result.Unsendable, TextSplit.split("a".repeat(3000) + "\r"))
        assertEquals(TextSplit.Result.Unsendable, TextSplit.split("\uD800"))
    }

    @Test
    fun `after a stop the field keeps the unsent tail`() {
        val text = "héllo 😀 world"
        assertEquals(" world", text.substring(deliveredEnd(text, 7)))
        assertEquals("nothing arrived", text, text.substring(deliveredEnd(text, 0)))
        assertEquals("more than the field holds", "", text.substring(deliveredEnd(text, 99)))
        assertEquals("a broken count drops nothing", text, text.substring(deliveredEnd(text, -1)))
        val stoppedInSecondPart = "a".repeat(1024) + "bcd"
        assertEquals("cd", stoppedInSecondPart.substring(deliveredEnd(stoppedInSecondPart, 1025)))
    }

    private fun parts(text: String): List<String> {
        val split = TextSplit.split(text)
        assertTrue("expected parts for ${text.take(20)}: $split", split is TextSplit.Result.Parts)
        val cut = (split as TextSplit.Result.Parts).parts
        assertFalse(cut.any { it.isEmpty() })
        return cut
    }

    private fun utf8Size(text: String): Int = text.toByteArray(Charsets.UTF_8).size
}
