package com.swayconnect.protocol

import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject

/** A frame that breaks docs/PROTOCOL.md 3.3. The stream position is unknown afterwards: close the connection. */
class FrameException(val kind: Kind, message: String) : IOException(message) {
    enum class Kind { BAD, TOO_LARGE }
}

/** `frame = length (4 bytes, unsigned, big-endian) || payload`; the payload is one JSON object. */
object FrameCodec {
    private const val LENGTH_BYTES = 4
    private const val MIN_PAYLOAD_BYTES = 2

    /**
     * Deepest array and object nesting read. The JSON parser recurses per level, so a 1 MiB frame of brackets
     * would overflow the stack. The deepest message of the protocol, a `desktop` state with a layout of 16
     * container levels (17.17), nests about 40 levels.
     */
    const val MAX_NESTING = 64

    /**
     * Reads one frame. Returns null when the stream ends cleanly before a frame starts.
     * The length is checked against [limit] before the payload is read or allocated.
     */
    @Throws(IOException::class)
    fun read(input: InputStream, limit: Int): JsonObject? {
        val header = ByteArray(LENGTH_BYTES)
        val headerBytes = readAvailable(input, header)
        if (headerBytes == 0) return null
        if (headerBytes < LENGTH_BYTES) throw EOFException("stream ended inside a frame length")

        // A length of 2^31 or more reads as a negative Int. It is over every limit.
        val length = ByteBuffer.wrap(header).int
        if (length < 0 || length > limit) throw FrameException(FrameException.Kind.TOO_LARGE, "frame is over $limit bytes")
        if (length < MIN_PAYLOAD_BYTES) throw FrameException(FrameException.Kind.BAD, "frame length is under 2")

        val payload = ByteArray(length)
        if (readAvailable(input, payload) < length) throw EOFException("stream ended inside a frame payload")
        return parse(payload)
    }

    @Throws(IOException::class)
    fun write(output: OutputStream, message: JsonObject, limit: Int) {
        val payload = message.toString().toByteArray(Charsets.UTF_8)
        if (payload.size > limit) {
            throw FrameException(FrameException.Kind.TOO_LARGE, "frame of ${payload.size} bytes is over $limit bytes")
        }
        // One write, so that a frame is never interleaved with another writer's bytes half way.
        output.write(ByteBuffer.allocate(LENGTH_BYTES + payload.size).putInt(payload.size).put(payload).array())
        output.flush()
    }

    private fun parse(payload: ByteArray): JsonObject {
        val text = decodeUtf8OrNull(payload) ?: throw FrameException(FrameException.Kind.BAD, "payload is not UTF-8")
        if (nestsDeeperThan(payload, MAX_NESTING)) throw FrameException(FrameException.Kind.BAD, "payload nests too deep")
        val element =
            try {
                Json.parseToJsonElement(text)
            } catch (e: SerializationException) {
                // The parser message quotes the input. It is left out on purpose.
                throw FrameException(FrameException.Kind.BAD, "payload is not JSON")
            }
        return element as? JsonObject ?: throw FrameException(FrameException.Kind.BAD, "payload is not a JSON object")
    }

    /** Counts brackets outside strings. Bytes of multi-byte UTF-8 characters are 0x80 or more and never match. */
    private fun nestsDeeperThan(payload: ByteArray, limit: Int): Boolean {
        var depth = 0
        var inString = false
        var escaped = false
        for (byte in payload) {
            val char = byte.toInt().toChar()
            when {
                escaped -> escaped = false
                inString && char == '\\' -> escaped = true
                char == '"' -> inString = !inString
                inString -> Unit
                char == '{' || char == '[' -> if (++depth > limit) return true
                char == '}' || char == ']' -> depth--
            }
        }
        return false
    }

    /** Fills [buffer] unless the stream ends first. Returns the number of bytes read. */
    private fun readAvailable(input: InputStream, buffer: ByteArray): Int {
        var filled = 0
        while (filled < buffer.size) {
            val count = input.read(buffer, filled, buffer.size - filled)
            if (count < 0) break
            filled += count
        }
        return filled
    }
}
