package com.swayconnect.protocol

import java.nio.ByteBuffer
import java.security.MessageDigest
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** Icon rules of docs/PROTOCOL.md 17.5. */
object IconHash {
    const val BYTES = 16
    const val MAX_PER_REQUEST = 8

    /** A client decodes no icon with more pixels per side (17.5 rule 3). */
    const val MAX_SIDE_PX = 256

    /** A server never sends a bigger PNG (17.17). */
    const val MAX_PNG_BYTES = 65_536

    /** True when [text] is a canonical bytes16, the only form a server issues. */
    fun isValid(text: String): Boolean = Base64Url.decode(text, BYTES) != null

    /** Base64url of the first 16 bytes of SHA-256 over [png]. */
    fun of(png: ByteArray): String = Base64Url.encode(MessageDigest.getInstance("SHA-256").digest(png).copyOf(BYTES))
}

/** An icon whose hash matched and whose PNG header passed the bounds check. Decode it as untrusted input still. */
class Icon(val hash: String, val png: ByteArray)

enum class DropReason { HASH_MISMATCH, NOT_PNG, BOUNDS, TOO_LARGE }

data class DroppedIcon(val hash: String, val reason: DropReason)

/** The checked answer to one `icon.get`. */
class IconBatch(val icons: List<Icon>, val dropped: List<DroppedIcon>, val missing: List<String>)

@Serializable
private class IconEntry(val hash: String, val png: String)

@Serializable
private class IconGetResult(val icons: List<IconEntry>, val missing: List<String>)

/**
 * Checks the result of an `icon.get` for [requested] (17.5 rule 3).
 * A result that breaks the wire format, or names a hash that was not asked for, is malformed.
 * An icon whose bytes fail a check is dropped and reported; the others are kept.
 */
@Throws(MalformedMessageException::class)
internal fun readIconResult(requested: List<String>, result: JsonObject): IconBatch {
    val decoded = Wire.decode(IconGetResult.serializer(), result)
    val named = decoded.icons.map { it.hash } + decoded.missing
    if (named.any { it !in requested } || named.toSet().size != named.size) {
        throw MalformedMessageException("icon.get answered a hash that was not asked for, or twice")
    }
    val icons = mutableListOf<Icon>()
    val dropped = mutableListOf<DroppedIcon>()
    for (entry in decoded.icons) {
        val png = Base64Url.decodeAny(entry.png) ?: throw MalformedMessageException("icon png is not canonical base64url")
        val reason = dropReason(entry.hash, png)
        if (reason == null) icons += Icon(entry.hash, png) else dropped += DroppedIcon(entry.hash, reason)
    }
    return IconBatch(icons, dropped, decoded.missing)
}

private fun dropReason(hash: String, png: ByteArray): DropReason? =
    when {
        IconHash.of(png) != hash -> DropReason.HASH_MISMATCH
        png.size > IconHash.MAX_PNG_BYTES -> DropReason.TOO_LARGE
        else -> pngBoundsProblem(png)
    }

private val PNG_SIGNATURE = byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
private val IHDR = "IHDR".toByteArray(Charsets.US_ASCII)
private const val IHDR_LENGTH = 13
private const val HEADER_BYTES = 8 + 8 + 8

/** Reads width and height from the IHDR chunk, which a PNG must have first, without decoding pixels. */
private fun pngBoundsProblem(png: ByteArray): DropReason? {
    if (png.size < HEADER_BYTES || !png.copyOf(8).contentEquals(PNG_SIGNATURE)) return DropReason.NOT_PNG
    val header = ByteBuffer.wrap(png, 8, HEADER_BYTES - 8)
    val chunkLength = header.int
    val chunkType = ByteArray(4).also { header.get(it) }
    if (chunkLength != IHDR_LENGTH || !chunkType.contentEquals(IHDR)) return DropReason.NOT_PNG
    val width = header.int
    val height = header.int
    // PNG allows 1 to 2^31 - 1: zero, or a set top bit (a negative Int), is not a PNG.
    if (width <= 0 || height <= 0) return DropReason.NOT_PNG
    return if (width > IconHash.MAX_SIDE_PX || height > IconHash.MAX_SIDE_PX) DropReason.BOUNDS else null
}
