//! Pure: the player list a backend reports becomes the `media` state (17.10, and the rules of
//! `protocol/vectors/v1/README.md` `ext/media.json`).
//!
//! [`Tracker`] issues the ids and remembers which player played last; [`MediaFacts::build`]
//! skips `playerctld`, picks the active player, applies the cap of 16 and cleans every string.
//! [`MediaFacts::state_for`] withholds titles from a device without `state.titles`.

use std::collections::{HashMap, HashSet};

use swayconnect_protocol::limits::{MAX_PLAYER_NAME_SCALARS, MAX_PLAYERS};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{IconHash, Media, Player, PlayerStatus};

use super::{MPRIS_PREFIX, PLAYERCTLD, PlayerReport};

/// `Player.app_id`, in Unicode scalar values.
pub const MAX_APP_ID_SCALARS: usize = 128;
/// `Player.title`, `artist` and `album`, each, in Unicode scalar values.
pub const MAX_TRACK_TEXT_SCALARS: usize = 200;

/// A reported player with the server's id and its last-played mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedPlayer {
    pub id: u64,
    /// When it last started playing: higher is later, `None` never seen playing.
    pub last_playing: Option<u64>,
    pub report: PlayerReport,
}

/// One player of the state, with the bus name it is reached by. The bus name never goes on
/// the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactPlayer {
    pub bus_name: String,
    /// Cleaned, cut, with titles.
    pub player: Player,
}

/// The `media` state before it is built for one device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaFacts {
    pub players: Vec<FactPlayer>,
    pub active_player_id: Option<u64>,
    pub truncated: bool,
}

impl MediaFacts {
    /// Skips `playerctld`; the active player is the one with the highest `last_playing`
    /// (the first of equals); above 16 players, players are dropped from the end, never the
    /// active one. `icon` gives the icon hash for a report.
    pub fn build(
        players: &[TrackedPlayer],
        icon: impl Fn(&PlayerReport) -> Option<IconHash>,
    ) -> MediaFacts {
        let mut kept: Vec<&TrackedPlayer> = players
            .iter()
            .filter(|player| player.report.bus_name != PLAYERCTLD)
            .collect();
        let active = active_index(&kept);
        let active_player_id = active.map(|index| kept[index].id);
        let truncated = kept.len() > MAX_PLAYERS;
        if truncated {
            match active {
                Some(index) if index >= MAX_PLAYERS => {
                    let active_player = kept[index];
                    kept.truncate(MAX_PLAYERS - 1);
                    kept.push(active_player);
                }
                _ => kept.truncate(MAX_PLAYERS),
            }
        }
        MediaFacts {
            players: kept
                .into_iter()
                .map(|tracked| FactPlayer {
                    bus_name: tracked.report.bus_name.clone(),
                    player: player_of(tracked, icon(&tracked.report)),
                })
                .collect(),
            active_player_id,
            truncated,
        }
    }

    /// The state object for a device; `with_titles` is whether it holds `state.titles`.
    pub fn state_for(&self, with_titles: bool) -> Media {
        Media {
            players: self
                .players
                .iter()
                .map(|fact| {
                    let mut player = fact.player.clone();
                    if !with_titles {
                        (player.title, player.artist, player.album) = (None, None, None);
                    }
                    player
                })
                .collect(),
            active_player_id: self.active_player_id,
            truncated: self.truncated,
        }
    }

    pub fn find(&self, player_id: u64) -> Option<&FactPlayer> {
        self.players.iter().find(|fact| fact.player.id == player_id)
    }
}

fn active_index(players: &[&TrackedPlayer]) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (index, player) in players.iter().enumerate() {
        if let Some(mark) = player.last_playing
            && best.is_none_or(|(_, best_mark)| mark > best_mark)
        {
            best = Some((index, mark));
        }
    }
    best.map(|(index, _)| index)
}

fn player_of(tracked: &TrackedPlayer, icon: Option<IconHash>) -> Player {
    let report = &tracked.report;
    let name = report
        .identity
        .as_deref()
        .unwrap_or_else(|| short_name(&report.bus_name));
    let track_text = |text: &Option<String>| {
        text.as_deref()
            .map(|text| clean(text, MAX_TRACK_TEXT_SCALARS))
    };
    Player {
        id: tracked.id,
        name: clean(name, MAX_PLAYER_NAME_SCALARS),
        app_id: report
            .desktop_entry
            .as_deref()
            .map(|entry| clean(entry, MAX_APP_ID_SCALARS)),
        icon,
        status: status_of(&report.status),
        title: track_text(&report.title),
        artist: track_text(&report.artist),
        album: track_text(&report.album),
        can_play: report.can_play,
        can_pause: report.can_pause,
        can_next: report.can_go_next,
        can_previous: report.can_go_previous,
    }
}

/// The bus name without the MPRIS prefix: the name of a player that sends no `Identity`.
fn short_name(bus_name: &str) -> &str {
    bus_name.strip_prefix(MPRIS_PREFIX).unwrap_or(bus_name)
}

/// Spec 9.3 rules 2 to 4 for server strings: each control or format character becomes U+FFFD,
/// then the text is cut to `max_scalars` Unicode scalar values.
pub fn clean(text: &str, max_scalars: usize) -> String {
    replace_unsafe(text).chars().take(max_scalars).collect()
}

/// MPRIS `PlaybackStatus` to the wire. Anything unknown is `stopped`.
pub fn status_of(status: &str) -> PlayerStatus {
    match status {
        "Playing" => PlayerStatus::Playing,
        "Paused" => PlayerStatus::Paused,
        _ => PlayerStatus::Stopped,
    }
}

/// Issues player ids (stable per bus name while the player stays, never reused) and the
/// last-played marks: a player gets a new mark each time it is seen starting to play.
#[derive(Debug, Default)]
pub struct Tracker {
    last_id: u64,
    clock: u64,
    known: HashMap<String, Known>,
}

#[derive(Debug)]
struct Known {
    id: u64,
    playing: bool,
    last_playing: Option<u64>,
}

impl Tracker {
    /// One snapshot of the backend. A bus name reported twice keeps its first report.
    /// `unread` names are on the bus but did not answer: they are not tracked this time,
    /// but keep their id and marks. Every other player not in the snapshot is forgotten.
    pub fn observe(&mut self, reports: Vec<PlayerReport>, unread: &[String]) -> Vec<TrackedPlayer> {
        let mut seen = HashSet::new();
        let mut tracked = Vec::with_capacity(reports.len());
        for report in reports {
            if !seen.insert(report.bus_name.clone()) {
                continue;
            }
            let known = match self.known.get_mut(&report.bus_name) {
                Some(known) => known,
                None => {
                    self.last_id += 1;
                    self.known.entry(report.bus_name.clone()).or_insert(Known {
                        id: self.last_id,
                        playing: false,
                        last_playing: None,
                    })
                }
            };
            let playing = report.is_playing();
            if playing && !known.playing {
                self.clock += 1;
                known.last_playing = Some(self.clock);
            }
            known.playing = playing;
            tracked.push(TrackedPlayer {
                id: known.id,
                last_playing: known.last_playing,
                report,
            });
        }
        self.known
            .retain(|bus_name, _| seen.contains(bus_name) || unread.contains(bus_name));
        tracked
    }
}

#[cfg(test)]
mod tests;
