//! The commands of the `swayconnect` binary (`docs/DESIGN.md` 7.2). Written by slice S7.
//!
//! Nothing here reads the process environment: `main.rs` passes the profile directory, the
//! connector and the standard streams in.

mod args;
mod error;
mod render;
mod run;
mod state;
mod store;

pub use args::{Cli, Command, RefusedUri, Switch};
pub use error::CliError;
pub use run::{Context, run};
pub use store::{DirectoryInputs, ProfileStore, StoreError, client_directory};
