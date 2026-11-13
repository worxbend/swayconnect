package com.swayconnect.protocol

/** Numbers of docs/PROTOCOL.md section 14 that a client needs. */
object Limits {
    const val PROTOCOL_VERSION = 1
    const val ALPN = "swayconnect/1"
    const val DEFAULT_PORT = 28414

    const val MAX_FRAME_C2S_BEFORE_WELCOME = 4096
    const val MAX_FRAME_S2C_UNTIL_WELCOME = 65536

    /** `limits.max_frame_s2c` of this protocol version. A `welcome` that announces more is refused. */
    const val MAX_FRAME_S2C = 1_048_576

    /** For `hello` and `welcome` together. The server allows itself 5 s and the client 2 s (section 14). */
    const val HANDSHAKE_TIMEOUT_MS = 10_000L

    /** The server closes after 30 s without a client frame; pinging earlier leaves room for a slow link. */
    const val PING_INTERVAL_MS = 20_000L
    const val REQUEST_TIMEOUT_MS = 10_000L

    const val DISPLAY_LABEL_MAX_BYTES = 64
    const val PLATFORM_MAX_BYTES = 32
    const val CLIENT_MAX_BYTES = 64
}
