package com.swayconnect.android.net

import com.swayconnect.protocol.fingerprintOf
import java.io.ByteArrayInputStream
import java.security.KeyFactory
import java.security.PrivateKey
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.security.spec.PKCS8EncodedKeySpec
import java.util.Base64

/**
 * The test certificate and keys of docs/PROTOCOL.md 15.1, copied from
 * protocol/vectors/v1/fingerprint.json. Public test material: never use it outside tests.
 * Copied instead of read from the vector directory so that this module needs no extra build wiring.
 */
internal object TestIdentity {
    private const val CERTIFICATE_DER_B64 =
        "MIIBhDCCASugAwIBAgIUZ4XSOS1jkR7kbwc7gQpAIDRQJ/gwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMc3dheWNvbm5lY3RkMCAX" +
            "DTI2MTExMzA3NDYwN1oYDzIxMjYxMDIwMDc0NjA3WjAXMRUwEwYDVQQDDAxzd2F5Y29ubmVjdGQwWTATBgcqhkjOPQIBBggqhkjO" +
            "PQMBBwNCAASzDw/9fxVP7HRyPhXHrGuqEqmIKaTeKrxt1G/UI9tc8L9mfNBKU8A3tk1Xi/XOk+Qtfsl615H/3nEPpSh481N7o1Mw" +
            "UTAdBgNVHQ4EFgQUR8uja7And/Vaz0m1GRjgJAUPuy8wHwYDVR0jBBgwFoAUR8uja7And/Vaz0m1GRjgJAUPuy8wDwYDVR0TAQH/" +
            "BAUwAwEB/zAKBggqhkjOPQQDAgNHADBEAiA2dF6JJpOBig7ulsRmJ7NKKmSucgDergNcgGh2v5I0SwIgSvDOgvij2Lm01EVda09A" +
            "UhrVEKT6p+xNUmwlagZAMTc="
    private const val KEY_PKCS8_B64 =
        "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgJkQEVjnF0lpIwuWMEJqv0CDvkddvfRDgOMLM5Xm9NRKhRANCAASz" +
            "Dw/9fxVP7HRyPhXHrGuqEqmIKaTeKrxt1G/UI9tc8L9mfNBKU8A3tk1Xi/XOk+Qtfsl615H/3nEPpSh481N7"
    private const val WRONG_KEY_PKCS8_B64 =
        "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgC9MSjNPFGa6T+oH5THcXLEPir0UaD7e7XtQluSr4MtGhRANCAAQl" +
            "v+KYAr0zvM5+eWp/YAnbNHT0tOxT3X6wwVJxLJn2OeEaQdtrxG0///N9OaZYqN9b+WV6uKJ9MYlx9J8Vsku8"

    /** `fp_hex` of the vector file. */
    const val FINGERPRINT_HEX = "aa5108ad019ade397fbe42244fba3d963644da7c753c29a774e684275c9fc0b8"

    val certificate: X509Certificate =
        CertificateFactory.getInstance("X.509")
            .generateCertificate(ByteArrayInputStream(Base64.getDecoder().decode(CERTIFICATE_DER_B64)))
            as X509Certificate

    val fingerprint: ByteArray = fingerprintOf(certificate.encoded)

    /** The private key of [certificate]. */
    val key: PrivateKey = privateKey(KEY_PKCS8_B64)

    /** A P-256 key that does not belong to [certificate] (docs/PROTOCOL.md 15.13). */
    val wrongKey: PrivateKey = privateKey(WRONG_KEY_PKCS8_B64)

    /** A fingerprint that no certificate of these tests has. */
    fun otherFingerprint(): ByteArray = fingerprint.copyOf().also { it[0] = (it[0] + 1).toByte() }

    private fun privateKey(pkcs8Base64: String): PrivateKey =
        KeyFactory.getInstance("EC").generatePrivate(PKCS8EncodedKeySpec(Base64.getDecoder().decode(pkcs8Base64)))
}
