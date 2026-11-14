//! Config sections `[input]` and `[wayland]` (`docs/DESIGN.md` 11.9), and where the Wayland
//! socket is. `config.rs` embeds both sections with `#[serde(default)]`.

use std::ffi::OsString;
use std::path::PathBuf;

use serde::Deserialize;

const DEFAULT_SEAT: &str = "seat0";
/// Far above any real seat name; keeps a config typo from becoming a huge string.
const MAX_SEAT_NAME_BYTES: usize = 64;

/// A `wl_seat` name: 1 to 64 bytes without control characters.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct SeatName(String);

impl SeatName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SeatName {
    type Error = String;

    fn try_from(name: String) -> Result<SeatName, String> {
        let fits = (1..=MAX_SEAT_NAME_BYTES).contains(&name.len());
        if fits && !name.chars().any(char::is_control) {
            Ok(SeatName(name))
        } else {
            Err(format!(
                "seat {name:?} must be 1 to {MAX_SEAT_NAME_BYTES} bytes without control characters"
            ))
        }
    }
}

/// `[input]`: the seat the virtual keyboard joins.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub seat: SeatName,
}

impl Default for InputConfig {
    fn default() -> InputConfig {
        InputConfig {
            seat: SeatName(DEFAULT_SEAT.to_owned()),
        }
    }
}

/// `[wayland]`: `socket = ""` (the default) means discover it from the session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaylandConfig {
    #[serde(deserialize_with = "optional_absolute_path")]
    pub socket: Option<PathBuf>,
}

fn optional_absolute_path<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PathBuf>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err(serde::de::Error::custom(format!(
            "wayland socket {} must be an absolute path or empty",
            path.display()
        )))
    }
}

/// How the daemon finds the compositor's Wayland socket. Mirrors `SwayTarget`: only
/// `main.rs` reads the environment and builds `Discover`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandTarget {
    /// Config `[wayland] socket`.
    Explicit(PathBuf),
    /// `WAYLAND_DISPLAY`, joined to `XDG_RUNTIME_DIR` unless it is absolute (as libwayland
    /// does). Without `WAYLAND_DISPLAY` there is no socket, so input is unavailable.
    Discover {
        runtime_dir: Option<PathBuf>,
        display: Option<OsString>,
    },
}

impl WaylandTarget {
    /// The config path when set, else discovery from these values.
    pub fn new(
        config: &WaylandConfig,
        runtime_dir: Option<PathBuf>,
        display: Option<OsString>,
    ) -> WaylandTarget {
        match &config.socket {
            Some(path) => WaylandTarget::Explicit(path.clone()),
            None => WaylandTarget::Discover {
                runtime_dir,
                display,
            },
        }
    }

    /// `None`: no socket can be named now.
    pub fn resolve(&self) -> Option<PathBuf> {
        match self {
            WaylandTarget::Explicit(path) => Some(path.clone()),
            WaylandTarget::Discover {
                runtime_dir,
                display,
            } => {
                let display = PathBuf::from(display.as_ref().filter(|name| !name.is_empty())?);
                if display.is_absolute() {
                    Some(display)
                } else {
                    Some(runtime_dir.as_ref()?.join(display))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sections {
        #[serde(default)]
        input: InputConfig,
        #[serde(default)]
        wayland: WaylandConfig,
    }

    fn parse(text: &str) -> Result<Sections, toml::de::Error> {
        toml::from_str(text)
    }

    #[test]
    fn defaults_match_the_design() {
        let sections = parse("").expect("empty config");
        assert_eq!(sections.input.seat.as_str(), "seat0");
        assert_eq!(sections.wayland.socket, None);
        let sections = parse("[wayland]\nsocket = \"\"").expect("empty socket");
        assert_eq!(sections.wayland.socket, None);
    }

    #[test]
    fn bad_values_fail_at_load() {
        for text in [
            "[input]\nseat = \"\"",
            "[input]\nseat = \"seat\\n0\"",
            "[input]\nother = 1",
            "[wayland]\nsocket = \"wayland-1\"",
            "[wayland]\nextra = true",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
        let long = format!(
            "[input]\nseat = \"{}\"",
            "s".repeat(MAX_SEAT_NAME_BYTES + 1)
        );
        assert!(parse(&long).is_err());
    }

    #[test]
    fn the_socket_comes_from_config_or_the_session() {
        let explicit = parse("[wayland]\nsocket = \"/run/user/1000/wayland-9\"").expect("config");
        let target = WaylandTarget::new(&explicit.wayland, None, None);
        assert_eq!(
            target.resolve(),
            Some(PathBuf::from("/run/user/1000/wayland-9"))
        );

        let discover = |runtime: Option<&str>, display: Option<&str>| {
            WaylandTarget::new(
                &WaylandConfig::default(),
                runtime.map(PathBuf::from),
                display.map(OsString::from),
            )
            .resolve()
        };
        assert_eq!(
            discover(Some("/run/user/1000"), Some("wayland-1")),
            Some(PathBuf::from("/run/user/1000/wayland-1"))
        );
        assert_eq!(
            discover(None, Some("/tmp/wl.sock")),
            Some(PathBuf::from("/tmp/wl.sock"))
        );
        assert_eq!(discover(Some("/run/user/1000"), None), None);
        assert_eq!(discover(Some("/run/user/1000"), Some("")), None);
        assert_eq!(discover(None, Some("wayland-1")), None);
    }
}
