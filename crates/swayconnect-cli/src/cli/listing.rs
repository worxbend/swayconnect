//! The list commands of topics `audio`, `media` and `apps`: `audio devices`, `media players`
//! and `apps`. Pure: nothing here writes.
//!
//! Every string of these states is untrusted text. A table cell has each control or format
//! character replaced (spec 9.3 rule 4); a JSON line keeps the value and escapes it, as
//! [`super::render::server_json`] does when the caller prints it.

use serde_json::Value;
use swayconnect_protocol::ops::{TOPIC_APPS, TOPIC_AUDIO, TOPIC_MEDIA};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{
    Apps, Audio, Device, InvalidMember, JsonObject, Media, Player, PlayerStatus,
};

use super::render::{or_absent, set_flags, table};

const CUT_NOTE: &str = "note: the server cut the lists short";
const TRACK_SEPARATOR: &str = " - ";

/// A topic state that a list command prints as a table or as one JSON value per row.
pub trait Listing: Sized {
    const TOPIC: &'static str;

    fn decode(data: &JsonObject) -> Result<Self, InvalidMember>;

    fn table(&self) -> String;

    /// One wire object per row, with the members a table shows in its FLAGS column added.
    fn json_rows(&self) -> Vec<Value>;

    /// What the table says after its rows when the rows are not the whole truth.
    fn notes(&self) -> Vec<&'static str>;
}

impl Listing for Audio {
    const TOPIC: &'static str = TOPIC_AUDIO;

    fn decode(data: &JsonObject) -> Result<Audio, InvalidMember> {
        Audio::decode(data)
    }

    /// Sinks first, then sources.
    fn table(&self) -> String {
        let sinks = self.sinks.iter().map(|device| (Role::Sink, device));
        let sources = self.sources.iter().map(|device| (Role::Source, device));
        let rows = sinks
            .chain(sources)
            .map(|(role, device)| {
                let is_default = default_id(self, role) == Some(device.id);
                device_row(role, device, is_default)
            })
            .collect();
        table(
            &["ID", "ROLE", "KIND", "VOLUME", "FLAGS", "DESCRIPTION"],
            rows,
        )
    }

    /// Each device with `role` (`sink` or `source`) and `default` added.
    fn json_rows(&self) -> Vec<Value> {
        let encoded = self.encode();
        let sinks = wire_rows(&encoded, "sinks")
            .into_iter()
            .map(|row| (Role::Sink, row));
        let sources = wire_rows(&encoded, "sources")
            .into_iter()
            .map(|row| (Role::Source, row));
        sinks
            .chain(sources)
            .map(|(role, mut row)| {
                let default_id = default_id(self, role);
                let is_default = default_id.is_some() && row["id"].as_u64() == default_id;
                row["role"] = Value::from(role.as_str());
                row["default"] = Value::Bool(is_default);
                row
            })
            .collect()
    }

    fn notes(&self) -> Vec<&'static str> {
        notes(&[
            (!self.available, "note: the sound server cannot be reached"),
            (self.truncated, CUT_NOTE),
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Sink,
    Source,
}

impl Role {
    const fn as_str(self) -> &'static str {
        match self {
            Role::Sink => "sink",
            Role::Source => "source",
        }
    }
}

fn default_id(audio: &Audio, role: Role) -> Option<u64> {
    match role {
        Role::Sink => audio.default_sink,
        Role::Source => audio.default_source,
    }
}

fn device_row(role: Role, device: &Device, is_default: bool) -> Vec<String> {
    let flags = [
        (is_default, "default"),
        (device.muted, "muted"),
        (!device.available, "unplugged"),
    ];
    vec![
        device.id.to_string(),
        role.as_str().to_owned(),
        device.kind.as_str().to_owned(),
        format!("{}%", device.volume_pct),
        set_flags(&flags),
        replace_unsafe(&device.description),
    ]
}

impl Listing for Media {
    const TOPIC: &'static str = TOPIC_MEDIA;

    fn decode(data: &JsonObject) -> Result<Media, InvalidMember> {
        Media::decode(data)
    }

    fn table(&self) -> String {
        let rows = self
            .players
            .iter()
            .map(|player| player_row(player, self.active_player_id == Some(player.id)))
            .collect();
        table(&["ID", "STATUS", "FLAGS", "CAN", "NAME", "TRACK"], rows)
    }

    /// Each player with `active` added.
    fn json_rows(&self) -> Vec<Value> {
        wire_rows(&self.encode(), "players")
            .into_iter()
            .map(|mut row| {
                let is_active =
                    self.active_player_id.is_some() && row["id"].as_u64() == self.active_player_id;
                row["active"] = Value::Bool(is_active);
                row
            })
            .collect()
    }

    fn notes(&self) -> Vec<&'static str> {
        notes(&[(self.truncated, CUT_NOTE)])
    }
}

/// The player a command without `--player` controls: `active_player_id`, else the first
/// player that plays, else the first paused one (`docs/PROTOCOL.md` 17.10). The server
/// names no active player until one has played, and a paused player still needs `play`.
pub fn controlled_player(media: &Media) -> Option<u64> {
    let first_with = |status| {
        media
            .players
            .iter()
            .find(|player| player.status == status)
            .map(|player| player.id)
    };
    media
        .active_player_id
        .or_else(|| first_with(PlayerStatus::Playing))
        .or_else(|| first_with(PlayerStatus::Paused))
}

fn player_row(player: &Player, is_active: bool) -> Vec<String> {
    let abilities = [
        (player.can_play, "play"),
        (player.can_pause, "pause"),
        (player.can_next, "next"),
        (player.can_previous, "previous"),
    ];
    vec![
        player.id.to_string(),
        player.status.as_str().to_owned(),
        set_flags(&[(is_active, "active")]),
        set_flags(&abilities),
        replace_unsafe(&player.name),
        track(player),
    ]
}

/// `artist - title`, either one alone, or `-`. Both are absent without scope `state.titles`.
fn track(player: &Player) -> String {
    let parts: Vec<String> = [&player.artist, &player.title]
        .into_iter()
        .flatten()
        .map(|part| replace_unsafe(part))
        .collect();
    if parts.is_empty() {
        or_absent(None)
    } else {
        parts.join(TRACK_SEPARATOR)
    }
}

impl Listing for Apps {
    const TOPIC: &'static str = TOPIC_APPS;

    fn decode(data: &JsonObject) -> Result<Apps, InvalidMember> {
        Apps::decode(data)
    }

    /// In the server's order, by name.
    fn table(&self) -> String {
        let rows = self
            .apps
            .iter()
            .map(|app| {
                let flags = [
                    (self.recent.contains(&app.entry_id), "recent"),
                    (app.risky, "risky"),
                    (app.terminal, "terminal"),
                ];
                vec![
                    replace_unsafe(&app.entry_id),
                    set_flags(&flags),
                    replace_unsafe(&app.name),
                ]
            })
            .collect();
        table(&["ENTRY", "FLAGS", "NAME"], rows)
    }

    /// Each app with `recent` added.
    fn json_rows(&self) -> Vec<Value> {
        wire_rows(&self.encode(), "apps")
            .into_iter()
            .map(|mut row| {
                let is_recent = row["entry_id"]
                    .as_str()
                    .is_some_and(|entry_id| self.recent.iter().any(|id| id == entry_id));
                row["recent"] = Value::Bool(is_recent);
                row
            })
            .collect()
    }

    fn notes(&self) -> Vec<&'static str> {
        notes(&[(self.truncated, CUT_NOTE)])
    }
}

fn wire_rows(encoded: &JsonObject, member: &str) -> Vec<Value> {
    encoded
        .get(member)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn notes(candidates: &[(bool, &'static str)]) -> Vec<&'static str> {
    candidates
        .iter()
        .filter_map(|&(applies, note)| applies.then_some(note))
        .collect()
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
