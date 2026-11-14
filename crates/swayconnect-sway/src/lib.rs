//! sway IPC client. The only crate that builds sway command text.

#![forbid(unsafe_code)]

pub mod command;
pub mod connection;
pub mod events;
mod frame;
pub mod socket;
#[cfg(test)]
mod test_socket;
pub mod types;

pub use command::{Command, LayoutKind, WorkspaceNumber};
pub use connection::{Connection, Error, REQUEST_TIMEOUT};
pub use events::{Event, EventStream};
pub use socket::{EnvSnapshot, SwayTarget, resolve};
pub use types::{Node, Rect, Version, WindowProperties, WorkspaceInfo};
