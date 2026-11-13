package com.swayconnect.protocol

import java.io.EOFException
import java.io.IOException
import java.security.SecureRandom
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.add

/** The optional `platform` and `client` members of `pair` and `auth`. */
class ClientInfo(val platform: String?, val client: String?) {
    init {
        require(platform == null || isInfoText(platform, Limits.PLATFORM_MAX_BYTES)) { "platform has characters the protocol refuses" }
        require(client == null || isInfoText(client, Limits.CLIENT_MAX_BYTES)) { "client has characters the protocol refuses" }
    }
}

/** A confirmed pairing. Store [key] in the most protected storage the platform offers. */
class Paired(val profile: Profile, val key: ByteArray)

/** The server broke the handshake rules, or its proof is wrong. Nothing was stored. */
class HandshakeException(message: String) : IOException(message)

/** The server answered `reject`. [code] is for switching; an unknown code counts as `bad_message`. */
class RejectedException(val code: String, serverText: String) :
    IOException("server rejected the connection: ${replaceUnsafe(code)}: ${replaceUnsafe(serverText)}")

/** The pairing connection ended without `pairing.confirmed`. Nothing was stored. */
class PairingNotConfirmedException(val end: SessionEnd) : IOException("the pairing was not confirmed")

/**
 * The server answered a request with an error, or this client did not send it ([NOT_OFFERED]).
 * An unknown [code] is a failure that is not retried.
 */
class RequestFailedException(val code: String, serverText: String, val detail: JsonObject?) :
    IOException("request failed: ${replaceUnsafe(code)}: ${replaceUnsafe(serverText)}") {
    companion object {
        /** Not a wire code: the operation is not in `welcome.ops`, so the request was never sent. */
        const val NOT_OFFERED = "not_offered"
    }
}

/** Why a connection ended after `welcome`. */
sealed interface SessionEnd {
    /** The server said why. An unknown [code] is handled like `shutdown`. */
    data class ByServer(val code: String, val message: String) : SessionEnd

    /** No `bye`: network loss, a bad frame, or a request that got no answer in time. */
    data class Lost(val cause: IOException) : SessionEnd

    /** Only on a pairing connection: the owner confirmed the device. */
    data object PairingConfirmed : SessionEnd
}

/** The waits of docs/PROTOCOL.md sections 11 and 14. Only tests shorten them. */
internal class Timing(
    val handshakeTimeoutMs: Long = Limits.HANDSHAKE_TIMEOUT_MS,
    val pingIntervalMs: Long = Limits.PING_INTERVAL_MS,
    val requestTimeoutMs: Long = Limits.REQUEST_TIMEOUT_MS,
)

private val secureRandom = SecureRandom()

private fun randomNonce(): ByteArray = ByteArray(Proofs.NONCE_BYTES).also { secureRandom.nextBytes(it) }

/**
 * The message layer of docs/PROTOCOL.md over a [Transport].
 * One call is one connection; reconnecting and backoff belong to the caller.
 *
 * @param ioDispatcher runs the blocking socket calls.
 * @param nonceSource gives 32 fresh random bytes per connection. Replaced only by the vector tests.
 */
class SwayConnectClient internal constructor(
    private val transport: Transport,
    private val ioDispatcher: CoroutineDispatcher,
    private val clientInfo: ClientInfo,
    private val nonceSource: () -> ByteArray,
    private val timing: Timing,
) {
    constructor(
        transport: Transport,
        ioDispatcher: CoroutineDispatcher,
        clientInfo: ClientInfo,
        nonceSource: () -> ByteArray = ::randomNonce,
    ) : this(transport, ioDispatcher, clientInfo, nonceSource, Timing())

    /**
     * Pairs with the server of [uri] and returns once the owner has confirmed the device on the desktop.
     * [onConfirmCode] gets the six digits to show while waiting.
     *
     * @throws PairingNotConfirmedException when the owner refuses, does not answer, or the connection breaks.
     */
    @Throws(IOException::class)
    suspend fun pair(uri: PairingUri, deviceName: String, onConfirmCode: (String) -> Unit): Paired {
        require(isDisplayLabel(deviceName)) { "device name must be 1 to 64 bytes without control characters" }
        return withConnection(uri.host, uri.port, uri.fingerprint) { connection ->
            val (exchange, welcome) = handshake(connection) {
                val request = PairRequest(
                    protocol = Limits.PROTOCOL_VERSION,
                    nonce = Base64Url.encode(it.clientNonce),
                    proof = Base64Url.encode(Proofs.pairProof(uri.secret, it.context)),
                    deviceName = deviceName,
                    platform = clientInfo.platform,
                    client = clientInfo.client,
                )
                Wire.encode(request)
            }
            val key = Proofs.deviceKey(uri.secret, exchange.context)
            verifyWelcome(welcome, key, exchange.context, welcome.deviceId)
            if (Base64Url.decode(welcome.deviceId, DEVICE_ID_BYTES) == null) throw HandshakeException("welcome has a bad device id")
            val confirmCode = welcome.confirmCode ?: throw HandshakeException("welcome after pair has no confirm code")

            onConfirmCode(confirmCode)
            val end = Session(connection, welcome, ioDispatcher, timing, awaitsConfirmation = true).runUntilEnd(this) {}
            if (end != SessionEnd.PairingConfirmed) throw PairingNotConfirmedException(end)
            Paired(
                Profile(
                    host = uri.host,
                    port = uri.port,
                    fp = Base64Url.encode(uri.fingerprint),
                    deviceId = welcome.deviceId,
                    serverName = replaceUnsafe(welcome.server.name),
                ),
                key,
            )
        }
    }

    /**
     * Authenticates and keeps the connection until it ends. [onReady] runs next to the reader, after `welcome`;
     * it is cancelled when the connection ends, and a failure in it ends the connection and is thrown from here.
     *
     * @throws RejectedException with `auth_failed` when the device is no longer paired. Do not retry in a loop.
     */
    @Throws(IOException::class)
    suspend fun runSession(profile: Profile, key: ByteArray, onReady: suspend (Session) -> Unit): SessionEnd {
        val pinned = Base64Url.decode(profile.fp, FINGERPRINT_BYTES)
            ?: throw HandshakeException("the stored fingerprint is not valid")
        return withConnection(profile.host, profile.port, pinned) { connection ->
            val (exchange, welcome) = handshake(connection) {
                val request = AuthRequest(
                    protocol = Limits.PROTOCOL_VERSION,
                    deviceId = profile.deviceId,
                    nonce = Base64Url.encode(it.clientNonce),
                    proof = Base64Url.encode(Proofs.clientProof(key, it.context, profile.deviceId)),
                    client = clientInfo.client,
                )
                Wire.encode(request)
            }
            verifyWelcome(welcome, key, exchange.context, profile.deviceId)
            Session(connection, welcome, ioDispatcher, timing, awaitsConfirmation = false).runUntilEnd(this, onReady)
        }
    }

    /**
     * Opens a connection for [block] and closes it afterwards. The socket calls block and ignore cancellation,
     * so a watcher closes the connection as soon as the scope of [block] is cancelled or fails.
     */
    private suspend fun <T> withConnection(
        host: String,
        port: Int,
        pinned: ByteArray,
        block: suspend CoroutineScope.(Connection) -> T,
    ): T {
        // Not cancellable: a connection opened while the caller is cancelled would never be closed.
        val connection = withContext(ioDispatcher + NonCancellable) { transport.open(host, port, pinned) }
        try {
            return coroutineScope {
                // Undispatched: a watcher that is cancelled before its first dispatch never runs, and
                // then nothing would close the connection under a read that already blocks.
                val closeOnCancel = launch(start = CoroutineStart.UNDISPATCHED) {
                    try {
                        awaitCancellation()
                    } finally {
                        connection.close()
                    }
                }
                try {
                    // The transport pins already. Checked again because the proofs rest on this value.
                    if (!Proofs.matches(pinned, connection.fingerprint)) throw PinMismatchException()
                    block(this, connection)
                } finally {
                    closeOnCancel.cancel()
                }
            }
        } finally {
            connection.close()
        }
    }

    private class Exchange(val clientNonce: ByteArray, val context: ByteArray)

    /**
     * Reads `hello`, sends [firstFrame] (`pair` or `auth`) and returns the `welcome`, whose proof is not
     * verified yet. The reads block with no socket timeout and the ping and request timers start only
     * after `welcome`, so a timer closes the connection of a server that stays silent.
     */
    private suspend fun CoroutineScope.handshake(
        connection: Connection,
        firstFrame: (Exchange) -> JsonObject,
    ): Pair<Exchange, Welcome> {
        val deadline = launch {
            delay(timing.handshakeTimeoutMs)
            connection.close()
        }
        try {
            val exchange = receiveHello(connection)
            return exchange to exchangeFirstFrame(connection, firstFrame(exchange))
        } finally {
            deadline.cancel()
        }
    }

    private suspend fun receiveHello(connection: Connection): Exchange {
        val hello = readHandshakeMessage(connection) as? Hello ?: throw HandshakeException("the first frame is not hello")
        if (Limits.PROTOCOL_VERSION !in hello.protocol.min..hello.protocol.max) {
            throw HandshakeException("the server does not speak protocol version ${Limits.PROTOCOL_VERSION}")
        }
        val serverNonce = Base64Url.decode(hello.nonce, Proofs.NONCE_BYTES) ?: throw HandshakeException("hello has a bad nonce")
        val clientNonce = nonceSource()
        return Exchange(clientNonce, Proofs.context(connection.fingerprint, serverNonce, clientNonce))
    }

    /** Sends `pair` or `auth` and returns the `welcome`. */
    private suspend fun exchangeFirstFrame(connection: Connection, firstFrame: JsonObject): Welcome {
        withContext(ioDispatcher) {
            FrameCodec.write(connection.output, firstFrame, Limits.MAX_FRAME_C2S_BEFORE_WELCOME)
        }
        return when (val answer = readHandshakeMessage(connection)) {
            is Welcome -> answer
            is Reject -> throw RejectedException(answer.code, answer.message)
            else -> throw HandshakeException("the answer is neither welcome nor reject")
        }
    }

    private suspend fun readHandshakeMessage(connection: Connection): ServerMessage? {
        val frame = withContext(ioDispatcher) { FrameCodec.read(connection.input, Limits.MAX_FRAME_S2C_UNTIL_WELCOME) }
            ?: throw EOFException("the server closed the connection during the handshake")
        return Wire.decodeServerMessage(frame)
    }

    /** docs/PROTOCOL.md 5.1 rule 4. Nothing else in [welcome] is trusted before this passes. */
    private fun verifyWelcome(welcome: Welcome, key: ByteArray, context: ByteArray, deviceId: String) {
        val serverProof = Base64Url.decode(welcome.serverProof, Proofs.PROOF_BYTES)
        if (serverProof == null || !Proofs.matches(Proofs.serverProof(key, context, deviceId), serverProof)) {
            throw HandshakeException("the server proof is wrong")
        }
        if (welcome.deviceId != deviceId) throw HandshakeException("welcome names another device")
        if (welcome.protocol != Limits.PROTOCOL_VERSION) throw HandshakeException("welcome names another protocol version")
        // The frame reader allocates what a length prefix announces, up to this limit.
        if (welcome.limits.maxFrameS2c > Limits.MAX_FRAME_S2C) {
            throw HandshakeException("welcome announces frames larger than this client reads")
        }
    }

    private companion object {
        const val DEVICE_ID_BYTES = 16
    }
}

/** A connection after `welcome`. Requests may be sent from any coroutine while the connection lives. */
class Session internal constructor(
    private val connection: Connection,
    val welcome: Welcome,
    private val ioDispatcher: CoroutineDispatcher,
    private val timing: Timing,
    private val awaitsConfirmation: Boolean,
) {
    private val writeLock = Mutex()
    private val nextRequestId = AtomicLong(1)
    private val unanswered = ConcurrentHashMap<Long, CompletableDeferred<Res>>()

    private val stateLock = Any()
    private var desktopWanted = false
    private var appliedDesktopRev: Long? = null
    private val desktopState = MutableStateFlow<Desktop?>(null)

    /**
     * The newest `desktop` state of this connection, after the client rules of 17.4 ([normalized]).
     * Null until [subscribeDesktop] has an answer.
     */
    val desktop: StateFlow<Desktop?> = desktopState.asStateFlow()

    /** Reads frames until the connection ends. [onReady] and the keepalive run in [scope] next to it. */
    internal suspend fun runUntilEnd(scope: CoroutineScope, onReady: suspend (Session) -> Unit): SessionEnd {
        val user = scope.launch { onReady(this@Session) }
        val keepalive = scope.launch { pingForever() }
        try {
            return readUntilEnd()
        } finally {
            user.cancel()
            keepalive.cancel()
            val closed = IOException("the connection is closed")
            unanswered.values.forEach { it.completeExceptionally(closed) }
        }
    }

    /**
     * Sends one request and waits for its response.
     *
     * @throws RequestFailedException when the server answers with an error. The connection stays open.
     * @throws IOException when the connection ends first or no answer comes in time. An action is not
     *   repeated then: reconnect, read the state and let the user decide.
     */
    @Throws(IOException::class)
    suspend fun request(op: String, args: JsonObject? = null): JsonObject {
        // docs/PROTOCOL.md 13.2 rule 7: an operation that is not listed is never called.
        if (op !in welcome.ops) {
            throw RequestFailedException(RequestFailedException.NOT_OFFERED, "the server does not offer $op", null)
        }
        return call(op, args)
    }

    /** Subscribes to topic `desktop`. Afterwards [desktop] follows the server. */
    @Throws(IOException::class)
    suspend fun subscribeDesktop() {
        // Set before the request: a state.changed can be read before this coroutine sees the response.
        synchronized(stateLock) { desktopWanted = true }
        val args = buildJsonObject { putJsonArray("topics") { add(Topic.DESKTOP) } }
        val result = Wire.decode(SubscribeResult.serializer(), request(Op.STATE_SUBSCRIBE, args))
        result.states.forEach(::applyState)
    }

    @Throws(IOException::class)
    suspend fun focusWorkspace(workspaceId: Long) {
        request(Op.WORKSPACE_FOCUS, buildJsonObject { put("workspace_id", workspaceId) })
    }

    @Throws(IOException::class)
    suspend fun focusWorkspaceNumber(number: Int) {
        request(Op.WORKSPACE_FOCUS, buildJsonObject { put("number", number) })
    }

    @Throws(IOException::class)
    suspend fun focusWindow(windowId: Long) {
        request(Op.WINDOW_FOCUS, buildJsonObject { put("window_id", windowId) })
    }

    @Throws(IOException::class)
    suspend fun setFullscreen(windowId: Long, enabled: Boolean) {
        val args = buildJsonObject {
            put("window_id", windowId)
            put("enabled", enabled)
        }
        request(Op.WINDOW_FULLSCREEN, args)
    }

    /**
     * Fetches icons by the hashes of [Window.icon] or [ScratchpadWindow.icon] (docs/PROTOCOL.md 17.5).
     * Each kept icon matched its hash and its PNG header is within 256 x 256; decode it as untrusted input still.
     * After `rate_limited`, wait `detail.retry_after_ms` before the next call.
     *
     * @param hashes 1 to 8 distinct values for which [IconHash.isValid] holds.
     * @throws MalformedMessageException when the result breaks the wire format.
     */
    @Throws(IOException::class)
    suspend fun iconGet(hashes: List<String>): IconBatch {
        require(hashes.size in 1..IconHash.MAX_PER_REQUEST) { "icon.get takes 1 to ${IconHash.MAX_PER_REQUEST} hashes" }
        require(hashes.toSet().size == hashes.size) { "icon hashes must be distinct" }
        require(hashes.all(IconHash::isValid)) { "an icon hash is not a canonical 16-byte base64url value" }
        val args = buildJsonObject { putJsonArray("hashes") { hashes.forEach { add(it) } } }
        return readIconResult(hashes, request(Op.ICON_GET, args))
    }

    private suspend fun call(op: String, args: JsonObject?): JsonObject {
        val id = nextRequestId.getAndIncrement()
        val answer = CompletableDeferred<Res>()
        unanswered[id] = answer
        try {
            send(Wire.encode(Req(id, op, args)))
            val response = withTimeoutOrNull(timing.requestTimeoutMs) { answer.await() }
            if (response == null) {
                // A request without an answer means a dead connection (docs/PROTOCOL.md section 11 rule 4).
                connection.close()
                throw IOException("no response to $op in ${timing.requestTimeoutMs} ms")
            }
            return resultOf(response)
        } finally {
            unanswered.remove(id)
        }
    }

    private fun resultOf(response: Res): JsonObject {
        if (response.ok) return response.result ?: throw MalformedMessageException("a successful response has no result")
        val error = response.error ?: throw MalformedMessageException("a failed response has no error")
        throw RequestFailedException(error.code, error.message, error.detail)
    }

    private suspend fun send(frame: JsonObject) {
        writeLock.withLock {
            withContext(ioDispatcher) { FrameCodec.write(connection.output, frame, welcome.limits.maxFrameC2s) }
        }
    }

    private suspend fun pingForever() {
        while (true) {
            delay(timing.pingIntervalMs)
            try {
                call(Op.PING, null)
            } catch (e: RequestFailedException) {
                // An error response is still a frame in each direction: the connection is alive.
            } catch (e: IOException) {
                // The connection is gone. The reader reports why.
                return
            }
        }
    }

    private suspend fun readUntilEnd(): SessionEnd {
        try {
            while (true) {
                val frame = withContext(ioDispatcher) { FrameCodec.read(connection.input, welcome.limits.maxFrameS2c) }
                    ?: return SessionEnd.Lost(EOFException("the server closed the connection without bye"))
                val end = handle(Wire.decodeServerMessage(frame))
                if (end != null) return end
            }
        } catch (e: IOException) {
            return SessionEnd.Lost(e)
        }
    }

    /** Returns the reason when [message] ends the connection, else null. */
    private fun handle(message: ServerMessage?): SessionEnd? =
        when (message) {
            is Res -> {
                unanswered[message.id]?.complete(message)
                null
            }
            is Evt -> handleEvent(message)
            is Bye -> SessionEnd.ByServer(message.code, message.message)
            is Hello, is Welcome, is Reject -> throw MalformedMessageException("a handshake message after welcome")
            // A type this client does not know (docs/PROTOCOL.md 13.2 rule 2).
            null -> null
        }

    private fun handleEvent(event: Evt): SessionEnd? {
        when (event.event) {
            EventName.STATE_CHANGED -> applyState(Wire.decode(TopicState.serializer(), event.data))
            EventName.PAIRING_CONFIRMED -> if (awaitsConfirmation) return SessionEnd.PairingConfirmed
        }
        return null
    }

    /** docs/PROTOCOL.md 8.2: a state is applied only when its `rev` is higher than the applied one. */
    private fun applyState(state: TopicState) {
        synchronized(stateLock) {
            if (state.topic != Topic.DESKTOP || !desktopWanted) return
            val applied = appliedDesktopRev
            if (applied != null && state.rev <= applied) return
            desktopState.value = Wire.decode(DesktopSerializer, state.data).normalized()
            appliedDesktopRev = state.rev
        }
    }
}
