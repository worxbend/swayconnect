use swayconnect_protocol::DeviceKind;
use swayconnect_protocol::limits::{MAX_LABEL_SCALARS, MAX_TOPIC_STATE_BYTES};

use super::*;

const MAX: u16 = 100;

fn node(name: &str) -> Node {
    Node {
        name: name.to_owned(),
        description: format!("{name} description"),
        kind: DeviceKind::Speaker,
        volume_pct: 40,
        muted: false,
        available: true,
    }
}

fn lists(sinks: &[&str], sources: &[&str]) -> Lists {
    Lists {
        sinks: sinks.iter().map(|name| node(name)).collect(),
        sources: sources.iter().map(|name| node(name)).collect(),
        default_sink: sinks.first().map(|name| (*name).to_owned()),
        default_source: sources.first().map(|name| (*name).to_owned()),
    }
}

fn ids(devices: &[Device]) -> Vec<u64> {
    devices.iter().map(|device| device.id).collect()
}

#[test]
fn ids_are_issued_once_and_stay_across_snapshots() {
    let mut registry = Registry::default();
    let first = registry.build(Some(lists(&["a", "b"], &["mic"])), MAX);
    assert!(first.audio().available);
    assert_eq!(ids(&first.audio().sinks), [1, 2]);
    assert_eq!(ids(&first.audio().sources), [3]);
    assert_eq!(first.audio().default_sink, Some(1));
    assert_eq!(first.audio().default_source, Some(3));

    // Order changes, a device joins: old ids stay, the new one gets the next id.
    let second = registry.build(Some(lists(&["c", "b", "a"], &["mic"])), MAX);
    assert_eq!(ids(&second.audio().sinks), [4, 2, 1]);
    assert_eq!(second.audio().default_sink, Some(4));
    assert_eq!(
        second.target(2),
        Some(&Target {
            direction: Direction::Sink,
            name: "b".to_owned()
        })
    );
}

#[test]
fn a_vanished_device_loses_its_id() {
    let mut registry = Registry::default();
    registry.build(Some(lists(&["a", "b"], &[])), MAX);
    let without_b = registry.build(Some(lists(&["a"], &[])), MAX);
    assert_eq!(without_b.target(2), None, "b's id is not_found now");

    let back = registry.build(Some(lists(&["a", "b"], &[])), MAX);
    assert_eq!(
        ids(&back.audio().sinks),
        [1, 3],
        "b comes back with a new id"
    );
}

#[test]
fn a_sink_and_a_source_with_one_name_get_two_ids() {
    let mut registry = Registry::default();
    let snapshot = registry.build(Some(lists(&["same"], &["same"])), MAX);
    assert_eq!(ids(&snapshot.audio().sinks), [1]);
    assert_eq!(ids(&snapshot.audio().sources), [2]);
    assert_eq!(
        snapshot.target(2).map(|t| t.direction),
        Some(Direction::Source)
    );
}

#[test]
fn an_outage_publishes_unavailable_and_keeps_the_ids() {
    let mut registry = Registry::default();
    registry.build(Some(lists(&["a", "b"], &["mic"])), MAX);
    let down = registry.build(None, MAX);
    assert_eq!(down, Snapshot::unavailable(MAX));
    assert!(!down.audio().available);
    assert_eq!(down.target(1), None);

    let up = registry.build(Some(lists(&["a", "b"], &["mic"])), MAX);
    assert_eq!(ids(&up.audio().sinks), [1, 2]);
    assert_eq!(ids(&up.audio().sources), [3]);
}

#[test]
fn caps_are_64_each_and_set_truncated() {
    let many: Vec<String> = (0..70).map(|index| format!("node{index}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    let mut registry = Registry::default();

    let snapshot = registry.build(Some(lists(&names, &names[..64])), MAX);
    assert_eq!(snapshot.audio().sinks.len(), MAX_AUDIO_DEVICES);
    assert_eq!(snapshot.audio().sources.len(), MAX_AUDIO_DEVICES);
    assert!(snapshot.audio().truncated);
    assert_eq!(snapshot.audio().sinks[63].description, "node63 description");

    let fits = registry.build(Some(lists(&names[..64], &names[..64])), MAX);
    assert!(!fits.audio().truncated);
}

#[test]
fn an_unlisted_or_dropped_default_is_null() {
    let mut registry = Registry::default();
    let mut gone = lists(&["a"], &["mic"]);
    gone.default_sink = Some("not-listed".to_owned());
    gone.default_source = None;
    let snapshot = registry.build(Some(gone), MAX);
    assert_eq!(snapshot.audio().default_sink, None);
    assert_eq!(snapshot.audio().default_source, None);

    let mut wrong_list = lists(&["a"], &["mic"]);
    wrong_list.default_sink = Some("mic".to_owned());
    let snapshot = registry.build(Some(wrong_list), MAX);
    assert_eq!(
        snapshot.audio().default_sink,
        None,
        "a source is no default sink"
    );
}

#[test]
fn a_repeated_name_is_listed_once() {
    let mut registry = Registry::default();
    let snapshot = registry.build(Some(lists(&["a", "a", "b"], &[])), MAX);
    assert_eq!(ids(&snapshot.audio().sinks), [1, 2]);
}

#[test]
fn the_fullest_state_stays_under_the_topic_size_rule() {
    let description = "\u{1F600}".repeat(MAX_LABEL_SCALARS);
    let fill = |direction: &str| -> Vec<Node> {
        (0..MAX_AUDIO_DEVICES)
            .map(|index| Node {
                description: description.clone(),
                kind: DeviceKind::Headphones,
                volume_pct: 1000,
                ..node(&format!("{direction}{index}"))
            })
            .collect()
    };
    let full = Lists {
        sinks: fill("sink"),
        sources: fill("source"),
        default_sink: Some("sink0".to_owned()),
        default_source: Some("source0".to_owned()),
    };
    let snapshot = Registry::default().build(Some(full), 150);
    let encoded = serde_json::to_vec(&snapshot.audio().encode()).expect("encode");
    assert!(
        encoded.len() < MAX_TOPIC_STATE_BYTES,
        "{} bytes",
        encoded.len()
    );
    let reread = Audio::decode(&snapshot.audio().encode()).expect("a client decodes it");
    assert_eq!(&reread, snapshot.audio());
}
