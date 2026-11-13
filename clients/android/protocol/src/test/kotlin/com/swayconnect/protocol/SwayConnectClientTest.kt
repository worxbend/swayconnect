package com.swayconnect.protocol

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.coroutines.CoroutineContext
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class SwayConnectClientTest {
    /** docs/PROTOCOL.md 15.10, for a transport that failed to pin. */
    @Test
    fun `another certificate is a pin failure and no frame is sent`() {
        val sent = ByteArrayOutputStream()
        val transport = object : Transport {
            override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection =
                object : Connection {
                    override val input: InputStream = ByteArrayInputStream(ByteArray(0))
                    override val output: OutputStream = sent
                    override val fingerprint: ByteArray = ByteArray(FINGERPRINT_BYTES)

                    override fun close() = Unit
                }
        }
        val client = SwayConnectClient(transport, Dispatchers.IO, Fixtures.cliClient)

        assertThrows(PinMismatchException::class.java) {
            runBlocking { client.runSession(Fixtures.profile, Fixtures.key) {} }
        }

        assertEquals(0, sent.size())
    }

    /** docs/PROTOCOL.md 5.1 rule 4. */
    @Test
    fun `a welcome with a wrong server proof ends the connection`() {
        val server = ScriptedServer(Fixtures.authHandshake, Fixtures.fingerprint, ignoredMembers = setOf("proof"))
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        val anotherKey = ByteArray(Proofs.KEY_BYTES)
        var reachedSession = false

        assertThrows(HandshakeException::class.java) {
            replay(server) { client.runSession(Fixtures.profile, anotherKey) { reachedSession = true } }
        }

        assertEquals(false, reachedSession)
    }

    @Test
    fun `a welcome that names another device is refused`() {
        assertWelcomeRefused(""""device_id":"${Fixtures.deviceId}"""", """"device_id":"AAAAAAAAAAAAAAAAAAAAAA"""")
    }

    @Test
    fun `a welcome that names another protocol version is refused`() {
        assertWelcomeRefused(""""protocol":1""", """"protocol":2""")
    }

    /** The reader allocates what a frame length announces, so the announced limit has a ceiling. */
    @Test
    fun `a welcome that announces frames over the limit of this version is refused`() {
        assertWelcomeRefused(""""max_frame_s2c":${Limits.MAX_FRAME_S2C}""", """"max_frame_s2c":${Int.MAX_VALUE}""")
    }

    /** Replays T2 with [original] in its `welcome` replaced by [changed]. The server proof stays right. */
    private fun assertWelcomeRefused(original: String, changed: String) {
        val welcome = (Fixtures.authHandshake.last() as Step.Send).payload.toString()
        assertTrue("the transcript holds $original", original in welcome)
        val script = Fixtures.authHandshake.dropLast(1) + Step.Send(parseObject(welcome.replace(original, changed)))
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var reachedSession = false

        assertThrows(HandshakeException::class.java) {
            replay(server) { client.runSession(Fixtures.profile, Fixtures.key) { reachedSession = true } }
        }

        assertEquals(false, reachedSession)
    }

    /** No socket timeout runs after TLS, and the ping and request timers start only after `welcome`. */
    @Test(timeout = SILENT_SERVER_WAIT_MS)
    fun `a server that stays silent after TLS is given up at the handshake deadline`() {
        // The script, and so the server's end of the connection, outlives the wait of this test.
        val server = ScriptedServer(listOf(Step.AwaitRelease), Fixtures.fingerprint)
        val client = clientFor(server, Timing(handshakeTimeoutMs = SHORT_WAIT_MS))

        try {
            assertThrows(IOException::class.java) {
                runBlocking { client.runSession(Fixtures.profile, Fixtures.key) {} }
            }
        } finally {
            server.release()
        }
    }

    /** docs/PROTOCOL.md section 11 rule 3. */
    @Test
    fun `a ping is sent when the interval passes without another frame`() {
        val script = Fixtures.authHandshake + firstRequestIsPing + firstRequestAnswered
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Timing(pingIntervalMs = SHORT_WAIT_MS))

        // The script ends, and with it the connection, only after the ping has arrived.
        val end = replay(server) { client.runSession(Fixtures.profile, Fixtures.key) {} }

        assertTrue("$end", end is SessionEnd.Lost)
    }

    /** docs/PROTOCOL.md section 11 rule 4. */
    @Test
    fun `a request without an answer in time ends the connection`() {
        val script = Fixtures.authHandshake + Step.ExpectAnyFrame + Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Timing(requestTimeoutMs = SHORT_WAIT_MS))
        var failure: IOException? = null

        val end = replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                try {
                    session.request(Op.PING)
                } catch (e: IOException) {
                    failure = e
                } finally {
                    server.release()
                }
            }
        }

        assertNotNull("the request failed", failure)
        assertTrue("$end", end is SessionEnd.Lost)
    }

    /** docs/PROTOCOL.md 13.2 rules 2 and 3: what a newer server may send is skipped, and the session goes on. */
    @Test
    fun `a frame type and an event that this client does not know are skipped`() {
        val unknownType = Step.Send(parseObject("""{"type":"note","text":"from a newer server"}"""))
        val unknownEvent = Step.Send(parseObject("""{"type":"evt","event":"weather.changed","data":{}}"""))
        val script = Fixtures.authHandshake + firstRequestIsPing + unknownType + unknownEvent + firstRequestAnswered +
            Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var answered = false

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                session.request(Op.PING)
                answered = true
                server.release()
            }
        }

        assertEquals(true, answered)
    }

    /**
     * The watcher that closes the connection on a cancel is a coroutine of its own. When the cancel
     * came before its first dispatch it never ran, and the read stayed blocked for ever.
     */
    @Test
    fun `a cancel that comes before the close watcher was dispatched still closes the connection`() {
        val dispatcher = HoldingDispatcher()
        val connection = BlockedConnection(Fixtures.fingerprint)
        val transport = object : Transport {
            override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection = connection
        }
        val client = SwayConnectClient(transport, dispatcher, Fixtures.cliClient)
        val call = CoroutineScope(dispatcher).launch { runCatching { client.runSession(Fixtures.profile, Fixtures.key) {} } }
        try {
            assertTrue("the read of hello began", connection.reading.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))

            call.cancel()
            dispatcher.release()

            assertTrue("the connection is closed", connection.closed.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))
        } finally {
            connection.close()
        }
    }

    /** docs/PROTOCOL.md 8.2 rule 4. */
    @Test
    fun `a state with a revision that is not higher is dropped`() {
        val subscription = Vectors.transcript("t3-session").take(2)
        val staleOffline = Vectors.transcript("t6-sway-offline").first()
        // The stale event follows the ping request: by then the client has applied the subscribe response.
        // Sent right after that response, the event could be applied first, and the response would be the stale one.
        val script = Fixtures.authHandshake + subscription + pingRequest + lowerRevision(staleOffline) + pingResponse +
            Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var online: Boolean? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                session.subscribeDesktop()
                // The answer to the ping comes after the stale event, so the event was handled by now.
                session.request(Op.PING)
                online = session.desktop.value?.sway?.online
                server.release()
            }
        }

        assertEquals(true, online)
    }

    /** The `state.changed` of T6 with the `rev` of the subscribe response of T3. */
    private fun lowerRevision(event: Step): Step {
        val payload = (event as Step.Send).payload
        return Step.Send(parseObject(payload.toString().replace("\"rev\":9", "\"rev\":7")))
    }

    private fun clientFor(server: Transport, timing: Timing): SwayConnectClient =
        SwayConnectClient(server, Dispatchers.IO, Fixtures.cliClient, { hexToBytes(Fixtures.laterClientNonce) }, timing)

    private val firstRequestIsPing = Step.Expect(parseObject("""{"type":"req","id":1,"op":"ping"}"""))
    private val firstRequestAnswered = Step.Send(parseObject("""{"type":"res","id":1,"ok":true,"result":{}}"""))
    private val pingRequest = Step.Expect(parseObject("""{"type":"req","id":2,"op":"ping"}"""))
    private val pingResponse = Step.Send(parseObject("""{"type":"res","id":2,"ok":true,"result":{}}"""))

    private companion object {
        const val SHORT_WAIT_MS = 100L
        const val SILENT_SERVER_WAIT_MS = TIMEOUT_MS / 2
    }
}

/** Runs the first task at once and holds every later one until [release], like a pool whose other threads are busy. */
private class HoldingDispatcher : CoroutineDispatcher() {
    private val firstTaskTaken = AtomicBoolean(false)
    private val released = AtomicBoolean(false)
    private val held = ConcurrentLinkedQueue<Runnable>()

    override fun dispatch(context: CoroutineContext, block: Runnable) {
        if (released.get() || firstTaskTaken.compareAndSet(false, true)) thread { block.run() } else held += block
    }

    fun release() {
        released.set(true)
        generateSequence { held.poll() }.forEach { thread { it.run() } }
    }
}

/** A connection whose read blocks until it is closed, like a socket whose peer says nothing. */
private class BlockedConnection(override val fingerprint: ByteArray) : Connection {
    val reading = CountDownLatch(1)
    val closed = CountDownLatch(1)

    override val input: InputStream = object : InputStream() {
        override fun read(): Int {
            reading.countDown()
            closed.await()
            throw IOException("the connection is closed")
        }
    }
    override val output: OutputStream = ByteArrayOutputStream()

    override fun close() = closed.countDown()
}
