package com.swayconnect.protocol

import java.io.InputStream
import java.io.OutputStream
import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue

internal sealed interface Step {
    /** The server sends this frame. */
    data class Send(val payload: JsonObject) : Step

    /** The client must send a frame equal to this one, as a JSON value. */
    data class Expect(val payload: JsonObject) : Step

    /** The client must send one frame. Its content is not checked. */
    data object ExpectAnyFrame : Step

    /**
     * The server waits here for [ScriptedServer.release]. The client cancels `onReady` when the connection ends,
     * so a close or a `bye` that follows at once can overtake the code under test.
     */
    data object AwaitRelease : Step
}

/**
 * A server that plays a transcript over in-memory pipes, on its own thread.
 * [ignoredMembers] are left out when a client frame is compared: a transcript may print a value
 * that a correct client cannot produce (a zero proof, a wrong version) or cannot choose (the id).
 */
internal class ScriptedServer(
    private val script: List<Step>,
    private val fingerprint: ByteArray,
    private val ignoredMembers: Set<String> = emptySet(),
) : Transport {
    private val serverOutput = PipedOutputStream()
    private val clientInput = PipedInputStream(serverOutput, PIPE_BYTES)
    private val clientOutput = PipedOutputStream()
    private val serverInput = PipedInputStream(clientOutput, PIPE_BYTES)
    private val failure = AtomicReference<Throwable?>(null)
    private val released = CountDownLatch(1)
    private val worker = Thread({ play() }, "scripted-server")

    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        worker.start()
        return object : Connection {
            override val input: InputStream = clientInput
            override val output: OutputStream = clientOutput
            override val fingerprint: ByteArray = this@ScriptedServer.fingerprint

            override fun close() {
                clientInput.close()
                clientOutput.close()
                // A blocked piped read ignores the close of its own end. The end of the writer wakes it.
                serverOutput.close()
            }
        }
    }

    /** Lets the script go past [Step.AwaitRelease]. The test calls it when its `onReady` work is done. */
    fun release() = released.countDown()

    /** Fails when the client did not send what the script expects, or left before the script ended. */
    fun assertScriptCompleted() {
        worker.join(TIMEOUT_MS)
        assertFalse("the scripted server still waits for the client", worker.isAlive)
        failure.get()?.let { throw AssertionError("the client left the script: $it", it) }
    }

    private fun play() {
        try {
            script.forEach(::playStep)
        } catch (e: Throwable) {
            // Kept for the test thread; an assertion on this thread would otherwise be lost.
            failure.set(e)
        } finally {
            serverOutput.close()
        }
    }

    private fun playStep(step: Step) {
        when (step) {
            is Step.Send -> FrameCodec.write(serverOutput, step.payload, Int.MAX_VALUE)
            is Step.Expect -> assertEquals(comparable(step.payload), comparable(readClientFrame()))
            Step.ExpectAnyFrame -> readClientFrame()
            Step.AwaitRelease -> assertTrue("the test did not release the script", released.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))
        }
    }

    private fun readClientFrame(): JsonObject =
        FrameCodec.read(serverInput, CLIENT_FRAME_LIMIT) ?: throw AssertionError("the client closed instead of sending a frame")

    private fun comparable(frame: JsonObject): Map<String, Any> = frame.filterKeys { it !in ignoredMembers }

    private companion object {
        const val PIPE_BYTES = 1 shl 16
        const val CLIENT_FRAME_LIMIT = 16384
    }
}

internal const val TIMEOUT_MS = 10_000L

/** Runs [clientCall] against [server] and checks afterwards that the whole script was played. */
internal fun <T> replay(server: ScriptedServer, clientCall: suspend () -> T): T {
    val outcome = runCatching { runBlocking { withTimeout(TIMEOUT_MS) { clientCall() } } }
    server.assertScriptCompleted()
    return outcome.getOrThrow()
}

internal fun clientFor(server: Transport, info: ClientInfo, clientNonceHex: String): SwayConnectClient =
    SwayConnectClient(server, Dispatchers.IO, info) { hexToBytes(clientNonceHex) }
