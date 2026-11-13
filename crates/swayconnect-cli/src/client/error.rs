//! Everything that can end a client call.

use swayconnect_protocol::frame::{EncodeError, FrameError};
use swayconnect_protocol::proof::RandomError;
use swayconnect_protocol::{ByeCode, ErrorBody, RejectCode};

/// `message` members hold server text: print them through `escape_for_terminal` only.
/// The `Display` text never contains server text.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("the server certificate does not match the pinned fingerprint")]
    PinMismatch,
    #[error("TLS handshake failed: {0}")]
    Tls(rustls::Error),
    #[error("the server did not select the ALPN protocol swayconnect/1")]
    NoAlpn,
    #[error(transparent)]
    Random(#[from] RandomError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error("the server broke the protocol: {0}")]
    Protocol(&'static str),
    #[error("no common protocol version: the server speaks {min} to {max}")]
    NoCommonVersion { min: u32, max: u32 },
    #[error("the server refused the handshake: {}", code.as_str())]
    Rejected { code: RejectCode, message: String },
    #[error("`welcome` is not trustworthy: wrong {0}")]
    BadWelcome(&'static str),
    #[error("the server closed the session: {}", code.as_str())]
    Bye { code: ByeCode, message: String },
    #[error("the connection closed")]
    Closed,
    #[error("the server did not answer a keepalive ping: the connection is dead")]
    NoAnswer,
    #[error("the request failed: {}", .0.code.as_str())]
    Failed(ErrorBody),
    #[error("the server does not offer operation `{0}` to this device")]
    NotOffered(&'static str),
    #[error("too many requests without a response")]
    TooManyPending,
}
