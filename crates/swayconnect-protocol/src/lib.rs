//! The swayconnect wire contract in Rust. `docs/PROTOCOL.md` is normative.
//!
//! No async runtime and no I/O in this crate.

#![forbid(unsafe_code)]

pub mod code;
pub mod desktop;
pub mod fingerprint;
pub mod frame;
mod json;
pub mod limits;
pub mod message;
pub mod ops;
pub mod proof;
pub mod text;
pub mod uri;

pub use code::{ByeCode, ErrorCode, RejectCode};
pub use desktop::{Desktop, SwayStatus, Window, Workspace};
pub use json::InvalidMember;
pub use message::{
    Auth, Bye, DecodeError, DeviceId, ErrorBody, Evt, Hello, JsonObject, Limits, Message, Pair,
    ProtocolRange, Reject, Req, Res, Scope, ServerInfo, Welcome,
};
