package com.swayconnect.android.net

import com.swayconnect.protocol.Connection
import com.swayconnect.protocol.Limits
import com.swayconnect.protocol.PinMismatchException
import com.swayconnect.protocol.Transport
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.security.GeneralSecurityException
import java.security.MessageDigest
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLHandshakeException
import javax.net.ssl.SSLPeerUnverifiedException
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager

/**
 * The transport binding of docs/PROTOCOL.md section 3 on the platform TLS stack.
 * Every call blocks: the caller runs it on an I/O dispatcher.
 */
internal class TlsTransport : Transport {
    @Throws(IOException::class)
    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection {
        val trust = PinnedTrustManager(pinnedFingerprint)
        val socket = newSocket(trust)
        try {
            socket.connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
            socket.soTimeout = HANDSHAKE_TIMEOUT_MS
            socket.startHandshake()
            if (socket.applicationProtocol != Limits.ALPN) {
                throw SSLHandshakeException("the server did not select ${Limits.ALPN}")
            }
            // A handshake that never asked the trust manager (a resumed one) has no checked certificate.
            val presented = trust.presentedFingerprint
                ?: throw SSLPeerUnverifiedException("the server certificate was not checked")
            // After the handshake the message layer bounds every wait with its handshake, ping and request timers.
            socket.soTimeout = 0
            return TlsConnection(socket, presented)
        } catch (failure: IOException) {
            closeAfterFailure(socket, failure)
            val presented = trust.presentedFingerprint
            val otherCertificate = presented != null && !MessageDigest.isEqual(pinnedFingerprint, presented)
            throw if (otherCertificate) PinMismatchException() else failure
        }
    }

    @Throws(IOException::class)
    private fun newSocket(trust: PinnedTrustManager): SSLSocket {
        // A new context per connection has an empty session cache, so no handshake is resumed
        // (docs/PROTOCOL.md 3.1 rule 10).
        val context =
            try {
                SSLContext.getInstance(TLS_1_3).apply { init(null, arrayOf<TrustManager>(trust), null) }
            } catch (e: GeneralSecurityException) {
                throw IOException("TLS 1.3 is not available on this device", e)
            }
        val socket = context.socketFactory.createSocket() as SSLSocket
        socket.enabledProtocols = arrayOf(TLS_1_3)
        // No endpoint identification algorithm is set: the host name plays no part, only the pin decides.
        val parameters = socket.sslParameters
        parameters.applicationProtocols = arrayOf(Limits.ALPN)
        socket.sslParameters = parameters
        return socket
    }

    private fun closeAfterFailure(socket: SSLSocket, failure: IOException) {
        try {
            socket.close()
        } catch (closeFailure: IOException) {
            failure.addSuppressed(closeFailure)
        }
    }

    private companion object {
        const val TLS_1_3 = "TLSv1.3"
        const val CONNECT_TIMEOUT_MS = 10_000
        const val HANDSHAKE_TIMEOUT_MS = 10_000
    }
}

private class TlsConnection(private val socket: SSLSocket, override val fingerprint: ByteArray) : Connection {
    override val input: InputStream = socket.inputStream
    override val output: OutputStream = socket.outputStream

    override fun close() {
        try {
            socket.close()
        } catch (e: IOException) {
            // The connection is given up either way. A throw from here would replace the result of
            // the session that just ended, for example a confirmed pairing.
        }
    }
}
