package com.swayconnect.protocol

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

private const val REPLACEMENT_CHARACTER = 0xFFFD

/** The fixed list of docs/PROTOCOL.md 2.4. It does not follow later Unicode versions. */
fun isControlOrFormat(codePoint: Int): Boolean =
    when (codePoint) {
        in 0x0000..0x001F, in 0x007F..0x009F -> true
        0x00AD, 0x061C, 0x180E -> true
        in 0x200B..0x200F, in 0x2028..0x202E, in 0x2060..0x206F -> true
        0xFEFF, in 0xFFF9..0xFFFB -> true
        in 0xE0000..0xE007F -> true
        else -> false
    }

fun hasControlOrFormat(text: String): Boolean = text.codePoints().anyMatch { isControlOrFormat(it) }

/** Replaces every control or format character with U+FFFD. */
fun replaceUnsafe(text: String): String {
    val safe = StringBuilder(text.length)
    var index = 0
    while (index < text.length) {
        val codePoint = text.codePointAt(index)
        safe.appendCodePoint(if (isControlOrFormat(codePoint)) REPLACEMENT_CHARACTER else codePoint)
        index += Character.charCount(codePoint)
    }
    return safe.toString()
}

/** Rule for `device_name`, `server.name` and the URI `name`: 1 to 64 bytes, no control or format character. */
fun isDisplayLabel(text: String): Boolean =
    text.toByteArray(Charsets.UTF_8).size in 1..Limits.DISPLAY_LABEL_MAX_BYTES && !hasControlOrFormat(text)

/** Rule for `platform` and `client`: 1 to [maxBytes] characters from `A-Z a-z 0-9 . _ / -` and space. */
fun isInfoText(text: String, maxBytes: Int): Boolean =
    text.length in 1..maxBytes && text.all { it in INFO_TEXT_CHARACTERS }

private const val INFO_TEXT_CHARACTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._/- "

/** Strict UTF-8 decoding. Returns null where `String(bytes)` would silently put U+FFFD. */
internal fun decodeUtf8OrNull(bytes: ByteArray): String? =
    try {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes))
            .toString()
    } catch (e: CharacterCodingException) {
        null
    }
