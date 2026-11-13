package com.swayconnect.android.net

import java.net.Socket
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLEngine
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class PinnedTrustManagerTest {
    private val chain = arrayOf(TestIdentity.certificate)

    /** The three server overloads, each as a call on a manager and a chain. */
    private val serverChecks: List<(PinnedTrustManager, Array<X509Certificate>?) -> Unit> =
        listOf(
            { manager, presented -> manager.checkServerTrusted(presented, AUTH_TYPE) },
            { manager, presented -> manager.checkServerTrusted(presented, AUTH_TYPE, null as Socket?) },
            { manager, presented -> manager.checkServerTrusted(presented, AUTH_TYPE, null as SSLEngine?) },
        )

    @Test
    fun `the vector certificate has the vector fingerprint`() {
        val hex = TestIdentity.fingerprint.joinToString("") { byte -> (byte.toInt() and 0xFF).toString(16).padStart(2, '0') }

        assertEquals(TestIdentity.FINGERPRINT_HEX, hex)
    }

    @Test
    fun `every server overload accepts the pinned certificate`() {
        for (checkServer in serverChecks) {
            val manager = PinnedTrustManager(TestIdentity.fingerprint)

            checkServer(manager, chain)

            assertArrayEquals(TestIdentity.fingerprint, manager.presentedFingerprint)
        }
    }

    @Test
    fun `every server overload refuses a certificate that is not pinned`() {
        for (checkServer in serverChecks) {
            val manager = PinnedTrustManager(TestIdentity.otherFingerprint())

            assertThrows(CertificateException::class.java) { checkServer(manager, chain) }

            assertArrayEquals(TestIdentity.fingerprint, manager.presentedFingerprint)
        }
    }

    @Test
    fun `every server overload refuses an empty or missing chain`() {
        for (checkServer in serverChecks) {
            val manager = PinnedTrustManager(TestIdentity.fingerprint)

            assertThrows(CertificateException::class.java) { checkServer(manager, emptyArray()) }
            assertThrows(CertificateException::class.java) { checkServer(manager, null) }

            assertNull(manager.presentedFingerprint)
        }
    }

    @Test
    fun `every client overload throws`() {
        val manager = PinnedTrustManager(TestIdentity.fingerprint)

        assertThrows(CertificateException::class.java) { manager.checkClientTrusted(chain, AUTH_TYPE) }
        assertThrows(CertificateException::class.java) { manager.checkClientTrusted(chain, AUTH_TYPE, null as Socket?) }
        assertThrows(CertificateException::class.java) { manager.checkClientTrusted(chain, AUTH_TYPE, null as SSLEngine?) }
    }

    @Test
    fun `no issuer is accepted`() {
        assertEquals(0, PinnedTrustManager(TestIdentity.fingerprint).acceptedIssuers.size)
    }

    private companion object {
        const val AUTH_TYPE = "ECDHE_ECDSA"
    }
}
