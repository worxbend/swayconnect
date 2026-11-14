//! pactl (`docs/DESIGN.md` D25, 11.6): the lenient parse of `pactl -f json list sinks` and
//! `sources`, the `pactl subscribe` event filter, and the argv of every pactl call.
//!
//! Every name that goes into an argv comes from pactl's own output and goes after `--`;
//! a name that starts with `-` never gets that far (11.3 rule 8). pactl stops reading options
//! at its command word and takes every later word as it is, a `--` too, so the `--` goes
//! before the command word.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use swayconnect_protocol::limits::{MAX_DEVICE_VOLUME_PCT, MAX_LABEL_SCALARS};
use swayconnect_protocol::text::{is_control_or_format, replace_unsafe};
use swayconnect_protocol::{AudioChange, DeviceKind};

/// pactl's volume that is 100 % (`PA_VOLUME_NORM`).
const VOLUME_NORM: u64 = 65_536;
const NOT_AVAILABLE: &str = "not available";
const MONITOR_CLASS: &str = "monitor";

/// Which list a device is in. pactl keeps sink and source names apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Sink,
    Source,
}

impl Direction {
    fn word(self) -> &'static str {
        match self {
            Direction::Sink => "sink",
            Direction::Source => "source",
        }
    }
}

/// One device of a pactl list, after the filters of 11.6, with its text cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The pactl name. Never starts with `-`, holds no control or format character.
    pub name: String,
    /// At most 128 scalar values, control and format characters replaced (17.1 rule 7).
    pub description: String,
    pub kind: DeviceKind,
    pub volume_pct: u16,
    pub muted: bool,
    pub available: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("pactl list output is not a JSON array: {0}")]
pub struct ParseError(#[from] serde_json::Error);

/// The members of a pactl list entry that are used. Every other member is ignored, and an
/// entry whose used members have the wrong type is skipped.
#[derive(Deserialize)]
struct RawNode {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    volume: HashMap<String, RawChannel>,
    #[serde(default)]
    properties: HashMap<String, Value>,
    #[serde(default)]
    ports: Vec<RawPort>,
    #[serde(default)]
    active_port: Option<String>,
    /// On a source pactl writes the sink it monitors under this key ("" for none).
    #[serde(default)]
    monitor_source: Option<String>,
    /// The PulseAudio spelling of the same.
    #[serde(default)]
    monitor_of_sink: Option<String>,
}

#[derive(Deserialize)]
struct RawChannel {
    #[serde(default)]
    value: u64,
}

#[derive(Deserialize)]
struct RawPort {
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    availability: Option<String>,
}

/// Parses the stdout of `pactl -f json list sinks` or `sources`. Only output that is not a
/// JSON array is an error; a bad entry is skipped.
pub fn parse_list(direction: Direction, stdout: &[u8]) -> Result<Vec<Node>, ParseError> {
    let entries: Vec<Value> = serde_json::from_slice(stdout)?;
    Ok(entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<RawNode>(entry).ok())
        .filter(|raw| is_usable_name(&raw.name))
        .filter(|raw| direction == Direction::Sink || !raw.is_monitor())
        .map(|raw| raw.into_node(direction))
        .collect())
}

/// The stdout of `pactl get-default-sink` or `get-default-source`: one name, or nothing.
pub fn parse_default(stdout: &[u8]) -> Option<String> {
    let name = String::from_utf8_lossy(stdout)
        .trim_end_matches('\n')
        .to_owned();
    is_usable_name(&name).then_some(name)
}

/// A name that may go into an argv after `--`: not empty, no leading `-`, no control or
/// format character (which also keeps NUL out).
fn is_usable_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && !name.chars().any(is_control_or_format)
}

impl RawNode {
    fn is_monitor(&self) -> bool {
        let names_a_sink = |sink: &Option<String>| {
            sink.as_deref()
                .is_some_and(|sink| !sink.is_empty() && sink != "n/a")
        };
        self.property("device.class") == Some(MONITOR_CLASS)
            || names_a_sink(&self.monitor_source)
            || names_a_sink(&self.monitor_of_sink)
    }

    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).and_then(Value::as_str)
    }

    fn active_port(&self) -> Option<&RawPort> {
        let active = self.active_port.as_deref()?;
        self.ports.iter().find(|port| port.name == active)
    }

    fn into_node(self, direction: Direction) -> Node {
        let loudest = self.volume.values().map(|channel| channel.value).max();
        let volume_pct = loudest.map_or(0, |value| {
            let pct = (value.saturating_mul(100) + VOLUME_NORM / 2) / VOLUME_NORM;
            u16::try_from(pct).map_or(MAX_DEVICE_VOLUME_PCT, |pct| pct.min(MAX_DEVICE_VOLUME_PCT))
        });
        let available = self
            .active_port()
            .and_then(|port| port.availability.as_deref())
            != Some(NOT_AVAILABLE);
        let description = if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        };
        Node {
            description: replace_unsafe(description)
                .chars()
                .take(MAX_LABEL_SCALARS)
                .collect(),
            kind: self.kind(direction),
            volume_pct,
            muted: self.mute,
            available,
            name: self.name,
        }
    }

    /// `Device.kind` from the bus, the active port's type and the profile; first match wins.
    fn kind(&self, direction: Direction) -> DeviceKind {
        let bus = self.property("device.bus").unwrap_or_default();
        let port = self
            .active_port()
            .and_then(|port| port.kind.as_deref())
            .unwrap_or_default();
        let profile = self.property("device.profile.name").unwrap_or_default();
        let is_alsa = self.property("device.api") == Some("alsa");
        if bus == "bluetooth" || matches!(port, "Bluetooth" | "Handsfree") {
            DeviceKind::Bluetooth
        } else if matches!(port, "HDMI" | "TV") || profile.contains("hdmi") {
            DeviceKind::Hdmi
        } else if direction == Direction::Sink
            && matches!(port, "Headphones" | "Headset" | "Earpiece")
        {
            DeviceKind::Headphones
        } else if bus == "usb" || port == "USB" {
            DeviceKind::Usb
        } else if direction == Direction::Sink
            && (is_alsa || matches!(port, "Speaker" | "Line" | "Analog"))
        {
            DeviceKind::Speaker
        } else if direction == Direction::Source
            && (is_alsa || matches!(port, "Mic" | "Line" | "Analog" | "Headset" | "Handset"))
        {
            DeviceKind::Microphone
        } else {
            DeviceKind::Other
        }
    }
}

/// `pactl subscribe` prints `Event '<type>' on <facility> #<index>`; only the wording around
/// the facility is translated. Devices change with events on sinks, sources, cards and the
/// server (the defaults). Client events are ignored: every snapshot's own pactl runs make
/// them, so they would start the next snapshot forever. Stream events change no device.
pub fn is_device_event(line: &str) -> bool {
    let mut words = line.split_whitespace().rev();
    let index = words.next().is_some_and(|word| word.starts_with('#'));
    index && matches!(words.next(), Some("sink" | "source" | "card" | "server"))
}

pub fn list_args(direction: Direction) -> [&'static str; 4] {
    let list = match direction {
        Direction::Sink => "sinks",
        Direction::Source => "sources",
    };
    ["-f", "json", "list", list]
}

pub fn default_args(direction: Direction) -> [&'static str; 1] {
    match direction {
        Direction::Sink => ["get-default-sink"],
        Direction::Source => ["get-default-source"],
    }
}

pub const SUBSCRIBE_ARGS: [&str; 1] = ["subscribe"];

/// The arguments of one write: `--`, the command word, the name, the value
/// (`docs/DESIGN.md` 11.6). The `--` ends pactl's options before any word of the daemon; after
/// the command word pactl would take it as the device name. A volume is clamped to
/// `max_volume_pct`.
pub fn write_args(
    direction: Direction,
    name: &str,
    change: AudioChange,
    max_volume_pct: u16,
) -> Vec<String> {
    let word = direction.word();
    let (verb, value) = match change {
        AudioChange::Volume(pct) => (
            format!("set-{word}-volume"),
            Some(format!("{}%", pct.min(max_volume_pct))),
        ),
        AudioChange::Mute(muted) => (
            format!("set-{word}-mute"),
            Some(if muted { "1" } else { "0" }.to_owned()),
        ),
        AudioChange::Default => (format!("set-default-{word}"), None),
    };
    ["--".to_owned(), verb, name.to_owned()]
        .into_iter()
        .chain(value)
        .collect()
}

#[cfg(test)]
mod tests;
