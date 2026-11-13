package com.swayconnect.protocol

import java.io.Closeable
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream

/** The transport binding of docs/PROTOCOL.md section 3, without the frames. */
interface Transport {
    /**
     * Connects and completes the TLS handshake. Blocking.
     * It throws [PinMismatchException] when the server certificate is not the pinned one,
     * and it must bound the connect and the handshake with a timeout of its own.
     */
    @Throws(IOException::class)
    fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection
}

interface Connection : Closeable {
    val input: InputStream
    val output: OutputStream

    /** SHA-256 of the leaf certificate presented on this connection. It is the `fp` of the proofs. */
    val fingerprint: ByteArray

    /** May be called twice and from any thread. It makes a blocked read on [input] fail. */
    override fun close()
}

/** The server presented another certificate. This is not a network failure: show it as its own error. */
class PinMismatchException : IOException("server certificate does not match the pinned fingerprint")
