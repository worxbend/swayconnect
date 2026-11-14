//! What the `fake-proc` binary and the testkit agree on: the files next to a fake
//! program and the text of its mode file. Compiled into both, so the format has one source.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// One line per call: a JSON array of the arguments after `argv[0]`.
pub const LOG_SUFFIX: &str = ".log";
/// Absent: exit 0. Present: the text of a [`Mode`].
pub const MODE_SUFFIX: &str = ".mode";
/// Present: its bytes go to stdout before the mode runs.
pub const STDOUT_SUFFIX: &str = ".stdout";

/// `<program><suffix>`, next to the program.
pub fn sibling(program: &Path, suffix: &str) -> PathBuf {
    let mut path = OsString::from(program.as_os_str());
    path.push(suffix);
    PathBuf::from(path)
}

/// What a fake program does after it logged its call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Exits at once with this code.
    Exit(u8),
    /// Sleeps, then exits 0.
    Sleep(Duration),
    /// Runs until it is killed, like `systemd-inhibit` holding its lock.
    Hang,
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exit(code) => write!(formatter, "exit {code}"),
            Self::Sleep(duration) => write!(formatter, "sleep_ms {}", duration.as_millis()),
            Self::Hang => formatter.write_str("hang"),
        }
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut words = text.split_whitespace();
        let mode = match (words.next(), words.next()) {
            (Some("exit"), Some(code)) => code.parse().map(Self::Exit).ok(),
            (Some("sleep_ms"), Some(millis)) => millis
                .parse()
                .map(|millis| Self::Sleep(Duration::from_millis(millis)))
                .ok(),
            (Some("hang"), None) => Some(Self::Hang),
            _ => None,
        };
        match (mode, words.next()) {
            (Some(mode), None) => Ok(mode),
            _ => Err(format!("unknown mode {text:?}")),
        }
    }
}

/// A JSON string literal that is also a TOML basic string: `"`, `\` and every control
/// character (U+0000 to U+001F and U+007F) escaped, everything else as it is.
pub fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if control.is_ascii_control() => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}
