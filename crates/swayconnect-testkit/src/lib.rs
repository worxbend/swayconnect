//! Test support: fake sway IPC server, fake programs, temp directories, fixtures
//! (`docs/DESIGN.md` 9.1, 11.12), and the sealed session of real programs
//! (`docs/M2C_PLAN.md` 3).
//!
//! Depends on neither `swayconnect-daemon` nor `swayconnect-protocol`.

#![forbid(unsafe_code)]

mod fake_proc;
mod fake_proc_spec;
mod fake_sway;
pub mod fixtures;
mod frame;
mod guard;
mod mpris_player;
mod real_session;
mod test_config;
mod test_dirs;
mod xdg_data;

pub use fake_proc::FakeProcs;
pub use fake_proc_spec::Mode as FakeProcMode;
pub use fake_sway::{Event, FakeSway, Request};
pub use mpris_player::mpris_test_player;
pub use real_session::{
    HEADLESS_OUTPUT, LiveEnv, RealSession, SEALED_SINKS, SEALED_SOURCE, kept_log_dir,
    leftovers_naming, live_processes_in_group, processes_naming,
};
pub use test_config::{
    ProgramSection, TestConfig, assert_no_system_programs, nonexistent_programs,
};
pub use test_dirs::{NO_SYSTEM_BUS, TestDirs};

/// File name of the fake sway socket inside its directory.
pub const SWAY_SOCKET_NAME: &str = "sway-ipc.sock";
