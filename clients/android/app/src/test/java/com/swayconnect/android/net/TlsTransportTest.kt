package com.swayconnect.android.net

import com.swayconnect.protocol.Limits
import com.swayconnect.protocol.PinMismatchException
import java.io.Closeable
import java.io.IOException
import java.net.InetAddress
import java.net.Socket
import java.security.Principal
import java.security.PrivateKey
import java.security.cert.X509Certificate
import javax.net.ssl.KeyManager
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLException
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509ExtendedKeyManager
import kotlin.concurrent.thread
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** [TlsTransport] against a plain JSSE server on a loopback port that the system picks. */
class TlsTransportTest {
    private val transport = TlsTransport()

    @Test
    fun `the pinned certificate with the right ALPN opens a connection`() {
        TlsTestServer(TLS_1_3, TestIdentity.key, Limits.ALPN).use { server ->
            transport.open(LOOPBACK, server.port, TestIdentity.fingerprint).use { connection ->
                assertArrayEquals(TestIdentity.fingerprint, connection.fingerprint)
            }
        }
    }

    @Test
    fun `a certificate that is not pinned is a pin mismatch`() {
        TlsTestServer(TLS_1_3, TestIdentity.key, Limits.ALPN).use { server ->
            assertThrows(PinMismatchException::class.java) {
                transport.open(LOOPBACK, server.port, TestIdentity.otherFingerprint())
            }
        }
    }

    /** docs/PROTOCOL.md 15.13: the fingerprint is right, the handshake signature is not. */
    @Test
    fun `the pinned certificate with another private key fails in the handshake`() {
        val server = TlsTestServer(TLS_1_3, TestIdentity.wrongKey, Limits.ALPN)
        server.use {
            // An SSLException is never a PinMismatchException: the pin held, the signature did not.
            assertThrows(SSLException::class.java) {
                transport.open(LOOPBACK, server.port, TestIdentity.fingerprint)
            }
        }

        assertEquals(0, server.applicationBytes)
    }

    @Test
    fun `a server that selects no ALPN protocol is refused`() {
        TlsTestServer(TLS_1_3, TestIdentity.key, alpn = null).use { server ->
            assertThrows(SSLException::class.java) {
                transport.open(LOOPBACK, server.port, TestIdentity.fingerprint)
            }
        }
    }

    @Test
    fun `a server without TLS 1_3 is refused`() {
        TlsTestServer(TLS_1_2, TestIdentity.key, Limits.ALPN).use { server ->
            assertThrows(IOException::class.java) {
                transport.open(LOOPBACK, server.port, TestIdentity.fingerprint)
            }
        }
    }

    private companion object {
        const val LOOPBACK = "127.0.0.1"
        const val TLS_1_3 = "TLSv1.3"
        const val TLS_1_2 = "TLSv1.2"
    }
}

/**
 * Serves one TLS connection with the vector certificate and [key], then counts the application
 * bytes the client sends. [protocol] names the only TLS version the server speaks.
 */
private class TlsTestServer(protocol: String, key: PrivateKey, alpn: String?) : Closeable {
    private val listener: SSLServerSocket
    private val worker: Thread

    @Volatile
    var applicationBytes = 0
        private set

    val port: Int
        get() = listener.localPort

    init {
        val context = SSLContext.getInstance(protocol)
        context.init(arrayOf<KeyManager>(FixedKeyManager(TestIdentity.certificate, key)), null, null)
        listener =
            context.serverSocketFactory.createServerSocket(0, 1, InetAddress.getLoopbackAddress()) as SSLServerSocket
        // The context name limits only what a client offers: a JSSE server socket enables TLS 1.3 either way.
        listener.enabledProtocols = arrayOf(protocol)
        if (alpn != null) {
            val parameters = listener.sslParameters
            parameters.applicationProtocols = arrayOf(alpn)
            listener.sslParameters = parameters
        }
        worker = thread(isDaemon = true) { serveOne() }
    }

    private fun serveOne() {
        try {
            (listener.accept() as SSLSocket).use { socket ->
                socket.soTimeout = TIMEOUT_MS
                socket.startHandshake()
                val input = socket.inputStream
                while (input.read() >= 0) applicationBytes += 1
            }
        } catch (e: IOException) {
            // Expected in the refusal cases: the client aborts the handshake or never connects.
        }
    }

    override fun close() {
        listener.close()
        worker.join(TIMEOUT_MS.toLong())
    }

    private companion object {
        const val TIMEOUT_MS = 10_000
    }
}

/**
 * Presents [certificate] with [key] whether or not they belong together. The default key manager
 * of the JDK cannot be given a mismatched pair, which case 15.13 needs.
 */
private class FixedKeyManager(private val certificate: X509Certificate, private val key: PrivateKey) :
    X509ExtendedKeyManager() {
    override fun getClientAliases(keyType: String?, issuers: Array<out Principal>?): Array<String>? = null

    override fun chooseClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, socket: Socket?): String? =
        null

    override fun getServerAliases(keyType: String?, issuers: Array<out Principal>?): Array<String>? =
        if (keyType == EC) arrayOf(ALIAS) else null

    override fun chooseServerAlias(keyType: String?, issuers: Array<out Principal>?, socket: Socket?): String? =
        if (keyType == EC) ALIAS else null

    override fun getCertificateChain(alias: String?): Array<X509Certificate>? =
        if (alias == ALIAS) arrayOf(certificate) else null

    override fun getPrivateKey(alias: String?): PrivateKey? = if (alias == ALIAS) key else null

    private companion object {
        const val ALIAS = "test"
        const val EC = "EC"
    }
}
