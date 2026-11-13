//! Command line grammar.

use std::convert::Infallible;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use swayconnect_protocol::IconHash;
use swayconnect_protocol::limits::MAX_FOCUS_NUMBER;

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
mod tests {
    use clap::error::ErrorKind;

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([&["swayconnect"], arguments].concat())
    }

    #[test]
    fn every_read_subcommand_parses() {
        for (word, command) in [
            ("info", Command::Info),
            ("state", Command::State),
            ("workspaces", Command::Workspaces),
            ("windows", Command::Windows),
            ("watch", Command::Watch),
        ] {
            let cli = parse(&[word]).expect(word);
            assert_eq!(cli.command, command);
            assert!(!cli.json);
            assert_eq!(cli.timeout(), Duration::from_secs(10));
        }
    }

    #[test]
    fn global_flags_are_accepted_before_and_after_the_subcommand() {
        for arguments in [
            ["--json", "--timeout", "3", "windows"],
            ["windows", "--json", "--timeout", "3"],
        ] {
            let cli = parse(&arguments).expect("flags");
            assert!(cli.json);
            assert_eq!(cli.timeout(), Duration::from_secs(3));
        }
    }

    #[test]
    fn a_timeout_of_zero_or_of_no_number_is_a_usage_error() {
        for value in ["0", "-1", "soon"] {
            let error = parse(&["--timeout", value, "info"]).expect_err(value);
            assert_eq!(error.exit_code(), 2);
        }
    }

    #[test]
    fn pair_takes_an_optional_name() {
        let named = parse(&["pair", "--name", "laptop"]).expect("pair --name");
        let expected = Command::Pair {
            name: Some("laptop".to_owned()),
            uri_arguments: Vec::new(),
        };
        assert_eq!(named.command, expected);

        let unnamed = parse(&["pair"]).expect("pair");
        let expected = Command::Pair {
            name: None,
            uri_arguments: Vec::new(),
        };
        assert_eq!(unnamed.command, expected);
    }

    #[test]
    fn a_uri_argument_of_pair_is_captured_and_not_reported_by_clap() {
        let cli = parse(&["pair", "swayconnect://pair?v=1"]).expect("captured");
        let Command::Pair { uri_arguments, .. } = cli.command else {
            panic!("not pair");
        };
        assert_eq!(uri_arguments, [RefusedUri]);
    }

    #[test]
    fn debug_output_cannot_show_a_pairing_uri_or_its_secret() {
        let secret = "c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2U";
        let uri = format!("swayconnect://pair?v=1&host=desk&port=1&fp={secret}&s={secret}");

        let cli = parse(&["pair", "--name", "laptop", &uri, &uri]).expect("captured");

        let debug_text = format!("{cli:?} {cli:#?} {:?}", cli.command);
        assert!(debug_text.contains("laptop"), "{debug_text}");
        assert!(debug_text.contains("<refused pairing URI>"), "{debug_text}");
        for secret_text in [secret, "swayconnect://", "fp=", "s="] {
            assert!(!debug_text.contains(secret_text), "{debug_text}");
        }
    }

    #[test]
    fn workspace_takes_a_number_or_an_id() {
        for (arguments, number, id) in [
            (&["workspace", "0"][..], Some(0), None),
            (&["workspace", "999"], Some(999), None),
            (&["workspace", "--id", "4"], None, Some(4)),
        ] {
            let cli = parse(arguments).expect("workspace");
            assert_eq!(cli.command, Command::Workspace { number, id });
        }
    }

    #[test]
    fn workspace_without_one_valid_target_is_a_usage_error() {
        for arguments in [
            &["workspace"][..],
            &["workspace", "1000"],
            &["workspace", "-1"],
            &["workspace", "web"],
            &["workspace", "3", "--id", "4"],
            &["workspace", "--id", "web"],
            &["workspace", "--id"],
        ] {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn focus_and_fullscreen_take_a_window_id() {
        let focus = parse(&["focus", "9"]).expect("focus");
        assert_eq!(focus.command, Command::Focus { window_id: 9 });

        for (word, switch) in [("on", Switch::On), ("off", Switch::Off)] {
            let fullscreen = parse(&["fullscreen", "9", word]).expect(word);
            let expected = Command::Fullscreen {
                window_id: 9,
                switch,
            };
            assert_eq!(fullscreen.command, expected);
        }
    }

    #[test]
    fn a_window_action_with_a_wrong_argument_is_a_usage_error() {
        for arguments in [
            &["focus"][..],
            &["focus", "title"],
            &["focus", "-9"],
            &["focus", "9", "10"],
            &["fullscreen", "9"],
            &["fullscreen", "9", "toggle"],
            &["fullscreen", "on", "9"],
        ] {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn layout_takes_a_number_or_an_id() {
        for (arguments, number, id) in [
            (&["layout", "3"][..], Some(3), None),
            (&["layout", "--id", "14"], None, Some(14)),
        ] {
            let cli = parse(arguments).expect("layout");
            assert_eq!(cli.command, Command::Layout { number, id });
        }
        for arguments in [
            &["layout"][..],
            &["layout", "web"],
            &["layout", "-1"],
            &["layout", "3", "--id", "14"],
        ] {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn icon_takes_a_strict_hash_and_an_output_file() {
        let hash = "q83vEjRWeJCrze8SNFZ4kA";
        for arguments in [
            &["icon", hash, "-o", "app.png"][..],
            &["icon", "--output", "app.png", hash],
        ] {
            let cli = parse(arguments).expect("icon");
            let expected = Command::Icon {
                hash: IconHash::parse(hash).expect("hash"),
                output: PathBuf::from("app.png"),
            };
            assert_eq!(cli.command, expected);
        }
        for arguments in [
            &["icon", hash][..],
            &["icon", "-o", "app.png"],
            &["icon", "firefox", "-o", "app.png"],
            &["icon", "q83vEjRWeJCrze8SNFZ4kA==", "-o", "app.png"],
            &["icon", "q83vEjRWeJCrze8SNFZ4kB", "-o", "app.png"],
        ] {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn raw_takes_exactly_one_payload_and_does_not_check_it() {
        let cli = parse(&["raw", "{not json"]).expect("raw");
        let payload = "{not json".to_owned();
        assert_eq!(cli.command, Command::Raw { payload });

        for arguments in [&["raw"][..], &["raw", "{}", "{}"]] {
            let error = parse(arguments).expect_err("usage error");
            assert_eq!(error.exit_code(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn a_missing_or_unknown_subcommand_is_a_usage_error() {
        let missing = parse(&[]).expect_err("no subcommand");
        assert_eq!(missing.exit_code(), 2);
        let unknown = parse(&["reboot"]).expect_err("unknown subcommand");
        assert_eq!(unknown.kind(), ErrorKind::InvalidSubcommand);
        assert_eq!(unknown.exit_code(), 2);
        let extra = parse(&["info", "extra"]).expect_err("unexpected argument");
        assert_eq!(extra.exit_code(), 2);
    }
}
