//! Config section `[audio]` (`docs/DESIGN.md` 11.9).

use std::path::PathBuf;

use serde::Deserialize;
use swayconnect_protocol::limits::{MAX_MAX_VOLUME_PCT, MIN_MAX_VOLUME_PCT};

use crate::proc::ProgramPath;

const DEFAULT_PACTL: &str = "/usr/bin/pactl";
const DEFAULT_MAX_VOLUME_PCT: u16 = 100;

/// Config section `[audio]`. `config.rs` embeds it. A relative `pactl` path or a
/// `max_volume_pct` outside 100 to 150 (`docs/PROTOCOL.md` 17.9) fails deserialising, so the
/// daemon exits 2 at start.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub pactl: ProgramPath,
    pub max_volume_pct: MaxVolumePct,
}

impl Default for AudioConfig {
    fn default() -> AudioConfig {
        AudioConfig {
            pactl: ProgramPath::new(PathBuf::from(DEFAULT_PACTL))
                .expect("the default path is absolute"),
            max_volume_pct: MaxVolumePct(DEFAULT_MAX_VOLUME_PCT),
        }
    }
}

/// The highest `volume_pct` a client may send: 100 to 150.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u16")]
pub struct MaxVolumePct(u16);

impl MaxVolumePct {
    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for MaxVolumePct {
    type Error = String;

    fn try_from(value: u16) -> Result<MaxVolumePct, String> {
        if (MIN_MAX_VOLUME_PCT..=MAX_MAX_VOLUME_PCT).contains(&value) {
            Ok(MaxVolumePct(value))
        } else {
            Err(format!(
                "max_volume_pct must be {MIN_MAX_VOLUME_PCT} to {MAX_MAX_VOLUME_PCT}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<AudioConfig, toml::de::Error> {
        toml::from_str(text)
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let config = parse("").expect("empty section");
        assert_eq!(config, AudioConfig::default());
        assert_eq!(config.pactl.as_path(), PathBuf::from("/usr/bin/pactl"));
        assert_eq!(config.max_volume_pct.get(), 100);
    }

    #[test]
    fn max_volume_must_be_100_to_150() {
        assert_eq!(
            parse("max_volume_pct = 150")
                .expect("150")
                .max_volume_pct
                .get(),
            150
        );
        for bad in [
            "max_volume_pct = 99",
            "max_volume_pct = 151",
            "max_volume_pct = -1",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_relative_program_or_an_unknown_key_is_refused() {
        assert!(parse("pactl = \"pactl\"").is_err());
        assert!(parse("volume = 3").is_err());
    }
}
