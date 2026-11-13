package com.swayconnect.protocol

import java.io.ByteArrayOutputStream

/**
 * A parsed pairing URI (docs/PROTOCOL.md section 4). It holds the pairing secret:
 * never log it, never store it after the pairing attempt ends.
 */
class PairingUri(
    val host: String,
    val port: Int,
    val fingerprint: ByteArray,
    val secret: ByteArray,
    val name: String?,
)

/** The text is not a valid pairing URI. The message never quotes the URI, because the URI holds the secret. */
class PairingUriException(message: String) : Exception(message)

private const val PREFIX = "swayconnect://pair"
private const val SUPPORTED_URI_VERSION = "1"
private val KNOWN_PARAMETERS = setOf("v", "host", "port", "fp", "secret", "name")
private val PORT_PATTERN = Regex("[1-9][0-9]{0,4}")
private const val MAX_PORT = 65535

// android.net.Uri and java.net.URLDecoder are not used: both turn "+" into a space.
@Throws(PairingUriException::class)
fun parsePairingUri(uri: String): PairingUri {
    if (!hasPairingPrefix(uri)) throw PairingUriException("not a swayconnect://pair link")
    val afterAuthority = uri.substring(PREFIX.length)
    if (!afterAuthority.startsWith("?")) throw PairingUriException("the link must have no path and must have parameters")
    if ('#' in afterAuthority) throw PairingUriException("the link must not have a fragment")

    val parameters = knownParameters(afterAuthority.substring(1))
    if (required(parameters, "v") != SUPPORTED_URI_VERSION) throw PairingUriException("unknown link version")
    return PairingUri(
        host = host(required(parameters, "host")),
        port = port(required(parameters, "port")),
        fingerprint = bytes32(required(parameters, "fp"), "fp"),
        secret = bytes32(required(parameters, "secret"), "secret"),
        name = parameters["name"]?.let(::displayName),
    )
}

/**
 * Scheme and authority, compared without case in ASCII only. `startsWith(ignoreCase = true)` folds
 * Unicode: it takes U+017F for "s" and U+0131 for "i".
 */
private fun hasPairingPrefix(uri: String): Boolean =
    uri.length >= PREFIX.length && PREFIX.indices.all { asciiLowercase(uri[it]) == PREFIX[it] }

private fun asciiLowercase(character: Char): Char =
    if (character in 'A'..'Z') character + ('a' - 'A') else character

/** Splits the query and percent-decodes the parameters this version knows. Others are ignored unread. */
private fun knownParameters(query: String): Map<String, String> {
    val parameters = mutableMapOf<String, String>()
    for (parameter in query.split('&')) {
        val separator = parameter.indexOf('=')
        if (separator < 0) throw PairingUriException("a parameter has no \"=\"")
        val name = parameter.substring(0, separator)
        if (name !in KNOWN_PARAMETERS) continue
        if (name in parameters) throw PairingUriException("parameter \"$name\" appears twice")
        parameters[name] = percentDecode(parameter.substring(separator + 1), name)
    }
    return parameters
}

private fun required(parameters: Map<String, String>, name: String): String =
    parameters[name] ?: throw PairingUriException("parameter \"$name\" is missing")

private fun percentDecode(value: String, name: String): String {
    val decoded = ByteArrayOutputStream(value.length)
    var index = 0
    while (index < value.length) {
        if (value[index] != '%') {
            // The whole run at once: a character outside the BMP is two chars, and half of it has no UTF-8 form.
            val end = value.indexOf('%', index).let { if (it < 0) value.length else it }
            decoded.write(value.substring(index, end).toByteArray(Charsets.UTF_8))
            index = end
            continue
        }
        val high = hexDigit(value, index + 1)
        val low = hexDigit(value, index + 2)
        if (high < 0 || low < 0) throw PairingUriException("parameter \"$name\" has a bad percent sequence")
        decoded.write(high * 16 + low)
        index += 3
    }
    return decodeUtf8OrNull(decoded.toByteArray()) ?: throw PairingUriException("parameter \"$name\" is not UTF-8")
}

/** Value of the ASCII hex digit at [index], or -1 when there is none. Character.digit also takes other scripts. */
private fun hexDigit(text: String, index: Int): Int {
    if (index >= text.length) return -1
    return when (val character = text[index]) {
        in '0'..'9' -> character - '0'
        in 'a'..'f' -> character - 'a' + 10
        in 'A'..'F' -> character - 'A' + 10
        else -> -1
    }
}

// The host is only handed to the socket API, which resolves or refuses it. The checks here keep out
// what the spec excludes by name (brackets, zone) and text that would be unsafe to show.
private fun host(value: String): String {
    val unsafe = value.isEmpty() || hasControlOrFormat(value) || value.any { it in "[]% " }
    if (unsafe) throw PairingUriException("parameter \"host\" is not a host name or an address")
    return value
}

private fun port(value: String): Int {
    val port = if (PORT_PATTERN.matches(value)) value.toInt() else 0
    if (port !in 1..MAX_PORT) throw PairingUriException("parameter \"port\" is not a port from 1 to $MAX_PORT")
    return port
}

private fun bytes32(value: String, name: String): ByteArray =
    Base64Url.decode(value, FINGERPRINT_BYTES)
        ?: throw PairingUriException("parameter \"$name\" is not base64url of 32 bytes")

private fun displayName(value: String): String {
    if (!isDisplayLabel(value)) throw PairingUriException("parameter \"name\" is empty, too long or has unsafe characters")
    return value
}
