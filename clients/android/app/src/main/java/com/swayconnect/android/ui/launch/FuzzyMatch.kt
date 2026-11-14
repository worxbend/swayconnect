package com.swayconnect.android.ui.launch

import com.swayconnect.protocol.App
import java.text.Normalizer
import java.util.Locale

/**
 * The client-side app search of docs/UX.md 8.2: over name, generic name and keywords. Pure, no Compose import.
 *
 * Every word of the query must match the app. A word ranks by where it matched (lower is better): the start of
 * the name, the start of a word of the name, inside the name, the start of a word of the generic name or a
 * keyword, inside them, and last its letters in order through the name ("ffx" finds Firefox). Case and accents
 * do not count. Equal ranks keep the server's order (sorted by name).
 */
object FuzzyMatch {
    private const val NAME_START = 0
    private const val NAME_WORD = 1
    private const val NAME_INSIDE = 2
    private const val OTHER_WORD = 3
    private const val OTHER_INSIDE = 4
    private const val NAME_LETTERS = 5

    private val SPACES = Regex("\\s+")
    private val WORD_BREAK = Regex("[^\\p{L}\\p{N}]+")
    private val MARKS = Regex("\\p{M}+")

    /** The apps that match [query], best first. A blank query matches every app in its order. */
    fun filter(apps: List<App>, query: String): List<App> {
        val words = fold(query).split(SPACES).filter { it.isNotEmpty() }
        if (words.isEmpty()) return apps
        return apps
            .mapNotNull { app -> rank(app, words)?.let { app to it } }
            .sortedBy { (_, rank) -> rank }
            .map { (app, _) -> app }
    }

    /** The summed rank of [words] in [app]; null when one word does not match. */
    private fun rank(app: App, words: List<String>): Int? {
        val name = fold(app.name)
        val others = (listOfNotNull(app.genericName) + app.keywords).map(::fold)
        var total = 0
        for (word in words) total += wordRank(word, name, others) ?: return null
        return total
    }

    private fun wordRank(word: String, name: String, others: List<String>): Int? =
        when {
            name.startsWith(word) -> NAME_START
            startsAWord(name, word) -> NAME_WORD
            word in name -> NAME_INSIDE
            others.any { startsAWord(it, word) } -> OTHER_WORD
            others.any { word in it } -> OTHER_INSIDE
            inOrder(word, name) -> NAME_LETTERS
            else -> null
        }

    private fun startsAWord(text: String, word: String): Boolean = text.split(WORD_BREAK).any { it.startsWith(word) }

    /** True when every character of [word] appears in [text] in the same order. */
    private fun inOrder(word: String, text: String): Boolean {
        var at = 0
        for (char in word) {
            at = text.indexOf(char, at)
            if (at < 0) return false
            at++
        }
        return true
    }

    /** Lower case without accents, so "é" finds "e" and "E". */
    private fun fold(text: String): String =
        MARKS.replace(Normalizer.normalize(text, Normalizer.Form.NFD), "").lowercase(Locale.ROOT)
}
