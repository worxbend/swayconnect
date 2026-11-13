package com.swayconnect.android.ui

import androidx.lifecycle.viewModelScope
import com.swayconnect.android.AppContainer
import com.swayconnect.android.data.MemorySecretsStore
import com.swayconnect.android.data.ProfileStore
import com.swayconnect.android.data.StoredPairing
import com.swayconnect.protocol.Base64Url
import com.swayconnect.protocol.Connection
import com.swayconnect.protocol.FINGERPRINT_BYTES
import com.swayconnect.protocol.FrameCodec
import com.swayconnect.protocol.Limits
import com.swayconnect.protocol.Profile
import com.swayconnect.protocol.Proofs
import com.swayconnect.protocol.RejectCode
import com.swayconnect.protocol.Transport
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** The connection rules of [MainViewModel], against transports that need no socket and no Android. */
class MainViewModelTest {
    @get:Rule val directory = TemporaryFolder()

    private val fingerprint = ByteArray(FINGERPRINT_BYTES) { 7 }
    private val link =
        "swayconnect://pair?v=1&host=192.0.2.1&port=4000&fp=${Base64Url.encode(fingerprint)}" +
            "&secret=${Base64Url.encode(ByteArray(SECRET_BYTES) { 9 })}"
    private val profile =
        Profile(host = "192.0.2.1", port = 4000, fp = Base64Url.encode(fingerprint), deviceId = "device", serverName = "desk")
    // Lazy: the rule creates the folder only when a test starts.
    private val store by lazy { ProfileStore(File(directory.root, "profile.json"), MemorySecretsStore(null)) }
    private val key = ByteArray(Proofs.KEY_BYTES) { 1 }
    private val models = mutableListOf<MainViewModel>()
    private val startedActivities = CoroutineScope(Dispatchers.Default)

    @After
    fun closeModels() {
        startedActivities.cancel()
        models.forEach { it.viewModelScope.cancel() }
    }

    /** A model with a stored pairing, a started activity and a session that shows its first desktop state. */
    private suspend fun readySession(): Pair<MainViewModel, PipeTransport> {
        store.save(StoredPairing(profile, key))
        val transport = PipeTransport(fingerprint)
        val model = modelWith(transport)
        startedActivities.launch { model.connectWhileStarted() }
        transport.serveSession(profile, key)
        // Not just Ready: the first state also ends a spinner, and that must not fall into the test.
        withTimeout(TIMEOUT_MS) { model.state.first { it.desktop != null } }
        return model to transport
    }

    private fun modelWith(transport: Transport): MainViewModel =
        MainViewModel(AppContainer(Dispatchers.IO, transport, store, defaultDeviceName = "Pixel 9")).also { models += it }

    private fun MainViewModel.beginPairingWithLink() {
        onLinkEdited(link)
        requestPairing()
    }

    @Test(timeout = TIMEOUT_MS)
    fun `cancel gives the form back while the connect still blocks`() {
        val transport = BlockedTransport()
        val model = modelWith(transport)
        model.beginPairingWithLink()
        assertTrue("the connect began", transport.entered.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))
        assertEquals(PairPhase.Connecting, model.state.value.pair.phase)

        model.cancelPairing()

        // Read at once, with no wait: the connect has not returned.
        assertEquals(PairPhase.Editing, model.state.value.pair.phase)
        assertEquals("the link is dropped with the attempt", "", model.state.value.pair.link)
        transport.release.countDown()
    }

    @Test(timeout = TIMEOUT_MS)
    fun `a pairing attempt needs no started activity and outlives a stop`() = runBlocking {
        val transport = PipeTransport(fingerprint)
        val model = modelWith(transport)
        model.beginPairingWithLink()
        assertTrue("the connection opened", transport.opened.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))

        // Undispatched: the activity counts as started before the next line stops it.
        val startedActivity = launch(start = CoroutineStart.UNDISPATCHED) { model.connectWhileStarted() }
        startedActivity.cancelAndJoin()

        // The attempt goes on: it answers a hello that comes after the stop.
        FrameCodec.write(transport.toClient, hello(), Limits.MAX_FRAME_S2C_UNTIL_WELCOME)
        val answer = FrameCodec.read(transport.fromClient, Limits.MAX_FRAME_C2S_BEFORE_WELCOME)
        assertEquals("pair", answer?.getValue("type")?.jsonPrimitive?.content)
    }

    /** docs/DESIGN.md 8.4: background means disconnect. Found on a device: the blocked read kept the stop from arriving. */
    @Test(timeout = TIMEOUT_MS)
    fun `a stop closes the session while its read still blocks`() = runBlocking {
        store.save(StoredPairing(profile, ByteArray(Proofs.KEY_BYTES) { 1 }))
        val transport = PipeTransport(fingerprint)
        val model = modelWith(transport)
        val startedActivity = launch(Dispatchers.Default) { model.connectWhileStarted() }
        // No frame is ever sent: the session blocks in the read of `hello`.
        assertTrue("the connection opened", transport.opened.await(TIMEOUT_MS, TimeUnit.MILLISECONDS))

        startedActivity.cancelAndJoin()

        assertTrue("the connection is closed", transport.closed.await(CLOSE_WAIT_MS, TimeUnit.MILLISECONDS))
    }

    /** docs/PROTOCOL.md 6.4 and docs/DESIGN.md 8.4. */
    @Test(timeout = TIMEOUT_MS)
    fun `auth_failed forgets the stored pairing and is not sent again`() = runBlocking {
        store.save(StoredPairing(profile, ByteArray(Proofs.KEY_BYTES) { 1 }))
        val transport = RejectingTransport(fingerprint, RejectCode.AUTH_FAILED)
        val model = modelWith(transport)
        val startedActivity = launch(Dispatchers.Default) { model.connectWhileStarted() }

        val unpaired = withTimeout(TIMEOUT_MS) { model.state.first { it.unpaired } }

        assertNull(unpaired.server)
        assertNull("the profile and the key are gone", store.load())
        assertEquals(1, transport.opens.get())
        startedActivity.cancelAndJoin()
    }

    /** docs/PROTOCOL.md 5.2 and 13.2 rule 5: the same `auth` would be refused again. */
    @Test(timeout = TIMEOUT_MS)
    fun `a reject other than auth_failed keeps the pairing and is not retried`() = runBlocking {
        store.save(StoredPairing(profile, ByteArray(Proofs.KEY_BYTES) { 1 }))
        val transport = RejectingTransport(fingerprint, RejectCode.UNSUPPORTED_VERSION)
        val model = modelWith(transport)
        val startedActivity = launch(Dispatchers.Default) { model.connectWhileStarted() }

        val refused = withTimeout(TIMEOUT_MS) {
            model.state.first { it.connectionProblem != null && it.connection == ConnectionStatus.Disconnected }
        }

        assertEquals(false, refused.unpaired)
        assertNotNull("the pairing stays", store.load())
        assertEquals(1, transport.opens.get())
        startedActivity.cancelAndJoin()
    }

    /** The reviewed target must be the target used: the dialog was opened for the link of that moment. */
    @Test(timeout = TIMEOUT_MS)
    fun `a link that arrives while the replace dialog is open closes the dialog`() = runBlocking {
        store.save(StoredPairing(profile, key))
        val model = modelWith(BlockedTransport())
        withTimeout(TIMEOUT_MS) { model.state.first { it.server != null } }
        model.beginPairingWithLink()
        assertTrue("the dialog is open", model.state.value.pair.confirmingReplace)

        model.onPairingLink(link.replace("192.0.2.1", "192.0.2.2"))

        val form = model.state.value.pair
        assertEquals(false, form.confirmingReplace)
        assertEquals("192.0.2.2", form.target?.host)
    }

    @Test(timeout = TIMEOUT_MS)
    fun `a second tap on a target that still waits sends no second request`() = runBlocking {
        val (model, transport) = readySession()

        model.focusWindow(7)
        model.focusWindow(7)
        assertEquals(7L, transport.readClientFrame().windowId())
        model.focusWindow(8)

        // A second request for window 7 would be on the wire before this one.
        assertEquals(8L, transport.readClientFrame().windowId())
    }

    @Test(timeout = TIMEOUT_MS)
    fun `an answer ends only the spinner of its own action`() = runBlocking {
        val (model, transport) = readySession()
        model.focusWindow(7)
        val first = transport.readClientFrame()
        model.focusWindow(8)
        transport.readClientFrame()

        transport.send("""{"type":"res","id":${first.getValue("id")},"ok":false,"error":{"code":"not_found","message":"from the test"}}""")

        val refused = withTimeout(TIMEOUT_MS) { model.state.first { it.notice != null } }
        assertEquals(8L, refused.pendingTargetId)
    }

    /** docs/PROTOCOL.md 6.4, the path through `bye`. */
    @Test(timeout = TIMEOUT_MS)
    fun `bye with revoked forgets the stored pairing`() = runBlocking {
        val (model, transport) = readySession()

        transport.send("""{"type":"bye","code":"revoked","message":"from the test"}""")

        val unpaired = withTimeout(TIMEOUT_MS) { model.state.first { it.unpaired } }
        assertNull(unpaired.server)
        assertNull("the profile and the key are gone", store.load())
    }

    @Test(timeout = TIMEOUT_MS)
    fun `retry now ends the wait between two attempts`() = runBlocking {
        store.save(StoredPairing(profile, key))
        val transport = UnreachableTransport()
        val model = modelWith(transport)
        startedActivities.launch { model.connectWhileStarted() }
        // After the third failure the app waits 4 s for the next attempt.
        repeat(3) { assertTrue("attempt ${it + 1}", transport.attempts.tryAcquire(TIMEOUT_MS, TimeUnit.MILLISECONDS)) }
        withTimeout(TIMEOUT_MS) { model.state.first { it.connection == ConnectionStatus.Backoff } }

        model.retryNow()

        assertTrue("an attempt follows at once", transport.attempts.tryAcquire(RETRY_WAIT_MS, TimeUnit.MILLISECONDS))
    }

    private companion object {
        const val TIMEOUT_MS = 10_000L
        const val CLOSE_WAIT_MS = 5_000L
        const val RETRY_WAIT_MS = 2_000L
        const val SECRET_BYTES = 32
    }
}

private fun JsonObject.windowId(): Long = getValue("args").jsonObject.getValue("window_id").jsonPrimitive.long

private val serverNonce = ByteArray(Proofs.NONCE_BYTES) { 3 }

private fun hello(): JsonObject {
    val nonce = Base64Url.encode(serverNonce)
    return Json.parseToJsonElement("""{"type":"hello","protocol":{"min":1,"max":1},"nonce":"$nonce"}""").jsonObject
}

/** Every connect fails at once, like a computer that is switched off. */
private class UnreachableTransport : Transport {
    val attempts = Semaphore(0)

    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        attempts.release()
        throw IOException("the host is unreachable")
    }
}

/** A connect that returns only when the test lets it, like a host that does not answer. */
private class BlockedTransport : Transport {
    val entered = CountDownLatch(1)
    val release = CountDownLatch(1)

    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        entered.countDown()
        release.await()
        throw IOException("the host did not answer")
    }
}

/** One connection over in-memory pipes. The test plays the server through [toClient] and [fromClient]. */
private class PipeTransport(private val fingerprint: ByteArray) : Transport {
    val opened = CountDownLatch(1)
    val closed = CountDownLatch(1)
    val toClient = PipedOutputStream()
    private val clientInput = PipedInputStream(toClient, PIPE_BYTES)
    private val clientOutput = PipedOutputStream()
    val fromClient = PipedInputStream(clientOutput, PIPE_BYTES)

    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        val streams = StreamConnection(clientInput, clientOutput, fingerprint)
        opened.countDown()
        return object : Connection by streams {
            override fun close() {
                streams.close()
                // A blocked piped read ignores the close of its own end. The end of the writer wakes it.
                toClient.close()
                closed.countDown()
            }
        }
    }

    fun send(frame: String) = FrameCodec.write(toClient, Json.parseToJsonElement(frame).jsonObject, Limits.MAX_FRAME_S2C_UNTIL_WELCOME)

    fun readClientFrame(): JsonObject = checkNotNull(FrameCodec.read(fromClient, CLIENT_FRAME_LIMIT)) { "the client closed" }

    /** The server side up to a ready session: `hello`, a `welcome` with the right proof, an empty desktop. */
    fun serveSession(profile: Profile, key: ByteArray) {
        FrameCodec.write(toClient, hello(), Limits.MAX_FRAME_S2C_UNTIL_WELCOME)
        val auth = readClientFrame()
        val clientNonce = checkNotNull(Base64Url.decode(auth.getValue("nonce").jsonPrimitive.content, Proofs.NONCE_BYTES))
        val proof = Proofs.serverProof(key, Proofs.context(fingerprint, serverNonce, clientNonce), profile.deviceId)
        send(
            """{"type":"welcome","protocol":1,"device_id":"${profile.deviceId}","server_proof":"${Base64Url.encode(proof)}",""" +
                """"scopes":["state.read","control"],"ops":["ping","state.subscribe","window.focus"],"topics":["desktop"],""" +
                """"read_only":false,"server":{"name":"desk","version":"0.1.0"},""" +
                """"limits":{"max_frame_c2s":16384,"max_frame_s2c":1048576,"idle_timeout_ms":90000}}"""
        )
        val subscribe = readClientFrame()
        val desktop = """{"sway":{"online":true,"version":null},"workspaces":[],"windows":[],"truncated":false}"""
        val states = """[{"topic":"desktop","rev":1,"data":$desktop}]"""
        send("""{"type":"res","id":${subscribe.getValue("id")},"ok":true,"result":{"states":$states}}""")
    }

    private companion object {
        const val PIPE_BYTES = 1 shl 16
        const val CLIENT_FRAME_LIMIT = 16384
    }
}

/** Every connection gets `hello` and then `reject` with [code]. */
private class RejectingTransport(private val fingerprint: ByteArray, private val code: String) : Transport {
    val opens = AtomicInteger(0)

    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        opens.incrementAndGet()
        val reject = Json.parseToJsonElement("""{"type":"reject","code":"$code","message":"from the test"}""").jsonObject
        val frames = ByteArrayOutputStream()
        FrameCodec.write(frames, hello(), Limits.MAX_FRAME_S2C_UNTIL_WELCOME)
        FrameCodec.write(frames, reject, Limits.MAX_FRAME_S2C_UNTIL_WELCOME)
        return StreamConnection(ByteArrayInputStream(frames.toByteArray()), ByteArrayOutputStream(), fingerprint)
    }
}

private class StreamConnection(
    override val input: InputStream,
    override val output: OutputStream,
    override val fingerprint: ByteArray,
) : Connection {
    override fun close() {
        input.close()
        output.close()
    }
}
