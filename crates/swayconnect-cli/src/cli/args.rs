//! Command line grammar.

use std::convert::Infallible;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use swayconnect_protocol::IconHash;
use swayconnect_protocol::limits::MAX_FOCUS_NUMBER;

use super::control_args::{
    AudioAction, KeyArgs, LaunchArgs, LauncherAction, MediaAction, MuteArgs, PlayArgs, TypeArgs,
    VolumeArgs,
};
use super::duration::parse_duration_s;

#[derive(Debug, Parser)]
#[command(
    name = "swayconnect",
    version,
    about = "Reference client of swayconnect"
)]
pub struct Cli {
    /// Print one JSON value per line
    #[arg(long, global = true)]
    pub json: bool,

    /// Seconds to wait for the connection and for each answer
    #[arg(
        long,
        global = true,
        value_name = "SECONDS",
        default_value_t = 10,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    timeout: u64,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Pair with a server. Reads the pairing URI as one line from stdin
    Pair {
        /// Name the server shows for this device
        #[arg(long, value_name = "TEXT")]
        name: Option<String>,

        // Caught here so that `run` can refuse it without echoing the secret, which a clap
        // "unexpected argument" message would do.
        #[arg(hide = true, value_name = "URI", value_parser = refuse_uri)]
        uri_arguments: Vec<RefusedUri>,
    },
    /// Print what the server told this device at login
    Info,
    /// Print the desktop state in full
    State,
    /// List the workspaces
    Workspaces,
    /// List the windows
    Windows,
    /// Print one line per state change, until the connection ends
    Watch,
    /// Focus a workspace, by its number or with --id by its id
    Workspace {
        /// Workspace number. Sway creates the workspace when none has it
        #[arg(
            value_name = "NUMBER",
            value_parser = clap::value_parser!(u16).range(..=i64::from(MAX_FOCUS_NUMBER)),
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        number: Option<u16>,

        /// Id of a workspace, as `workspaces` lists it
        #[arg(long, value_name = "ID")]
        id: Option<u64>,
    },
    /// Focus a window
    Focus {
        /// Id of a window, as `windows` lists it
        window_id: u64,
    },
    /// Make a window fullscreen, or end its fullscreen
    Fullscreen {
        /// Id of a window, as `windows` lists it
        window_id: u64,
        switch: Switch,
    },
    /// Draw the tiling layout of a workspace, by its number or with --id by its id
    Layout {
        /// Workspace number, as `workspaces` lists it
        #[arg(
            value_name = "NUMBER",
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        number: Option<u32>,

        /// Id of a workspace, as `workspaces` lists it
        #[arg(long, value_name = "ID")]
        id: Option<u64>,
    },
    /// Fetch one app icon by its hash, as `windows --json` lists it, and save it as a PNG
    Icon {
        /// Icon hash: 22 characters of base64url
        #[arg(value_name = "HASH", value_parser = parse_icon_hash)]
        hash: IconHash,

        /// File to write the PNG to. An existing file is replaced
        #[arg(short, long = "output", value_name = "FILE")]
        output: PathBuf,
    },
    /// Move a window to a workspace, by its number or with --id by its id
    Move {
        /// Id of a window, as `windows` lists it
        window_id: u64,

        /// Workspace number. Sway creates the workspace when none has it
        #[arg(
            value_name = "NUMBER",
            value_parser = clap::value_parser!(u16).range(..=i64::from(MAX_FOCUS_NUMBER)),
            required_unless_present = "id",
            conflicts_with = "id"
        )]
        number: Option<u16>,

        /// Id of a workspace, as `workspaces` lists it
        #[arg(long, value_name = "ID")]
        id: Option<u64>,
    },
    /// Make a window floating, or tile it again
    Float {
        /// Id of a window, as `windows` lists it
        window_id: u64,
        switch: Switch,
    },
    /// Ask the app of a window to close it. The app may refuse or ask to save
    Close {
        /// Id of a window, as `windows` lists it
        window_id: u64,

        /// Confirm the close. Without it nothing is sent
        #[arg(long)]
        yes: bool,
    },
    /// Lock the screen with the lock command of the server
    Lock,
    /// Keep the desktop awake for a while, or end that
    Awake {
        #[command(subcommand)]
        switch: AwakeSwitch,
    },
    /// Set the volume of the default sink, or of the device given with --device
    Volume(VolumeArgs),
    /// Mute or unmute the default sink, or the device given with --device
    Mute(MuteArgs),
    /// Play or pause the active media player, or the player given with --player
    Play(PlayArgs),
    /// List the audio devices, or choose the default one
    Audio {
        #[command(subcommand)]
        action: AudioAction,
    },
    /// List the media players, or skip a track
    Media {
        #[command(subcommand)]
        action: MediaAction,
    },
    /// List the apps of the server, which `launch` takes by their entry id
    Apps,
    /// Start an app of the server's list by its desktop entry id
    Launch(LaunchArgs),
    /// Open the launcher of the server
    Launcher {
        #[command(subcommand)]
        action: LauncherAction,
    },
    /// Type the text read from stdin into the focused window or into the launcher. One final
    /// newline is dropped
    Type(TypeArgs),
    /// Press one key, with modifiers, in the focused window or in the launcher
    Key(KeyArgs),
    /// Send one frame as it is and print every reply for 2 seconds. For negative tests
    Raw {
        /// The frame payload. Nothing checks it
        #[arg(value_name = "JSON")]
        payload: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AwakeSwitch {
    /// Start keep-awake, or give it a new end. Every device shares the one keep-awake
    On {
        /// How long from now: a whole number with s, m or h, at least 60 s, such as 90m.
        /// Default: the server's default. The server may end it sooner because of its cap
        #[arg(long = "for", value_name = "DURATION", value_parser = parse_duration_s)]
        duration_s: Option<u64>,
    },
    /// End keep-awake, whichever device started it
    Off,
}

/// Stands for a pairing URI that was given as an argument. It does not hold the text, so
/// nothing that prints a [`Command`] can show the pairing secret.
#[derive(Clone, PartialEq, Eq)]
pub struct RefusedUri;

impl fmt::Debug for RefusedUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<refused pairing URI>")
    }
}

fn refuse_uri(_secret_text: &str) -> Result<RefusedUri, Infallible> {
    Ok(RefusedUri)
}

fn parse_icon_hash(text: &str) -> Result<IconHash, &'static str> {
    IconHash::parse(text).map_err(|_invalid| "not an icon hash: 22 characters of base64url")
}

impl Cli {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
