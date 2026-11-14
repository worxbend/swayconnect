//! Arguments of the M2b subcommands: audio, media, apps, the launcher and the remote
//! keyboard (`docs/PROTOCOL.md` 17.9 to 17.13).

use std::convert::Infallible;
use std::fmt;

use clap::{Args, Subcommand};
use swayconnect_protocol::limits::{MAX_FOCUS_NUMBER, MAX_KEY_REPEAT, MAX_MAX_VOLUME_PCT};
use swayconnect_protocol::{EntryId, InputTarget, Key, Modifier};

use super::args::Switch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct VolumeArgs {
    /// Volume in percent. The server's `max_volume_pct` (100 to 150) is the highest it takes
    #[arg(
        value_name = "PERCENT",
        value_parser = clap::value_parser!(u16).range(..=i64::from(MAX_MAX_VOLUME_PCT))
    )]
    pub volume_pct: u16,

    #[command(flatten)]
    pub device: DeviceArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct MuteArgs {
    pub switch: Switch,

    #[command(flatten)]
    pub device: DeviceArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct DeviceArg {
    /// Id of a sink or a source of the server's `audio` state. Default: the default sink.
    /// Unmuting a microphone or changing its volume needs scope `audio.capture`
    #[arg(long = "device", value_name = "ID")]
    pub device_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AudioAction {
    /// List the sinks and sources of the server's `audio` state
    Devices,
    /// Make a sink or a source the default one. A source needs scope `audio.capture`
    Default {
        /// Id of a sink or a source, as `audio devices` lists it
        #[arg(value_name = "ID")]
        device_id: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum MediaAction {
    /// List the players of the server's `media` state
    Players,
    /// Skip to the next track of the active player, or of the player given with --player
    Next(PlayArgs),
    /// Go back one track on the active player, or on the player given with --player
    Previous(PlayArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum LauncherAction {
    /// Run the server's launcher, such as fuzzel. Type into it with `type --launcher` and
    /// `key --launcher`; `key --launcher Escape` closes it
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct PlayArgs {
    /// Id of a player of the server's `media` state. Default: the active player
    #[arg(long = "player", value_name = "ID")]
    pub player_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct LaunchArgs {
    /// Desktop entry id, as the server's `apps` state lists it, such as firefox
    #[arg(value_name = "ENTRY", value_parser = parse_entry_id)]
    pub entry_id: EntryId,

    /// Focus this workspace number first
    #[arg(
        long = "workspace",
        value_name = "NUMBER",
        value_parser = clap::value_parser!(u16).range(..=i64::from(MAX_FOCUS_NUMBER))
    )]
    pub number: Option<u16>,

    /// Confirm the launch of a risky app: a terminal, or one the owner marked
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct TypeArgs {
    #[command(flatten)]
    pub target: InputTargetArg,

    // Caught here so that `run` can refuse text given as an argument without echoing it,
    // which a clap "unexpected argument" message would do.
    #[arg(hide = true, value_name = "TEXT", value_parser = refuse_text)]
    pub text_arguments: Vec<RefusedText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct KeyArgs {
    /// Escape, Tab, Return, BackSpace, Delete, Insert, Left, Right, Up, Down, Home, End,
    /// Page_Up, Page_Down, space, F1 to F12, or one letter a-z or digit 0-9
    #[arg(value_name = "KEY", value_parser = parse_key)]
    pub key: Key,

    /// Modifiers held during the press: ctrl, alt, shift, super. Repeat or separate with commas
    #[arg(
        long = "mod",
        value_name = "MODIFIER",
        value_delimiter = ',',
        value_parser = parse_modifier
    )]
    pub modifiers: Vec<Modifier>,

    /// How many times to press it
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 1,
        value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_KEY_REPEAT))
    )]
    pub repeat: u8,

    #[command(flatten)]
    pub target: InputTargetArg,
}

/// Where `type` and `key` deliver: exactly one of `--window` and `--launcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
#[group(required = true, multiple = false)]
pub struct InputTargetArg {
    /// Id of the focused window, as `windows` lists it. Input stops when another window
    /// has the focus
    #[arg(long = "window", value_name = "ID")]
    pub window_id: Option<u64>,

    /// The launcher this device opened with `launcher open`. Input stops when it closes
    #[arg(long)]
    pub launcher: bool,
}

impl InputTargetArg {
    /// `None` unless exactly one target is set. The parser lets only that through; this
    /// does not rely on it.
    pub fn input_target(self) -> Option<InputTarget> {
        match (self.window_id, self.launcher) {
            (Some(window_id), false) => Some(InputTarget::Window(window_id)),
            (None, true) => Some(InputTarget::Launcher),
            (Some(_), true) | (None, false) => None,
        }
    }
}

/// Stands for text that was given as an argument. It does not hold the text, so nothing
/// that prints a command can show what was meant to be typed.
#[derive(Clone, PartialEq, Eq)]
pub struct RefusedText;

impl fmt::Debug for RefusedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<refused text>")
    }
}

fn refuse_text(_typed_text: &str) -> Result<RefusedText, Infallible> {
    Ok(RefusedText)
}

fn parse_entry_id(text: &str) -> Result<EntryId, &'static str> {
    EntryId::parse(text).ok_or("an entry id is 1 to 255 characters of A-Z a-z 0-9 . _ -")
}

fn parse_key(text: &str) -> Result<Key, &'static str> {
    Key::from_wire(text).ok_or("not a key name: see `swayconnect key --help`")
}

fn parse_modifier(text: &str) -> Result<Modifier, &'static str> {
    Modifier::from_wire(text).ok_or("a modifier is ctrl, alt, shift or super")
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::super::args::{Cli, Command};
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([&["swayconnect"], arguments].concat())
    }

    fn assert_usage_errors(cases: &[&[&str]]) {
        for arguments in cases {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn volume_and_mute_take_an_optional_device() {
        let cases = [
            (&["volume", "0"][..], Command::Volume(volume(0, None))),
            (
                &["volume", "150", "--device", "3"],
                Command::Volume(volume(150, Some(3))),
            ),
            (&["mute", "on"], Command::Mute(mute(Switch::On, None))),
            (
                &["mute", "off", "--device", "9"],
                Command::Mute(mute(Switch::Off, Some(9))),
            ),
        ];
        for (arguments, expected) in cases {
            assert_eq!(parse(arguments).expect("parses").command, expected);
        }
    }

    fn volume(volume_pct: u16, device_id: Option<u64>) -> VolumeArgs {
        let device = DeviceArg { device_id };
        VolumeArgs { volume_pct, device }
    }

    fn mute(switch: Switch, device_id: Option<u64>) -> MuteArgs {
        let device = DeviceArg { device_id };
        MuteArgs { switch, device }
    }

    #[test]
    fn a_volume_or_mute_with_a_wrong_argument_is_a_usage_error() {
        assert_usage_errors(&[
            &["volume"],
            &["volume", "151"],
            &["volume", "-1"],
            &["volume", "50%"],
            &["volume", "50", "--device", "speaker"],
            &["mute"],
            &["mute", "toggle"],
        ]);
    }

    #[test]
    fn play_takes_an_optional_player() {
        let active = parse(&["play"]).expect("play").command;
        assert_eq!(active, Command::Play(PlayArgs { player_id: None }));
        let chosen = parse(&["play", "--player", "2"]).expect("play").command;
        assert_eq!(chosen, Command::Play(PlayArgs { player_id: Some(2) }));
        assert_usage_errors(&[&["play", "2"], &["play", "--player", "spotify"]]);
    }

    #[test]
    fn launch_takes_an_entry_id_an_optional_workspace_and_yes() {
        let cli = parse(&["launch", "org.gnome.Nautilus.desktop"]).expect("launch");
        let entry_id = EntryId::parse("org.gnome.Nautilus.desktop").expect("entry id");
        let plain = LaunchArgs {
            entry_id: entry_id.clone(),
            number: None,
            yes: false,
        };
        assert_eq!(cli.command, Command::Launch(plain));

        let arguments = [
            "launch",
            "org.gnome.Nautilus.desktop",
            "--workspace",
            "3",
            "--yes",
        ];
        let cli = parse(&arguments).expect("launch");
        let full = LaunchArgs {
            entry_id,
            number: Some(3),
            yes: true,
        };
        assert_eq!(cli.command, Command::Launch(full));
    }

    #[test]
    fn a_launch_with_a_bad_entry_id_or_workspace_is_a_usage_error() {
        let too_long = "a".repeat(256);
        assert_usage_errors(&[
            &["launch"],
            &["launch", "../firefox.desktop"],
            &["launch", "fire fox"],
            &["launch", ""],
            &["launch", &too_long],
            &["launch", "firefox.desktop", "--workspace", "1000"],
        ]);
    }

    #[test]
    fn type_needs_a_window_and_captures_text_arguments() {
        let cli = parse(&["type", "--window", "6"]).expect("type");
        let target = window_target(6);
        let text_arguments = Vec::new();
        assert_eq!(
            cli.command,
            Command::Type(TypeArgs {
                target,
                text_arguments
            })
        );

        let cli = parse(&["type", "--window", "6", "hunter2", "secret"]).expect("captured");
        let Command::Type(typed) = &cli.command else {
            panic!("not type");
        };
        assert_eq!(typed.text_arguments, [RefusedText, RefusedText]);
        let debug_text = format!("{cli:?} {cli:#?}");
        assert!(!debug_text.contains("hunter2"), "{debug_text}");
        assert!(!debug_text.contains("secret"), "{debug_text}");

        assert_usage_errors(&[&["type"], &["type", "--window", "focused"]]);
    }

    #[test]
    fn key_takes_a_closed_name_modifiers_a_repeat_and_a_window() {
        let cli = parse(&["key", "Return", "--window", "6"]).expect("key");
        let expected = KeyArgs {
            key: Key::from_wire("Return").expect("key"),
            modifiers: Vec::new(),
            repeat: 1,
            target: window_target(6),
        };
        assert_eq!(cli.command, Command::Key(expected));

        let arguments = [
            "key",
            "t",
            "--mod",
            "ctrl,shift",
            "--mod",
            "alt",
            "--repeat",
            "32",
        ];
        let cli = parse(&[&arguments[..], &["--window", "6"]].concat()).expect("key");
        let Command::Key(press) = cli.command else {
            panic!("not key");
        };
        let modifiers = [Modifier::Ctrl, Modifier::Shift, Modifier::Alt];
        assert_eq!(press.modifiers, modifiers);
        assert_eq!(press.repeat, 32);
    }

    #[test]
    fn a_key_with_a_wrong_argument_is_a_usage_error() {
        assert_usage_errors(&[
            &["key", "Return"],
            &["key", "Enter", "--window", "6"],
            &["key", "A", "--window", "6"],
            &["key", "XF86Switch_VT_1", "--window", "6"],
            &["key", "a", "--mod", "meta", "--window", "6"],
            &["key", "a", "--repeat", "0", "--window", "6"],
            &["key", "a", "--repeat", "33", "--window", "6"],
        ]);
    }

    fn window_target(window_id: u64) -> InputTargetArg {
        let window_id = Some(window_id);
        InputTargetArg {
            window_id,
            launcher: false,
        }
    }

    #[test]
    fn type_and_key_take_the_launcher_as_their_target() {
        let launcher = InputTargetArg {
            window_id: None,
            launcher: true,
        };
        let Command::Type(typed) = parse(&["type", "--launcher"]).expect("type").command else {
            panic!("not type");
        };
        assert_eq!(typed.target, launcher);
        let Command::Key(key) = parse(&["key", "Escape", "--launcher"])
            .expect("key")
            .command
        else {
            panic!("not key");
        };
        assert_eq!(key.target, launcher);

        assert_eq!(launcher.input_target(), Some(InputTarget::Launcher));
        assert_eq!(
            window_target(6).input_target(),
            Some(InputTarget::Window(6))
        );
        let both = InputTargetArg {
            window_id: Some(6),
            launcher: true,
        };
        assert_eq!(both.input_target(), None);
    }

    #[test]
    fn an_input_without_exactly_one_target_is_a_usage_error() {
        assert_usage_errors(&[
            &["type", "--window", "6", "--launcher"],
            &["key", "Escape"],
            &["key", "Escape", "--launcher", "--window", "6"],
            &["key", "Escape", "--launcher=yes"],
        ]);
    }

    #[test]
    fn audio_takes_devices_or_default_with_an_id() {
        let devices = parse(&["audio", "devices"]).expect("devices").command;
        let action = AudioAction::Devices;
        assert_eq!(devices, Command::Audio { action });
        let default = parse(&["audio", "default", "7"]).expect("default").command;
        let action = AudioAction::Default { device_id: 7 };
        assert_eq!(default, Command::Audio { action });
        assert_usage_errors(&[
            &["audio"],
            &["audio", "default"],
            &["audio", "default", "speaker"],
            &["audio", "default", "-1"],
            &["audio", "devices", "7"],
        ]);
    }

    #[test]
    fn media_takes_players_or_a_skip_with_an_optional_player() {
        let cases = [
            (&["media", "players"][..], MediaAction::Players),
            (
                &["media", "next"],
                MediaAction::Next(PlayArgs { player_id: None }),
            ),
            (
                &["media", "previous", "--player", "2"],
                MediaAction::Previous(PlayArgs { player_id: Some(2) }),
            ),
        ];
        for (arguments, action) in cases {
            let command = parse(arguments).expect("media").command;
            assert_eq!(command, Command::Media { action }, "{arguments:?}");
        }
        assert_usage_errors(&[
            &["media"],
            &["media", "skip"],
            &["media", "next", "2"],
            &["media", "players", "--player", "2"],
        ]);
    }

    #[test]
    fn apps_and_launcher_open_take_no_argument() {
        assert_eq!(parse(&["apps"]).expect("apps").command, Command::Apps);
        let open = parse(&["launcher", "open"]).expect("open").command;
        let action = LauncherAction::Open;
        assert_eq!(open, Command::Launcher { action });
        assert_usage_errors(&[&["apps", "firefox"], &["launcher"], &["launcher", "close"]]);
    }
}
