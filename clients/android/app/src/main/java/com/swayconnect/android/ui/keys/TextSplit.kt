package com.swayconnect.android.ui.keys

import com.swayconnect.protocol.InputText
import java.text.BreakIterator
import java.util.Locale

/**
 * Cuts the composer's text into `input.text` requests (docs/UX.md 9.2, PROTOCOL.md 17.13): each at most
 * [InputText.MAX_BYTES] bytes of UTF-8, cut between user-perceived characters, so a refusal between two requests
 * never leaves half an emoji typed. Only a single character longer than the limit is cut between code points.
 * Pure: no Compose import.
 */
object TextSplit {
    sealed interface Result {
        /** Nothing to send. */
        data object Empty : Result

        /** A line break, tab or other control or format character: `input.text` refuses it, so nothing is sent. */
        data object Unsendable : Result

        /** Send these in order and stop at the first error. Joined, they are the text. */
        data class Parts(val parts: List<String>) : Result
    }

    fun split(text: String): Result {
        if (text.isEmpty()) return Result.Empty
        val parts = cut(text)
        return if (parts.all(InputText::isValid)) Result.Parts(parts) else Result.Unsendable
    }

    private fun cut(text: String): List<String> {
        val parts = mutableListOf<String>()
        val part = StringBuilder()
        var partBytes = 0
        for (character in graphemes(text)) {
            val bytes = utf8Bytes(character)
            if (partBytes + bytes > InputText.MAX_BYTES && part.isNotEmpty()) {
                parts += part.toString()
                part.clear()
                partBytes = 0
            }
            if (bytes > InputText.MAX_BYTES) {
                parts += cutBetweenCodePoints(character)
            } else {
                part.append(character)
                partBytes += bytes
            }
        }
        if (part.isNotEmpty()) parts += part.toString()
        return parts
    }

    private fun graphemes(text: String): Sequence<String> {
        val boundaries = BreakIterator.getCharacterInstance(Locale.ROOT).apply { setText(text) }
        return generateSequence(boundaries.first() to boundaries.next()) { (_, end) -> end to boundaries.next() }
            .takeWhile { (_, end) -> end != BreakIterator.DONE }
            .map { (start, end) -> text.substring(start, end) }
    }

    /** One character over the limit (a base with hundreds of combining marks): cut where the code points meet. */
    private fun cutBetweenCodePoints(character: String): List<String> {
        val parts = mutableListOf<String>()
        val part = StringBuilder()
        var partBytes = 0
        character.codePoints().forEach { codePoint ->
            val bytes = utf8Bytes(codePoint)
            if (partBytes + bytes > InputText.MAX_BYTES) {
                parts += part.toString()
                part.clear()
                partBytes = 0
            }
            part.appendCodePoint(codePoint)
            partBytes += bytes
        }
        if (part.isNotEmpty()) parts += part.toString()
        return parts
    }

    private fun utf8Bytes(text: String): Int = text.codePoints().map(::utf8Bytes).sum()

    private fun utf8Bytes(codePoint: Int): Int =
        when {
            codePoint < 0x80 -> 1
            codePoint < 0x800 -> 2
            codePoint < 0x10000 -> 3
            else -> 4
        }
}
