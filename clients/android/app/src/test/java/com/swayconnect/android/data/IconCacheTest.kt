package com.swayconnect.android.data

import com.swayconnect.protocol.Icon
import com.swayconnect.protocol.IconBatch
import com.swayconnect.protocol.IconHash
import com.swayconnect.protocol.RequestFailedException
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.file.Files
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class IconCacheTest {
    private lateinit var dir: File
    private val decoded = mutableListOf<String>()
    private val decoder = IconDecoder<String> { png -> IconHash.of(png).also { decoded += it } }

    /** Every wait of the cache, in order. The test reads them to know where the loop is. */
    private val pauses = Channel<Long>(Channel.UNLIMITED)
    private val requests = mutableListOf<List<String>>()

    @Before
    fun setUp() {
        dir = Files.createTempDirectory("icons").toFile()
    }

    @After
    fun tearDown() {
        dir.deleteRecursively()
    }

    private fun cache(diskCapBytes: Long = IconCache.DISK_CAP_BYTES) =
        IconCache(dir, decoder, diskCapBytes = diskCapBytes, pause = { pauses.send(it); yield() })

    /** Answers each request with the PNGs it knows and reports the others as missing. */
    private fun server(vararg pngs: ByteArray): IconFetch {
        val byHash = pngs.associateBy(IconHash::of)
        return { hashes ->
            requests += hashes
            IconBatch(hashes.mapNotNull { h -> byHash[h]?.let { Icon(h, it) } }, emptyList(), hashes.filter { it !in byHash })
        }
    }

    private fun CoroutineScope.running(cache: IconCache<String>) = launch { cache.run() }

    /** Waits until the loop has paused for these waits, in this order. */
    private suspend fun expectPauses(vararg ms: Long) {
        for (expected in ms) assertEquals(expected, pauses.receive())
    }

    @Test
    fun `wanted icons are fetched at most 8 per request, 500 ms apart, and kept on disk`() = runBlocking {
        val pngs = (1..10).map { png(16, 16, salt = it) }
        val cache = cache()
        val loop = running(cache)
        pngs.forEach { cache.want(IconHash.of(it)) }
        cache.attach(server(*pngs.toTypedArray()))

        cache.loaded.first { it == 10 }
        loop.cancel()

        assertEquals(listOf(8, 2), requests.map { it.size })
        assertEquals(pngs.map(IconHash::of).toSet(), requests.flatten().toSet())
        expectPauses(IconCache.BATCH_WINDOW_MS, IconCache.MIN_INTERVAL_MS, IconCache.MIN_INTERVAL_MS)
        pngs.forEach { assertEquals(IconHash.of(it), cache.image(IconHash.of(it))) }
        assertEquals(10, dir.listFiles { f -> f.name.endsWith(".png") }!!.size)
    }

    @Test
    fun `a disk hit needs no request, also without a session`() = runBlocking {
        val png = png(32, 32, salt = 1)
        val hash = IconHash.of(png)
        File(dir, "$hash.png").writeBytes(png)
        val cache = cache()
        val loop = running(cache)
        cache.want(hash)

        cache.loaded.first { it == 1 }
        loop.cancel()

        assertEquals(hash, cache.image(hash))
        assertTrue(requests.isEmpty())
    }

    @Test
    fun `an icon whose bytes do not match its hash is dropped before decode`() = runBlocking {
        val asked = IconHash.of(png(16, 16, salt = 1))
        val other = png(16, 16, salt = 2)
        val cache = cache()
        val loop = running(cache)
        cache.want(asked)
        cache.attach { hashes -> requests += hashes; IconBatch(listOf(Icon(asked, other)), emptyList(), emptyList()) }

        expectPauses(IconCache.BATCH_WINDOW_MS, IconCache.MIN_INTERVAL_MS)
        // Unavailable for the rest of this connection: wanting it again wakes nothing.
        cache.want(asked)
        yield()
        loop.cancel()

        assertTrue(pauses.tryReceive().isFailure)
        assertEquals(1, requests.size)
        assertNull(cache.image(asked))
        assertTrue(decoded.isEmpty())
        assertTrue(dir.listFiles()!!.isEmpty())
    }

    @Test
    fun `a damaged disk file is deleted and fetched again`() = runBlocking {
        val png = png(16, 16, salt = 1)
        val hash = IconHash.of(png)
        File(dir, "$hash.png").writeBytes(png(16, 16, salt = 9))
        val cache = cache()
        val loop = running(cache)
        cache.want(hash)
        cache.attach(server(png))

        cache.loaded.first { it == 1 }
        loop.cancel()

        assertEquals(listOf(listOf(hash)), requests)
        assertEquals(listOf(hash), decoded)
        assertTrue(File(dir, "$hash.png").readBytes().contentEquals(png))
    }

    @Test
    fun `an icon wider than 256 px is refused before decode`() = runBlocking {
        val wide = png(257, 16, salt = 1)
        val tall = png(16, 300, salt = 2)
        val cache = cache()
        val loop = running(cache)
        cache.want(IconHash.of(wide))
        cache.want(IconHash.of(tall))
        cache.attach(server(wide, tall))

        expectPauses(IconCache.BATCH_WINDOW_MS, IconCache.MIN_INTERVAL_MS)
        loop.cancel()

        assertTrue(decoded.isEmpty())
        assertNull(cache.image(IconHash.of(wide)))
        assertNull(cache.image(IconHash.of(tall)))
        assertTrue(dir.listFiles()!!.isEmpty())
    }

    @Test
    fun `rate_limited waits retry_after_ms and sends the same batch again`() = runBlocking {
        val png = png(16, 16, salt = 1)
        val hash = IconHash.of(png)
        val answer = server(png)
        var calls = 0
        val cache = cache()
        val loop = running(cache)
        cache.want(hash)
        cache.attach { hashes ->
            calls += 1
            if (calls == 1) {
                requests += hashes
                throw RequestFailedException("rate_limited", "slow down", buildJsonObject { put("retry_after_ms", 1200) })
            }
            answer(hashes)
        }

        cache.loaded.first { it == 1 }
        loop.cancel()

        expectPauses(IconCache.BATCH_WINDOW_MS, 1200, IconCache.MIN_INTERVAL_MS)
        assertEquals(listOf(listOf(hash), listOf(hash)), requests)
    }

    @Test
    fun `a huge retry_after_ms is capped`() = runBlocking {
        val cache = cache()
        val loop = running(cache)
        cache.want(IconHash.of(png(16, 16, salt = 1)))
        cache.attach { throw RequestFailedException("rate_limited", "", buildJsonObject { put("retry_after_ms", Long.MAX_VALUE) }) }

        expectPauses(IconCache.BATCH_WINDOW_MS, IconCache.MAX_RETRY_AFTER_MS)
        loop.cancel()
    }

    @Test
    fun `a lost connection keeps the hashes for the next session`() = runBlocking {
        val png = png(16, 16, salt = 1)
        val hash = IconHash.of(png)
        val cache = cache()
        val loop = running(cache)
        cache.want(hash)
        cache.attach { throw IOException("gone") }
        expectPauses(IconCache.BATCH_WINDOW_MS, IconCache.MIN_INTERVAL_MS)

        cache.attach(server(png))
        cache.loaded.first { it == 1 }
        loop.cancel()

        assertEquals(hash, cache.image(hash))
    }

    @Test
    fun `a value that is not a server hash never reaches the disk or the wire`() = runBlocking {
        val cache = cache()
        val loop = running(cache)
        cache.want("../../../etc/passwd")
        cache.want("short")
        val real = png(16, 16, salt = 1)
        cache.want(IconHash.of(real))
        cache.attach(server(real))

        cache.loaded.first { it == 1 }
        loop.cancel()

        assertEquals(listOf(listOf(IconHash.of(real))), requests)
    }

    @Test
    fun `the disk cache stays under its cap and drops the oldest file first`() = runBlocking {
        val pngs = (1..6).map { png(16, 16, salt = it, padding = 1000) }
        val cap = pngs.first().size * 4L
        val cache = cache(diskCapBytes = cap)
        val loop = running(cache)
        cache.attach(server(*pngs.toTypedArray()))
        // One request each, so the files get distinct write times in order.
        for ((index, png) in pngs.withIndex()) {
            cache.want(IconHash.of(png))
            cache.loaded.first { it == index + 1 }
            File(dir, "${IconHash.of(png)}.png").setLastModified(1_000_000L * (index + 1))
        }
        loop.cancel()

        val left = dir.listFiles { f -> f.name.endsWith(".png") }!!.map { it.name.removeSuffix(".png") }.toSet()
        assertTrue(left.sumOf { File(dir, "$it.png").length() } <= cap)
        assertEquals(pngs.takeLast(4).map(IconHash::of).toSet(), left)
        assertFalse(dir.listFiles()!!.any { it.name.endsWith(".tmp") })
    }

    @Test
    fun `pngSize reads the IHDR and refuses what is not a PNG`() {
        assertEquals(300 to 20, pngSize(png(300, 20, salt = 0)))
        assertNull(pngSize(ByteArray(40)))
        assertNull(pngSize(png(0, 20, salt = 0)))
        assertNull(pngSize(png(-1, 20, salt = 0)))
    }
}

/** A PNG signature and IHDR chunk for [width] x [height], then [salt] and [padding] bytes so hashes differ. */
private fun png(width: Int, height: Int, salt: Int, padding: Int = 0): ByteArray {
    val buffer = ByteBuffer.allocate(8 + 8 + 13 + 4 + 4 + padding)
    buffer.put(byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A))
    buffer.putInt(13).put("IHDR".toByteArray(Charsets.US_ASCII)).putInt(width).putInt(height)
    buffer.put(byteArrayOf(8, 6, 0, 0, 0))
    buffer.putInt(0).putInt(salt)
    return buffer.array()
}
