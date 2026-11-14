package com.swayconnect.android.ui.state

import java.util.Collections
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The slider throttle of docs/UX.md 7.2: spaced sends, newest value wins, the final value is never lost. */
class LatestValueSenderTest {
    private class Sent(val key: Long, val value: Int, val atNs: Long)

    private val sent = Collections.synchronizedList(mutableListOf<Sent>())
    private val sender = LatestValueSender<Long, Int>(INTERVAL_MS) { key, value -> sent += Sent(key, value, System.nanoTime()) }

    @Test(timeout = TIMEOUT_MS)
    fun `a fast drag sends few values, spaced, and always the last one`() = runBlocking {
        val run = launch(Dispatchers.Default) { sender.run() }
        for (value in 0..100) sender.offer(1, value)
        awaitSent { it.value == 100 }
        delay(INTERVAL_MS * 2)
        run.cancelAndJoin()

        val values = sent.map { it.value }
        assertEquals(100, values.last())
        assertTrue("conflated, not one send per value: $values", values.size < 10)
        assertSpaced()
    }

    @Test(timeout = TIMEOUT_MS)
    fun `the final value of each key is sent, one key does not swallow another`() = runBlocking {
        val run = launch(Dispatchers.Default) { sender.run() }
        sender.offer(1, 10)
        sender.offer(2, 20)
        sender.offer(1, 11)
        awaitSent { it.key == 2L }
        awaitSent { it.key == 1L && it.value == 11 }
        delay(INTERVAL_MS * 2)
        run.cancelAndJoin()

        assertEquals(11, sent.last { it.key == 1L }.value)
        assertEquals(listOf(20), sent.filter { it.key == 2L }.map { it.value })
        assertSpaced()
    }

    @Test(timeout = TIMEOUT_MS)
    fun `a value offered after a pause goes out again, also when it repeats the last one`() = runBlocking {
        val run = launch(Dispatchers.Default) { sender.run() }
        sender.offer(1, 40)
        awaitSent { it.value == 40 }
        delay(INTERVAL_MS * 2)
        sender.offer(1, 40)
        withTimeout(TIMEOUT_MS) { while (sent.size < 2) delay(POLL_MS) }
        run.cancelAndJoin()

        assertEquals(listOf(40, 40), sent.map { it.value })
    }

    private suspend fun awaitSent(match: (Sent) -> Boolean) {
        withTimeout(TIMEOUT_MS) { while (synchronized(sent) { sent.none(match) }) delay(POLL_MS) }
    }

    /** Over all keys, two sends are never closer than the interval: the `continuous` bucket is per device. */
    private fun assertSpaced() {
        sent.zipWithNext().forEach { (a, b) ->
            assertTrue("sends ${(b.atNs - a.atNs) / 1_000_000} ms apart", b.atNs - a.atNs >= INTERVAL_MS * 1_000_000)
        }
    }

    private companion object {
        const val INTERVAL_MS = 40L
        const val POLL_MS = 5L
        const val TIMEOUT_MS = 10_000L
    }
}
