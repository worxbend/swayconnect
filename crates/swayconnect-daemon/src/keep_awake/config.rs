//! Config section `[keep_awake]` (`docs/DESIGN.md` 11.9, checks of 11.6).

use std::path::PathBuf;

use serde::Deserialize;
use swayconnect_protocol::limits::MIN_KEEP_AWAKE_DURATION_S;

use crate::proc::ProgramPath;

pub(super) const DEFAULT_INHIBIT: &str = "/usr/bin/systemd-inhibit";
pub(super) const DEFAULT_SLEEP: &str = "/usr/bin/sleep";
const DEFAULT_DURATION_S: u64 = 3600;
const DEFAULT_MAX_DURATION_S: u64 = 7200;
const DEFAULT_MAX_CONTINUOUS_S: u64 = 14_400;
const DEFAULT_COOLDOWN_S: u64 = 900;
const MIN_COOLDOWN_S: u64 = 60;
/// No config value of this section may exceed a week. It keeps every `Instant` sum far from
/// overflow.
const MAX_CONFIG_S: u64 = 7 * 24 * 3600;

/// Config section `[keep_awake]` (`docs/DESIGN.md` 11.9). `config.rs` embeds it. A value
/// against the rules of 11.6 fails deserialising, so the daemon exits 2 at start.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawKeepAwakeConfig")]
pub struct KeepAwakeConfig {
    pub inhibit: ProgramPath,
    pub sleep: ProgramPath,
    pub default_duration_s: u64,
    pub max_duration_s: u64,
    pub max_continuous_s: u64,
    pub cooldown_s: u64,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawKeepAwakeConfig {
    inhibit: ProgramPath,
    sleep: ProgramPath,
    default_duration_s: u64,
    max_duration_s: u64,
    max_continuous_s: u64,
    cooldown_s: u64,
}

impl Default for RawKeepAwakeConfig {
    fn default() -> RawKeepAwakeConfig {
        RawKeepAwakeConfig {
            inhibit: default_program(DEFAULT_INHIBIT),
            sleep: default_program(DEFAULT_SLEEP),
            default_duration_s: DEFAULT_DURATION_S,
            max_duration_s: DEFAULT_MAX_DURATION_S,
            max_continuous_s: DEFAULT_MAX_CONTINUOUS_S,
            cooldown_s: DEFAULT_COOLDOWN_S,
        }
    }
}

impl Default for KeepAwakeConfig {
    fn default() -> KeepAwakeConfig {
        KeepAwakeConfig::try_from(RawKeepAwakeConfig::default()).expect("the defaults are valid")
    }
}

impl TryFrom<RawKeepAwakeConfig> for KeepAwakeConfig {
    type Error = String;

    fn try_from(raw: RawKeepAwakeConfig) -> Result<KeepAwakeConfig, String> {
        let seconds = [
            raw.default_duration_s,
            raw.max_duration_s,
            raw.max_continuous_s,
            raw.cooldown_s,
        ];
        if seconds.iter().any(|value| *value > MAX_CONFIG_S) {
            return Err(format!(
                "[keep_awake] values must be at most {MAX_CONFIG_S} s"
            ));
        }
        if raw.cooldown_s < MIN_COOLDOWN_S {
            return Err(format!("cooldown_s must be at least {MIN_COOLDOWN_S}"));
        }
        if raw.max_continuous_s < raw.max_duration_s {
            return Err("max_continuous_s must be at least max_duration_s".to_owned());
        }
        let durations = MIN_KEEP_AWAKE_DURATION_S..=raw.max_duration_s;
        if !durations.contains(&raw.default_duration_s) {
            return Err(format!(
                "default_duration_s must be {MIN_KEEP_AWAKE_DURATION_S} to max_duration_s"
            ));
        }
        Ok(KeepAwakeConfig {
            inhibit: raw.inhibit,
            sleep: raw.sleep,
            default_duration_s: raw.default_duration_s,
            max_duration_s: raw.max_duration_s,
            max_continuous_s: raw.max_continuous_s,
            cooldown_s: raw.cooldown_s,
        })
    }
}

fn default_program(path: &str) -> ProgramPath {
    ProgramPath::new(PathBuf::from(path)).expect("the default path is absolute")
}
