//! Error, reject and bye codes (`docs/PROTOCOL.md` 5.2, 6.4, 10.1, 17.15).
//!
//! The wire form of each variant is its name in `snake_case`.

/// Gives an enum its wire names: `as_str` and `from_wire`.
macro_rules! wire_names {
    ($enum:ident { $($variant:ident => $name:literal,)+ }) => {
        impl $enum {
            /// The name on the wire.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// `None` for a name this version does not know.
            pub fn from_wire(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}
pub(crate) use wire_names;

/// `res.error.code` (spec 10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    BadRequest,
    UnknownOp,
    UnknownTopic,
    Forbidden,
    ReadOnly,
    RateLimited,
    NotFound,
    UnsupportedTarget,
    SwayUnavailable,
    SwayError,
    Timeout,
    Internal,
    /// A server cap applies; `detail.retry_after_ms` (17.15).
    LimitReached,
    /// The system service behind the operation is missing or down (17.15).
    BackendUnavailable,
    /// The server could not start the program (17.15).
    SpawnFailed,
    /// Remote input: the focused window is not the target (17.13, 17.15).
    FocusChanged,
    /// Remote input: the screen is locked, or lock state is unknown.
    Locked,
    /// Remote input: another surface takes the keyboard.
    KeyboardGrabbed,
    /// Remote input: no launcher this device opened is running.
    LauncherClosed,
    /// A code this version does not know. Decode only; a server never sends it.
    /// The request failed and the client does not retry (spec 13.2 rule 4).
    Unknown,
}

wire_names!(ErrorCode {
    BadRequest => "bad_request",
    UnknownOp => "unknown_op",
    UnknownTopic => "unknown_topic",
    Forbidden => "forbidden",
    ReadOnly => "read_only",
    RateLimited => "rate_limited",
    NotFound => "not_found",
    UnsupportedTarget => "unsupported_target",
    SwayUnavailable => "sway_unavailable",
    SwayError => "sway_error",
    Timeout => "timeout",
    Internal => "internal",
    LimitReached => "limit_reached",
    BackendUnavailable => "backend_unavailable",
    SpawnFailed => "spawn_failed",
    FocusChanged => "focus_changed",
    Locked => "locked",
    KeyboardGrabbed => "keyboard_grabbed",
    LauncherClosed => "launcher_closed",
    Unknown => "unknown",
});

/// `reject.code` (spec 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectCode {
    BadMessage,
    UnsupportedVersion,
    AuthFailed,
    PairingFailed,
}

wire_names!(RejectCode {
    BadMessage => "bad_message",
    UnsupportedVersion => "unsupported_version",
    AuthFailed => "auth_failed",
    PairingFailed => "pairing_failed",
});

/// `bye.code` (spec 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ByeCode {
    Shutdown,
    Revoked,
    Idle,
    BadFrame,
    TooLarge,
    ProtocolError,
    /// The owner changed this device's scopes: reconnect at once (17.15).
    ScopesChanged,
}

wire_names!(ByeCode {
    Shutdown => "shutdown",
    Revoked => "revoked",
    Idle => "idle",
    BadFrame => "bad_frame",
    TooLarge => "too_large",
    ProtocolError => "protocol_error",
    ScopesChanged => "scopes_changed",
});
