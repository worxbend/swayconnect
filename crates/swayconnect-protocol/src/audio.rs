//! Topic `audio` and the change an audio operation asks for (`docs/PROTOCOL.md` 17.9).

use serde_json::Value;

use crate::code::wire_names;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::{MAX_DEVICE_VOLUME_PCT, MAX_MAX_VOLUME_PCT, MIN_MAX_VOLUME_PCT};
use crate::message::JsonObject;
use crate::ops::Operation;

/// The state object of topic `audio`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audio {
    /// `false` when the sound server cannot be reached; both lists are then empty.
    pub available: bool,
    /// The highest `volume_pct` a client may send, 100 to 150.
    pub max_volume_pct: u16,
    pub default_sink: Option<u64>,
    pub default_source: Option<u64>,
    pub sinks: Vec<Device>,
    /// Monitor sources are not listed.
    pub sources: Vec<Device>,
    pub truncated: bool,
}

/// One output or input device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Issued by the server.
    pub id: u64,
    /// Untrusted text, at most 128 scalar values.
    pub description: String,
    pub kind: DeviceKind,
    /// Highest channel, 0 to 1000.
    pub volume_pct: u16,
    pub muted: bool,
    /// `false` when its port is unplugged. It is still a valid target.
    pub available: bool,
}

/// `Device.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Speaker,
    Headphones,
    Hdmi,
    Bluetooth,
    Usb,
    Microphone,
    /// Also every kind this version does not know.
    Other,
}

wire_names!(DeviceKind {
    Speaker => "speaker",
    Headphones => "headphones",
    Hdmi => "hdmi",
    Bluetooth => "bluetooth",
    Usb => "usb",
    Microphone => "microphone",
    Other => "other",
});

impl Audio {
    /// Checks presence, types and stated ranges, and maps an unknown `kind` to
    /// [`DeviceKind::Other`]. Unknown members are ignored.
    pub fn decode(object: &JsonObject) -> Result<Audio, InvalidMember> {
        Ok(Audio {
            available: json::boolean(object, "available")?,
            max_volume_pct: json::int_in(
                object,
                "max_volume_pct",
                MIN_MAX_VOLUME_PCT..=MAX_MAX_VOLUME_PCT,
            )?,
            default_sink: json::nullable_int(object, "default_sink")?,
            default_source: json::nullable_int(object, "default_source")?,
            sinks: json::object_array(object, "sinks", Device::decode)?,
            sources: json::object_array(object, "sources", Device::decode)?,
            truncated: json::boolean(object, "truncated")?,
        })
    }

    pub fn encode(&self) -> JsonObject {
        object_of([
            ("available", self.available.into()),
            ("max_volume_pct", self.max_volume_pct.into()),
            ("default_sink", self.default_sink.into()),
            ("default_source", self.default_source.into()),
            ("sinks", Device::encode_all(&self.sinks)),
            ("sources", Device::encode_all(&self.sources)),
            ("truncated", self.truncated.into()),
        ])
    }
}

impl Device {
    fn decode(object: &JsonObject) -> Member<Device> {
        let kind = json::string(object, "kind")?;
        Ok(Device {
            id: json::int(object, "id")?,
            description: json::string(object, "description")?.to_owned(),
            kind: DeviceKind::from_wire(kind).unwrap_or(DeviceKind::Other),
            volume_pct: json::int_in(object, "volume_pct", 0..=MAX_DEVICE_VOLUME_PCT)?,
            muted: json::boolean(object, "muted")?,
            available: json::boolean(object, "available")?,
        })
    }

    fn encode_all(devices: &[Device]) -> Value {
        devices
            .iter()
            .map(|device| Value::Object(device.encode()))
            .collect()
    }

    fn encode(&self) -> JsonObject {
        object_of([
            ("id", self.id.into()),
            ("description", self.description.as_str().into()),
            ("kind", self.kind.as_str().into()),
            ("volume_pct", self.volume_pct.into()),
            ("muted", self.muted.into()),
            ("available", self.available.into()),
        ])
    }
}

/// What an audio operation changes on its `device_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChange {
    /// `audio.set_volume`: 0 to the server's `max_volume_pct`.
    Volume(u16),
    /// `audio.set_mute`.
    Mute(bool),
    /// `audio.set_default`: a sink or a source.
    Default,
}

const VOLUME_PCT: &str = "volume_pct";
const MUTED: &str = "muted";

impl AudioChange {
    pub const fn operation(self) -> Operation {
        match self {
            AudioChange::Volume(_) => Operation::AudioSetVolume,
            AudioChange::Mute(_) => Operation::AudioSetMute,
            AudioChange::Default => Operation::AudioSetDefault,
        }
    }

    /// The capture rule of 17.9: on a source, every change except muting it also needs scope
    /// `audio.capture` (step 8a of 17.16, after the device was found).
    pub const fn needs_capture_on_source(self) -> bool {
        !matches!(self, AudioChange::Mute(true))
    }

    /// The upper end of `volume_pct` is the server's `max_volume_pct`. A server applies this at
    /// step 6 of spec 10.2, before it looks up the device; an error is a `bad_request`.
    pub fn check_max_volume(self, max_volume_pct: u16) -> Result<AudioChange, InvalidMember> {
        match self {
            AudioChange::Volume(volume_pct) if volume_pct > max_volume_pct => {
                Err(InvalidMember(VOLUME_PCT))
            }
            _ => Ok(self),
        }
    }

    /// `volume_pct` is never above 150, the highest `max_volume_pct` there can be.
    pub(crate) fn decode_volume(args: &JsonObject) -> Member<AudioChange> {
        json::int_in(args, VOLUME_PCT, 0..=MAX_MAX_VOLUME_PCT).map(AudioChange::Volume)
    }

    pub(crate) fn decode_mute(args: &JsonObject) -> Member<AudioChange> {
        json::boolean(args, MUTED).map(AudioChange::Mute)
    }

    /// The member beside `device_id`, if any.
    pub(crate) fn member(self) -> Option<(&'static str, Value)> {
        match self {
            AudioChange::Volume(volume_pct) => Some((VOLUME_PCT, volume_pct.into())),
            AudioChange::Mute(muted) => Some((MUTED, muted.into())),
            AudioChange::Default => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_muting_a_source_needs_no_capture() {
        assert!(!AudioChange::Mute(true).needs_capture_on_source());
        for change in [
            AudioChange::Mute(false),
            AudioChange::Volume(0),
            AudioChange::Default,
        ] {
            assert!(change.needs_capture_on_source(), "{change:?}");
        }
    }

    #[test]
    fn maximum_volume_is_the_servers() {
        assert_eq!(
            AudioChange::Volume(100).check_max_volume(100),
            Ok(AudioChange::Volume(100))
        );
        assert_eq!(
            AudioChange::Volume(101).check_max_volume(100),
            Err(InvalidMember(VOLUME_PCT))
        );
        assert_eq!(
            AudioChange::Default.check_max_volume(100),
            Ok(AudioChange::Default)
        );
    }
}
