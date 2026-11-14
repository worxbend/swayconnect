package com.swayconnect.android.ui.state

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay

/**
 * The slider throttle of docs/UX.md 7.2 and 17.8: one send, then [intervalMs] of rest, so a slider sends at most
 * 1000 / [intervalMs] values per second over all keys (the `continuous` bucket of docs/PROTOCOL.md 17.14 is per
 * device, not per key). A value offered while one waits replaces it, so the last value offered for a key is
 * always sent: the final value of `onValueChangeFinished` cannot be lost.
 */
class LatestValueSender<K : Any, V : Any>(private val intervalMs: Long, private val send: suspend (K, V) -> Unit) {
    /** Guarded by itself. In offer order, one value per key. */
    private val waiting = LinkedHashMap<K, V>()
    private val wake = Channel<Unit>(Channel.CONFLATED)

    /** Never blocks; callable from any thread. */
    fun offer(key: K, value: V) {
        synchronized(waiting) { waiting[key] = value }
        wake.trySend(Unit)
    }

    /** Sends until cancelled. A [send] that throws ends it: [send] handles its own failures. */
    suspend fun run() {
        while (true) {
            wake.receive()
            while (true) {
                val (key, value) = takeOldest() ?: break
                send(key, value)
                delay(intervalMs)
            }
        }
    }

    private fun takeOldest(): Pair<K, V>? =
        synchronized(waiting) {
            val oldest = waiting.entries.firstOrNull()?.toPair() ?: return null
            waiting.remove(oldest.first)
            oldest
        }
}
