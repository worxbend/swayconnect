//! A read-only snapshot of the live sound server, for the real-backend tests: each compares
//! it before and after, so a test that reached the live server fails (`docs/M2C_PLAN.md` 4).
//!
//! The one place a test talks to the live session on purpose: `pactl -f json list` and
//! `pactl -f json info`, which change nothing.

use std::process::Command;

use serde_json::Value;

/// Name, mute and volume of each live sink and source, and the two defaults.
#[derive(Debug, PartialEq)]
pub struct LiveAudio {
    sinks: Vec<(Value, Value, Value)>,
    sources: Vec<(Value, Value, Value)>,
    default_sink: Value,
    default_source: Value,
}

impl LiveAudio {
    /// `None` when no live sound server answers.
    pub fn read() -> Option<LiveAudio> {
        let info = live_pactl(&["info"])?;
        Some(LiveAudio {
            sinks: rows(live_pactl(&["list", "sinks"])?),
            sources: rows(live_pactl(&["list", "sources"])?),
            default_sink: info["default_sink_name"].clone(),
            default_source: info["default_source_name"].clone(),
        })
    }

    /// Panics when the live sound server is not as `before`.
    pub fn assert_unchanged(before: &Option<LiveAudio>) {
        assert_eq!(
            &LiveAudio::read(),
            before,
            "the live sound server changed during a sealed test"
        );
    }
}

/// `pactl -f json <args>` on the live server, with the test process's own environment.
fn live_pactl(args: &[&str]) -> Option<Value> {
    let output = Command::new("/usr/bin/pactl")
        .args(["-f", "json"])
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(serde_json::from_slice(&output.stdout).expect("pactl JSON"))
}

fn rows(list: Value) -> Vec<(Value, Value, Value)> {
    let Value::Array(nodes) = list else {
        panic!("pactl list is not a JSON array: {list}");
    };
    nodes
        .into_iter()
        .map(|node| {
            let (name, mute, volume) = (&node["name"], &node["mute"], &node["volume"]);
            (name.clone(), mute.clone(), volume.clone())
        })
        .collect()
}
