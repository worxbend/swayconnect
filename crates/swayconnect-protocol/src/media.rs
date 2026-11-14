//! Topic `media` and the media commands (`docs/PROTOCOL.md` 17.10).

use serde_json::Value;

use crate::code::wire_names;
use crate::icon::IconHash;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::message::JsonObject;
use crate::ops::Operation;

/// The state object of topic `media`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub players: Vec<Player>,
    /// The player a simple client controls: the last one that played.
    pub active_player_id: Option<u64>,
    pub truncated: bool,
}

/// One MPRIS player. Every string is untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Issued by the server.
    pub id: u64,
    /// Player identity, at most 64 scalar values.
    pub name: String,
    /// Icon hint.
    pub app_id: Option<String>,
    pub icon: Option<IconHash>,
    pub status: PlayerStatus,
    /// `None` also without scope `state.titles`.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

/// `Player.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerStatus {
    Playing,
    Paused,
    /// Also every status this version does not know.
    Stopped,
}

wire_names!(PlayerStatus {
    Playing => "playing",
    Paused => "paused",
    Stopped => "stopped",
});

/// What a `media.*` operation asks its player to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
}

impl MediaCommand {
    pub const fn operation(self) -> Operation {
        match self {
            MediaCommand::PlayPause => Operation::MediaPlayPause,
            MediaCommand::Next => Operation::MediaNext,
            MediaCommand::Previous => Operation::MediaPrevious,
        }
    }
}

impl Media {
    /// Checks presence and types, and maps an unknown `status` to [`PlayerStatus::Stopped`].
    /// Unknown members are ignored.
    pub fn decode(object: &JsonObject) -> Result<Media, InvalidMember> {
        Ok(Media {
            players: json::object_array(object, "players", Player::decode)?,
            active_player_id: json::nullable_int(object, "active_player_id")?,
            truncated: json::boolean(object, "truncated")?,
        })
    }

    pub fn encode(&self) -> JsonObject {
        let players: Value = self
            .players
            .iter()
            .map(|player| Value::Object(player.encode()))
            .collect();
        object_of([
            ("players", players),
            ("active_player_id", self.active_player_id.into()),
            ("truncated", self.truncated.into()),
        ])
    }
}

impl Player {
    /// The `unsupported_target` rule: `media.next` needs `can_next`, `media.previous` needs
    /// `can_previous`, and `media.play_pause` needs `can_play` or `can_pause`.
    pub const fn allows(&self, command: MediaCommand) -> bool {
        match command {
            MediaCommand::PlayPause => self.can_play || self.can_pause,
            MediaCommand::Next => self.can_next,
            MediaCommand::Previous => self.can_previous,
        }
    }

    fn decode(object: &JsonObject) -> Member<Player> {
        let status = json::string(object, "status")?;
        Ok(Player {
            id: json::int(object, "id")?,
            name: json::string(object, "name")?.to_owned(),
            app_id: json::nullable_string(object, "app_id")?,
            icon: json::nullable(object, "icon", |hash| IconHash::decode_value(hash, "icon"))?,
            status: PlayerStatus::from_wire(status).unwrap_or(PlayerStatus::Stopped),
            title: json::nullable_string(object, "title")?,
            artist: json::nullable_string(object, "artist")?,
            album: json::nullable_string(object, "album")?,
            can_play: json::boolean(object, "can_play")?,
            can_pause: json::boolean(object, "can_pause")?,
            can_next: json::boolean(object, "can_next")?,
            can_previous: json::boolean(object, "can_previous")?,
        })
    }

    fn encode(&self) -> JsonObject {
        object_of([
            ("id", self.id.into()),
            ("name", self.name.as_str().into()),
            ("app_id", self.app_id.clone().into()),
            ("icon", self.icon.map(IconHash::encode_value).into()),
            ("status", self.status.as_str().into()),
            ("title", self.title.clone().into()),
            ("artist", self.artist.clone().into()),
            ("album", self.album.clone().into()),
            ("can_play", self.can_play.into()),
            ("can_pause", self.can_pause.into()),
            ("can_next", self.can_next.into()),
            ("can_previous", self.can_previous.into()),
        ])
    }
}
