//! The state of topic `audio` and the pactl names behind its ids (`docs/DESIGN.md` 11.6).
//! Ids are small integers the daemon issues; a client never sees a pactl name.

use std::collections::{HashMap, HashSet};

use swayconnect_protocol::limits::MAX_AUDIO_DEVICES;
use swayconnect_protocol::{Audio, Device};

use super::pactl::{Direction, Node};

/// What one refresh read from pactl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lists {
    pub sinks: Vec<Node>,
    pub sources: Vec<Node>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
}

/// The pactl name behind one id of the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub direction: Direction,
    pub name: String,
}

/// Topic `audio` as published, and the target of each listed id. Only listed devices have a
/// target, so an id that is not in the state is `not_found`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub(super) audio: Audio,
    pub(super) targets: HashMap<u64, Target>,
}

impl Snapshot {
    /// The sound server cannot be reached: both lists empty (17.9).
    pub fn unavailable(max_volume_pct: u16) -> Snapshot {
        Snapshot {
            audio: Audio {
                available: false,
                max_volume_pct,
                default_sink: None,
                default_source: None,
                sinks: Vec::new(),
                sources: Vec::new(),
                truncated: false,
            },
            targets: HashMap::new(),
        }
    }

    /// The state object of topic `audio`.
    pub fn audio(&self) -> &Audio {
        &self.audio
    }

    pub fn target(&self, id: u64) -> Option<&Target> {
        self.targets.get(&id)
    }
}

/// Issues ids. An id stays with its (direction, name) while the device is listed; a device
/// that left the lists loses its id, and comes back with a new one.
#[derive(Debug, Default)]
pub struct Registry {
    last_id: u64,
    ids: HashMap<(Direction, String), u64>,
}

impl Registry {
    /// Builds the published state. `None` (pactl failed) keeps every id, so a short outage of
    /// the sound server changes no id.
    pub fn build(&mut self, lists: Option<Lists>, max_volume_pct: u16) -> Snapshot {
        let Some(lists) = lists else {
            return Snapshot::unavailable(max_volume_pct);
        };
        let mut snapshot = Snapshot::unavailable(max_volume_pct);
        snapshot.audio.available = true;
        let (sinks, sinks_cut) = capped(lists.sinks);
        let (sources, sources_cut) = capped(lists.sources);
        snapshot.audio.truncated = sinks_cut || sources_cut;
        let listed: HashSet<(Direction, String)> = sinks
            .iter()
            .map(|node| (Direction::Sink, node.name.clone()))
            .chain(
                sources
                    .iter()
                    .map(|node| (Direction::Source, node.name.clone())),
            )
            .collect();
        self.ids.retain(|key, _| listed.contains(key));
        snapshot.audio.sinks = self.issue(&mut snapshot.targets, Direction::Sink, sinks);
        snapshot.audio.sources = self.issue(&mut snapshot.targets, Direction::Source, sources);
        snapshot.audio.default_sink =
            default_id(&snapshot.targets, Direction::Sink, &lists.default_sink);
        snapshot.audio.default_source =
            default_id(&snapshot.targets, Direction::Source, &lists.default_source);
        snapshot
    }

    fn issue(
        &mut self,
        targets: &mut HashMap<u64, Target>,
        direction: Direction,
        nodes: Vec<Node>,
    ) -> Vec<Device> {
        nodes
            .into_iter()
            .map(|node| {
                let id = self.id_of(direction, &node.name);
                let device = Device {
                    id,
                    description: node.description,
                    kind: node.kind,
                    volume_pct: node.volume_pct,
                    muted: node.muted,
                    available: node.available,
                };
                targets.insert(
                    id,
                    Target {
                        direction,
                        name: node.name,
                    },
                );
                device
            })
            .collect()
    }

    fn id_of(&mut self, direction: Direction, name: &str) -> u64 {
        let key = (direction, name.to_owned());
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        self.last_id += 1;
        self.ids.insert(key, self.last_id);
        self.last_id
    }
}

/// The first [`MAX_AUDIO_DEVICES`] distinct names, and whether any device was dropped for the
/// cap. A repeated name (pactl lists each once) is dropped silently.
fn capped(nodes: Vec<Node>) -> (Vec<Node>, bool) {
    let mut seen = HashSet::new();
    let mut unique: Vec<Node> = nodes
        .into_iter()
        .filter(|node| seen.insert(node.name.clone()))
        .collect();
    let cut = unique.len() > MAX_AUDIO_DEVICES;
    unique.truncate(MAX_AUDIO_DEVICES);
    (unique, cut)
}

/// The id of the default device, when it is listed.
fn default_id(
    targets: &HashMap<u64, Target>,
    direction: Direction,
    name: &Option<String>,
) -> Option<u64> {
    let name = name.as_deref()?;
    targets
        .iter()
        .find(|(_, target)| target.direction == direction && target.name == name)
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests;
