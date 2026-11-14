package com.swayconnect.protocol

import java.io.IOException
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

// The remote keyboard (docs/PROTOCOL.md 17.13). The server types into the target only while each check of 17.13
// rule 2 holds and stops at the first that fails; [InputRefusal] reads how much was delivered then.

/** Where input goes. */
sealed interface InputTarget {
    /** The window the user saw as focused. It must still be focused when each part is delivered. */
    data class Window(val windowId: Long) : InputTarget

    /** The launcher this device opened: offer it only while `session.launcher.holder` is `this_device`. */
    data object Launcher : InputTarget
}

/** The rule of `input.text` (17.13). */
object InputText {
    const val MAX_BYTES = 1024

    private const val ZERO_WIDTH_NON_JOINER = 0x200C
    private const val ZERO_WIDTH_JOINER = 0x200D

    /**
     * True when [text] is 1 to [MAX_BYTES] bytes of UTF-8 with no control or format character (2.4) other than
     * U+200C and U+200D, and no unpaired surrogate. So no newline and no tab: Enter is [InputKey.RETURN].
     */
    fun isValid(text: String): Boolean {
        val codePoints = text.codePoints().toArray()
        return codePoints.none { (isControlOrFormat(it) && it != ZERO_WIDTH_NON_JOINER && it != ZERO_WIDTH_JOINER) || it in SURROGATES } &&
            text.toByteArray(Charsets.UTF_8).size in 1..MAX_BYTES
    }

    private val SURROGATES = 0xD800..0xDFFF
}

/** A key of `input.key`: a named key, `F1` to `F12`, one letter `a`-`z` or one digit `0`-`9`. */
@JvmInline
value class InputKey private constructor(val wire: String) {
    val isFunctionKey: Boolean get() = wire in FUNCTION_KEYS

    companion object {
        private val NAMED = setOf(
            "Escape", "Tab", "Return", "BackSpace", "Delete", "Insert", "Left", "Right", "Up", "Down", "Home", "End",
            "Page_Up", "Page_Down", "space",
        )
        private val FUNCTION_KEYS = (1..12).map { "F$it" }.toSet()

        val ESCAPE = InputKey("Escape")
        val TAB = InputKey("Tab")
        val RETURN = InputKey("Return")
        val BACKSPACE = InputKey("BackSpace")

        /** The key named [wire] on the wire, or null when `input.key` does not take it. */
        fun of(wire: String): InputKey? {
            val single = wire.singleOrNull()
            val known = wire in NAMED || wire in FUNCTION_KEYS || (single != null && (single in 'a'..'z' || single in '0'..'9'))
            return if (known) InputKey(wire) else null
        }
    }
}

enum class KeyModifier(val wire: String) {
    CTRL("ctrl"),
    ALT("alt"),
    SHIFT("shift"),
    SUPER("super"),
}

/** Most presses of one `input.key` (17.13). */
const val MAX_KEY_REPEAT = 32

/**
 * Types [text] into [target]. Returns `delivered_chars`, the Unicode scalar values typed.
 * Longer text is split by the caller at character boundaries into requests of at most [InputText.MAX_BYTES]
 * bytes, sent in order, stopping at the first error.
 *
 * @param text one for which [InputText.isValid] holds.
 * @throws RequestFailedException with a code of [InputRefusal] when typing stopped; read it with [InputRefusal.of].
 * @throws MalformedMessageException when the result breaks the wire format.
 */
@Throws(IOException::class)
suspend fun Session.typeText(target: InputTarget, text: String): Long {
    require(InputText.isValid(text)) { "input text must be 1 to ${InputText.MAX_BYTES} bytes without control or format characters" }
    val result = request(Op.INPUT_TEXT, targetArgs(target) { put("text", text) })
    return Wire.decode(TypeTextResult.serializer(), result).deliveredChars
}

/**
 * Presses [key] with [modifiers] held, [repeat] times.
 *
 * @param modifiers not both [KeyModifier.CTRL] and [KeyModifier.ALT] with a function key: that switches the virtual
 *   terminal, and the server refuses it.
 * @param repeat 1 to [MAX_KEY_REPEAT].
 * @throws RequestFailedException with a code of [InputRefusal] when the press was refused; read it with [InputRefusal.of].
 */
@Throws(IOException::class)
suspend fun Session.pressKey(target: InputTarget, key: InputKey, modifiers: Set<KeyModifier> = emptySet(), repeat: Int = 1) {
    require(!(key.isFunctionKey && KeyModifier.CTRL in modifiers && KeyModifier.ALT in modifiers)) { "ctrl+alt with a function key switches the terminal" }
    require(repeat in 1..MAX_KEY_REPEAT) { "repeat must be 1 to $MAX_KEY_REPEAT" }
    val args = targetArgs(target) {
        put("key", key.wire)
        if (modifiers.isNotEmpty()) putJsonArray("modifiers") { modifiers.forEach { add(it.wire) } }
        if (repeat != 1) put("repeat", repeat)
    }
    request(Op.INPUT_KEY, args)
}

/** Why input stopped (17.13): how much was delivered, and for `focus_changed` the window that has the focus now. */
class InputRefusal(val code: String, val deliveredChars: Long, val focusedWindowId: Long?) {
    companion object {
        /** The error codes of 17.13 rule 2. */
        val CODES = setOf(ErrorCode.LOCKED, ErrorCode.LAUNCHER_CLOSED, ErrorCode.KEYBOARD_GRABBED, ErrorCode.FOCUS_CHANGED)

        /**
         * Reads [failure] of `input.text` or `input.key`. Null when its code is not one of [CODES].
         *
         * @throws MalformedMessageException when the `detail` of such a code breaks the wire format.
         */
        @Throws(MalformedMessageException::class)
        fun of(failure: RequestFailedException): InputRefusal? {
            if (failure.code !in CODES) return null
            val detail = failure.detail ?: throw MalformedMessageException("${failure.code} has no detail")
            val read = Wire.decode(InputRefusalDetail.serializer(), detail)
            val focusChanged = failure.code == ErrorCode.FOCUS_CHANGED
            if (focusChanged && FOCUSED_WINDOW_ID !in detail) throw MalformedMessageException("focus_changed has no $FOCUSED_WINDOW_ID")
            return InputRefusal(failure.code, read.deliveredChars, read.focusedWindowId.takeIf { focusChanged })
        }

        private const val FOCUSED_WINDOW_ID = "focused_window_id"
    }
}

private fun targetArgs(target: InputTarget, more: JsonObjectBuilder.() -> Unit): JsonObject {
    if (target is InputTarget.Window) requireId(target.windowId)
    return buildJsonObject {
        when (target) {
            is InputTarget.Window -> put("window_id", target.windowId)
            InputTarget.Launcher -> put("launcher", true)
        }
        more()
    }
}

@Serializable
internal class TypeTextResult(@SerialName("delivered_chars") @Serializable(with = WireInt::class) val deliveredChars: Long)

@Serializable
private class InputRefusalDetail(
    @SerialName("delivered_chars") @Serializable(with = WireInt::class) val deliveredChars: Long,
    @SerialName("focused_window_id") @Serializable(with = WireInt::class) val focusedWindowId: Long? = null,
)
