package com.swayconnect.android.net

import android.annotation.SuppressLint
import com.swayconnect.protocol.fingerprintOf
import java.net.Socket
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLEngine
import javax.net.ssl.X509ExtendedTrustManager

/**
 * Trusts exactly one server certificate: the one whose SHA-256 fingerprint was pinned at pairing
 * (docs/PROTOCOL.md 3.2). No certificate authority, host name or date takes part.
 *
 * Android calls the three-argument form for an `SSLSocket`, so all three server overloads run the
 * same check. The platform still verifies the handshake signature with the key of the certificate;
 * nothing here touches that.
 */
// The lint rule warns about hand-written trust managers. This one is the pin: it accepts a chain
// only when its leaf has the pinned fingerprint and refuses every other chain.
@SuppressLint("CustomX509TrustManager")
internal class PinnedTrustManager(pinnedFingerprint: ByteArray) : X509ExtendedTrustManager() {
    private val pinned = pinnedFingerprint.copyOf()

    /** Fingerprint of the leaf the server presented. Null until a server chain was checked. */
    @Volatile
    var presentedFingerprint: ByteArray? = null
        private set

    @Throws(CertificateException::class)
    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) = checkPin(chain)

    @Throws(CertificateException::class)
    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?, socket: Socket?) =
        checkPin(chain)

    @Throws(CertificateException::class)
    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?, engine: SSLEngine?) =
        checkPin(chain)

    @Throws(CertificateException::class)
    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?): Unit =
        throw clientsNotTrusted()

    @Throws(CertificateException::class)
    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?, socket: Socket?): Unit =
        throw clientsNotTrusted()

    @Throws(CertificateException::class)
    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?, engine: SSLEngine?): Unit =
        throw clientsNotTrusted()

    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()

    private fun checkPin(chain: Array<out X509Certificate>?) {
        val leaf = chain?.firstOrNull() ?: throw CertificateException("the server presented no certificate")
        val presented = fingerprintOf(leaf.encoded)
        presentedFingerprint = presented
        if (!MessageDigest.isEqual(pinned, presented)) {
            throw CertificateException("the server certificate does not match the pinned fingerprint")
        }
    }

    private fun clientsNotTrusted() = CertificateException("this trust manager checks servers only")
}
