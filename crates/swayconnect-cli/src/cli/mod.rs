//! The commands of the `swayconnect` binary (`docs/DESIGN.md` 7.2). Written by slice S7.
//!
//! Nothing here reads the process environment: `main.rs` passes the profile directory, the
//! connector and the standard streams in.

mod args;
mod control_args;
mod duration;
mod error;
mod layout_map;
mod listing;
mod render;
mod run;
mod state;
mod store;

pub use args::{AwakeSwitch, Cli, Command, RefusedUri, Switch};
pub use control_args::{
    AudioAction, DeviceArg, InputTargetArg, KeyArgs, LaunchArgs, LauncherAction, MediaAction,
    MuteArgs, PlayArgs, RefusedText, TypeArgs, VolumeArgs,
};
pub use error::CliError;
pub use run::{Context, run};
pub use store::{DirectoryInputs, ProfileStore, StoreError, client_directory};
