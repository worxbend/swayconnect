//! Test support: fake sway IPC server, temp directories, fixtures (`docs/DESIGN.md` 9.1).
//!
//! Depends on neither `swayconnect-daemon` nor `swayconnect-protocol`.

#![forbid(unsafe_code)]

mod fake_sway;
pub mod fixtures;
mod frame;
mod guard;
mod test_dirs;

pub use fake_sway::{Event, FakeSway, Request};
pub use test_dirs::TestDirs;

/// File name of the fake sway socket inside its directory.
pub const SWAY_SOCKET_NAME: &str = "sway-ipc.sock";
