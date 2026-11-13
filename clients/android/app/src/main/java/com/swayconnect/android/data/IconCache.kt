package com.swayconnect.android.data

import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import com.swayconnect.protocol.IconBatch
import com.swayconnect.protocol.IconHash
import com.swayconnect.protocol.RequestFailedException
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/** Turns PNG bytes that passed every check into an image, or returns null when they do not decode. */
fun interface IconDecoder<I : Any> {
    fun decode(png: ByteArray): I?
}

/** One `icon.get` call: `Session::iconGet`. */
typealias IconFetch = suspend (List<String>) -> IconBatch

/**
 * App icons by hash (docs/PROTOCOL.md 17.5, docs/UX.md 17.6): a small memory cache in front of a capped disk
 * cache in front of `icon.get`. PNG bytes are untrusted, from the network and from the disk alike: each one is
 * checked against its hash and its IHDR bounds before [decoder] sees it.
 *
 * The UI calls [want] and reads [image]; [loaded] ticks after each new image. The view model runs [run] for its
 * whole life on an IO dispatcher and calls [attach] with the live session's `iconGet`, [detach] when it ends.
 * Requests carry at most 8 hashes, start at least [MIN_INTERVAL_MS] apart, and wait out `rate_limited`.
 */
class IconCache<I : Any>(
    private val dir: File,
    private val decoder: IconDecoder<I>,
    private val diskCapBytes: Long = DISK_CAP_BYTES,
    private val memoryEntries: Int = MEMORY_ENTRIES,
    /** Waits between requests. Replaced in tests to record the waits instead of sleeping. */
    private val pause: suspend (Long) -> Unit = { delay(it) },
) {
    private val lock = Any()
    private val memory = object : LinkedHashMap<String, I>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, I>?): Boolean = size > memoryEntries
    }

    /** Wanted, not yet looked up on the disk. */
    private val pending = LinkedHashSet<String>()

    /** Not on the disk; waits for a session. */
    private val toFetch = LinkedHashSet<String>()

    /** Missing on the server, or failed a check. Not asked for again until the next [attach]. */
    private val unavailable = HashSet<String>()
    private var fetch: IconFetch? = null
    private val work = Channel<Unit>(Channel.CONFLATED)
    private val mutableLoaded = MutableStateFlow(0)

    /** Counts the images added so far. Read it to recompose when an icon arrives. */
    val loaded: StateFlow<Int> = mutableLoaded.asStateFlow()

    /** The image of [hash] when it is in memory. Never blocks. */
    fun image(hash: String): I? = synchronized(lock) { memory[hash] }

    /** Asks for [hash]. A value that is not a server-issued hash is ignored: it never reaches the disk or the wire. */
    fun want(hash: String) {
        if (!IconHash.isValid(hash)) return
        synchronized(lock) {
            if (hash in memory || hash in unavailable || hash in toFetch || !pending.add(hash)) return
        }
        work.trySend(Unit)
    }

    /** Fetches missing icons through [fetch] until [detach]. A new session asks again for what was unavailable. */
    fun attach(fetch: IconFetch) {
        synchronized(lock) {
            this.fetch = fetch
            toFetch += unavailable
            unavailable.clear()
        }
        work.trySend(Unit)
    }

    /** Stops fetching through [fetch]. Hashes still wanted wait for the next [attach]. */
    fun detach(fetch: IconFetch) {
        synchronized(lock) { if (this.fetch === fetch) this.fetch = null }
    }

    /** Serves [want] until cancelled. Blocks on file I/O: run it on an IO dispatcher. */
    suspend fun run(): Nothing {
        while (true) {
            work.receive()
            // Cards of one frame call want() one by one; a short window lets them share a request.
            pause(BATCH_WINDOW_MS)
            drain()
        }
    }

    private suspend fun drain() {
        while (true) {
            loadPendingFromDisk()
            val (current, batch) = synchronized(lock) { fetch to toFetch.take(IconHash.MAX_PER_REQUEST) }
            if (current == null || batch.isEmpty()) return
            pause(fetchBatch(current, batch))
        }
    }

    private fun loadPendingFromDisk() {
        val hashes = synchronized(lock) { pending.toList().also { pending.clear() } }
        for (hash in hashes) {
            val image = readFromDisk(hash)
            synchronized(lock) {
                if (image != null) remember(hash, image) else toFetch += hash
            }
        }
    }

    /** Sends one request. Returns how long to wait before the next one. */
    private suspend fun fetchBatch(current: IconFetch, batch: List<String>): Long {
        val result = try {
            current(batch)
        } catch (e: RequestFailedException) {
            if (e.code == RATE_LIMITED) return retryAfterMs(e)
            // forbidden, bad_request, not_offered: asking again on this connection gives the same answer.
            synchronized(lock) { giveUp(batch) }
            return MIN_INTERVAL_MS
        } catch (e: IOException) {
            // The connection is gone, or its answer was malformed (MalformedMessageException); the session
            // ends either way. Keep the batch for the next session.
            detach(current)
            return MIN_INTERVAL_MS
        }
        for (icon in result.icons) {
            val image = accept(icon.hash, icon.png) ?: continue
            writeToDisk(icon.hash, icon.png)
            synchronized(lock) { remember(icon.hash, image) }
        }
        // Missing, dropped by the protocol checks, refused by accept(), or left out of the answer.
        synchronized(lock) { giveUp(batch.filter { it in toFetch }) }
        return MIN_INTERVAL_MS
    }

    /** Checks untrusted bytes, then decodes them. Null when a check or the decoder refuses them. */
    private fun accept(hash: String, png: ByteArray): I? {
        if (png.size > IconHash.MAX_PNG_BYTES || IconHash.of(png) != hash) return null
        val size = pngSize(png) ?: return null
        if (size.first > IconHash.MAX_SIDE_PX || size.second > IconHash.MAX_SIDE_PX) return null
        return decoder.decode(png)
    }

    private fun remember(hash: String, image: I) {
        toFetch -= hash
        memory[hash] = image
        mutableLoaded.value += 1
    }

    private fun giveUp(hashes: List<String>) {
        toFetch -= hashes.toSet()
        unavailable += hashes
    }

    private fun fileOf(hash: String) = File(dir, "$hash.png")

    /** A damaged or foreign file is deleted and counts as a miss. */
    private fun readFromDisk(hash: String): I? {
        val file = fileOf(hash)
        if (!file.isFile) return null
        val image = try {
            if (file.length() > IconHash.MAX_PNG_BYTES) null else accept(hash, file.readBytes())
        } catch (e: IOException) {
            null
        }
        if (image == null) {
            file.delete()
        } else {
            // Eviction drops the least recently used file first.
            file.setLastModified(System.currentTimeMillis())
        }
        return image
    }

    /**
     * The disk cache only saves a request. When a write fails the icon stays in memory and is fetched again after
     * the app restarts, so the failure is not reported further.
     */
    private fun writeToDisk(hash: String, png: ByteArray) {
        val draft = File(dir, "$hash.tmp")
        val written = try {
            if (dir.isDirectory || dir.mkdirs()) {
                draft.writeBytes(png)
                draft.renameTo(fileOf(hash))
            } else {
                false
            }
        } catch (e: IOException) {
            false
        }
        if (written) trimDisk() else draft.delete()
    }

    /** Deletes the oldest files until the PNGs fit [diskCapBytes]. */
    private fun trimDisk() {
        val files = dir.listFiles { file -> file.isFile && file.name.endsWith(".png") }?.toMutableList() ?: return
        var total = files.sumOf { it.length() }
        files.sortBy { it.lastModified() }
        for (file in files) {
            if (total <= diskCapBytes) return
            val length = file.length()
            if (file.delete()) total -= length
        }
    }

    companion object {
        const val DISK_CAP_BYTES = 4L * 1024 * 1024

        /** 128 icons of at most 128 x 128 ARGB pixels: at most 8 MiB. */
        const val MEMORY_ENTRIES = 128
        const val MIN_INTERVAL_MS = 500L
        const val BATCH_WINDOW_MS = 50L

        /** A server hint above this is not trusted; the cache asks again after this long. */
        const val MAX_RETRY_AFTER_MS = 60_000L
        private const val RATE_LIMITED = "rate_limited"

        private fun retryAfterMs(e: RequestFailedException): Long {
            val hint = try {
                e.detail?.get("retry_after_ms")?.jsonPrimitive?.longOrNull
            } catch (bad: IllegalArgumentException) {
                // retry_after_ms is not a primitive.
                null
            }
            return (hint ?: MIN_INTERVAL_MS).coerceIn(MIN_INTERVAL_MS, MAX_RETRY_AFTER_MS)
        }
    }
}

private val PNG_SIGNATURE = byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
private val IHDR = "IHDR".toByteArray(Charsets.US_ASCII)
private const val IHDR_LENGTH = 13
private const val HEADER_BYTES = 24

/** Width and height from the IHDR chunk, read without decoding pixels. Null when [png] is not a PNG. */
internal fun pngSize(png: ByteArray): Pair<Int, Int>? {
    if (png.size < HEADER_BYTES || !png.copyOf(PNG_SIGNATURE.size).contentEquals(PNG_SIGNATURE)) return null
    val header = ByteBuffer.wrap(png, PNG_SIGNATURE.size, HEADER_BYTES - PNG_SIGNATURE.size)
    val chunkLength = header.int
    val chunkType = ByteArray(IHDR.size).also { header.get(it) }
    if (chunkLength != IHDR_LENGTH || !chunkType.contentEquals(IHDR)) return null
    val width = header.int
    val height = header.int
    // A set top bit reads as a negative Int; PNG allows 1 to 2^31 - 1.
    return if (width <= 0 || height <= 0) null else width to height
}

/** The platform decoder. It reads the bounds again itself and refuses more than 256 px a side before decoding. */
object BitmapIconDecoder : IconDecoder<ImageBitmap> {
    override fun decode(png: ByteArray): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(png, 0, png.size, bounds)
        if (bounds.outWidth !in 1..IconHash.MAX_SIDE_PX || bounds.outHeight !in 1..IconHash.MAX_SIDE_PX) return null
        return BitmapFactory.decodeByteArray(png, 0, png.size)?.asImageBitmap()
    }
}
