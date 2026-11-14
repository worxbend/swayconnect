//! The swayconnect wire contract in Rust. `docs/PROTOCOL.md` is normative.
//!
//! No async runtime and no I/O in this crate.

#![forbid(unsafe_code)]

pub mod apps;
pub mod audio;
pub mod availability;
pub mod code;
pub mod desktop;
pub mod fingerprint;
pub mod frame;
pub mod icon;
pub mod input;
mod json;
pub mod limits;
pub mod media;
pub mod message;
pub mod ops;
pub mod proof;
pub mod session;
pub mod text;
pub mod uri;

pub use apps::{App, Apps, EntryId, LauncherOpenResult};
pub use audio::{Audio, AudioChange, Device, DeviceKind};
pub use availability::Availability;
pub use code::{ByeCode, ErrorCode, RejectCode};
pub use desktop::{
    BasisRect, Container, ContainerKind, Desktop, Layout, Output, Rect, ScratchpadWindow,
    SwayStatus, Tile, Transform, Window, Workspace,
};
pub use icon::{Icon, IconGetResult, IconHash, IconRejection};
pub use input::{InputRefusal, InputTarget, InputText, InputTextResult, Key, KeyPress, Modifier};
pub use json::InvalidMember;
pub use media::{Media, MediaCommand, Player, PlayerStatus};
pub use message::{
    Auth, Bye, DecodeError, DeviceId, ErrorBody, Evt, Hello, JsonObject, Limits, Message, Pair,
    ProtocolRange, Reject, Req, Res, Scope, ServerInfo, Welcome,
};
pub use session::{Held, Holder, KeepAwakeResult, KeepAwakeState, LockResult, Session};
