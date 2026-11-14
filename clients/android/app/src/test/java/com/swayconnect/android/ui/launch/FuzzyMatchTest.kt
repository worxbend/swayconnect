package com.swayconnect.android.ui.launch

import com.swayconnect.protocol.App
import org.junit.Assert.assertEquals
import org.junit.Test

/** The client-side app search of docs/UX.md 8.2. */
class FuzzyMatchTest {
    private val apps = listOf(
        app("Alacritty", generic = "Terminal", keywords = listOf("shell", "prompt")),
        app("Files", generic = "File Manager", keywords = listOf("folder", "explorer")),
        app("Firefox", generic = "Web Browser", keywords = listOf("internet", "web")),
        app("mpv Media Player", generic = null, keywords = listOf("video")),
        app("Terminator", generic = null, keywords = emptyList()),
        app("Xterm", generic = null, keywords = emptyList()),
    )

    @Test
    fun `a blank query keeps every app in the server's order`() {
        assertEquals(names(apps), names(FuzzyMatch.filter(apps, "")))
        assertEquals(names(apps), names(FuzzyMatch.filter(apps, "  \t ")))
    }

    @Test
    fun `name start, then a word of the name, then inside the name, then generic name and keywords`() {
        assertEquals(listOf("Terminator", "Xterm", "Alacritty"), names(FuzzyMatch.filter(apps, "term")))
        assertEquals(listOf("mpv Media Player"), names(FuzzyMatch.filter(apps, "media")))
        assertEquals(listOf("Firefox"), names(FuzzyMatch.filter(apps, "fox")))
    }

    @Test
    fun `generic name and keywords match`() {
        assertEquals(listOf("Firefox"), names(FuzzyMatch.filter(apps, "browser")))
        assertEquals(listOf("Files"), names(FuzzyMatch.filter(apps, "folder")))
        assertEquals("inside a keyword", listOf("Files"), names(FuzzyMatch.filter(apps, "plorer")))
        assertEquals(listOf("mpv Media Player"), names(FuzzyMatch.filter(apps, "video")))
    }

    @Test
    fun `equal ranks keep the server's order`() {
        assertEquals(listOf("Files", "Firefox"), names(FuzzyMatch.filter(apps, "fi")))
    }

    @Test
    fun `letters in order through the name rank last`() {
        assertEquals(listOf("Firefox"), names(FuzzyMatch.filter(apps, "ffx")))
        assertEquals("out of order", emptyList<String>(), names(FuzzyMatch.filter(apps, "xff")))
        val ranked = FuzzyMatch.filter(listOf(app("Fax Fun"), app("Faxed")), "fx")
        assertEquals("both by letters: server order", listOf("Fax Fun", "Faxed"), names(ranked))
        assertEquals("a name start wins over letters", listOf("Fx Tool", "Fax Fun"), names(FuzzyMatch.filter(listOf(app("Fax Fun"), app("Fx Tool")), "fx")))
    }

    @Test
    fun `case and accents do not count`() {
        assertEquals(listOf("Firefox"), names(FuzzyMatch.filter(apps, "FIREFOX")))
        val accented = listOf(app("Écran"), app("Cafe"))
        assertEquals(listOf("Écran"), names(FuzzyMatch.filter(accented, "ecran")))
        assertEquals(listOf("Cafe"), names(FuzzyMatch.filter(accented, "café")))
    }

    @Test
    fun `every word of the query must match`() {
        assertEquals(listOf("Firefox"), names(FuzzyMatch.filter(apps, "web fire")))
        assertEquals(emptyList<String>(), names(FuzzyMatch.filter(apps, "web term")))
        assertEquals("no app at all", emptyList<String>(), names(FuzzyMatch.filter(apps, "zzz")))
    }

    @Test
    fun `the ranks of all words add up`() {
        val ranked = FuzzyMatch.filter(listOf(app("Files", keywords = listOf("web")), app("Web Files")), "files web")
        assertEquals("name word + name start beats name start + keyword", listOf("Web Files", "Files"), names(ranked))
    }

    private fun names(list: List<App>) = list.map { it.name }

    private companion object {
        fun app(name: String, generic: String? = null, keywords: List<String> = emptyList()) =
            App(entryId = name.replace(' ', '-'), name = name, genericName = generic, keywords = keywords, icon = null, terminal = false, risky = false)
    }
}
