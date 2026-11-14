//! The `server` cases of `protocol/vectors/v1/ext/media.json`, and the id and last-played
//! rules of [`Tracker`].

use serde_json::Value;
use swayconnect_protocol::limits::MAX_TOPIC_STATE_BYTES;

use super::*;

const MEDIA_VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../protocol/vectors/v1/ext/media.json"
);

fn vectors() -> Value {
    let text = std::fs::read_to_string(MEDIA_VECTORS).expect("media vectors");
    serde_json::from_str(&text).expect("media vectors are JSON")
}

fn optional_text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn flag(value: &Value, key: &str) -> bool {
    value[key]
        .as_bool()
        .unwrap_or_else(|| panic!("{key} is a bool"))
}

fn report_of(value: &Value) -> PlayerReport {
    PlayerReport {
        bus_name: value["bus_name"].as_str().expect("bus_name").to_owned(),
        identity: optional_text(&value["identity"]),
        desktop_entry: optional_text(&value["desktop_entry"]),
        status: value["status"].as_str().expect("status").to_owned(),
        title: optional_text(&value["title"]),
        artist: optional_text(&value["artist"]),
        album: optional_text(&value["album"]),
        can_play: flag(value, "can_play"),
        can_pause: flag(value, "can_pause"),
        can_go_next: flag(value, "can_go_next"),
        can_go_previous: flag(value, "can_go_previous"),
    }
}

/// The published players without the server-issued `id` and `icon`.
fn wire_players(media: &Media) -> Vec<Value> {
    let encoded = Value::Object(media.encode());
    encoded["players"]
        .as_array()
        .expect("players")
        .iter()
        .map(|player| {
            let mut player = player.as_object().expect("player").clone();
            player.remove("id");
            player.remove("icon");
            Value::Object(player)
        })
        .collect()
}

#[test]
fn the_server_vectors_give_the_expected_media_state() {
    let file = vectors();
    let cases = file["server"].as_array().expect("server cases");
    assert!(cases.len() >= 11, "the file holds cases 01 to 11");
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let tracked: Vec<TrackedPlayer> = case["players"]
            .as_array()
            .expect("players")
            .iter()
            .zip(1..)
            .map(|(player, id)| TrackedPlayer {
                id,
                last_playing: player["last_playing"].as_u64(),
                report: report_of(player),
            })
            .collect();
        let with_titles = case["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .any(|scope| scope == "state.titles");
        let media = MediaFacts::build(&tracked, |_| None).state_for(with_titles);

        let expect = &case["expect"];
        let expected_players = expect["players"].as_array().expect("expected players");
        assert_eq!(&wire_players(&media), expected_players, "{name}: players");
        let active = expect["active"]
            .as_u64()
            .map(|index| media.players[usize::try_from(index).expect("index")].id);
        assert_eq!(media.active_player_id, active, "{name}: active player");
        assert_eq!(
            Value::Bool(media.truncated),
            expect["truncated"],
            "{name}: truncated"
        );
    }
}

fn report(bus_suffix: &str, status: &str) -> PlayerReport {
    PlayerReport {
        bus_name: format!("{MPRIS_PREFIX}{bus_suffix}"),
        identity: Some(bus_suffix.to_owned()),
        desktop_entry: None,
        status: status.to_owned(),
        title: Some("title".to_owned()),
        artist: None,
        album: None,
        can_play: true,
        can_pause: true,
        can_go_next: false,
        can_go_previous: false,
    }
}

#[test]
fn ids_stay_with_their_player_and_are_never_reused() {
    let mut tracker = Tracker::default();
    let first = tracker.observe(vec![report("a", "Paused"), report("b", "Paused")], &[]);
    assert_eq!(first.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);

    let second = tracker.observe(vec![report("b", "Paused"), report("c", "Paused")], &[]);
    assert_eq!(second.iter().map(|p| p.id).collect::<Vec<_>>(), [2, 3]);

    // `a` left; when it comes back it is a new player.
    let third = tracker.observe(vec![report("a", "Paused")], &[]);
    assert_eq!(third[0].id, 4);
}

#[test]
fn a_player_whose_read_failed_keeps_its_id_and_active_mark() {
    let mut tracker = Tracker::default();
    tracker.observe(vec![report("a", "Playing"), report("b", "Paused")], &[]);
    let unread = ["org.mpris.MediaPlayer2.a".to_owned()];
    let missed = tracker.observe(vec![report("b", "Paused")], &unread);
    assert_eq!(missed.iter().map(|p| p.id).collect::<Vec<_>>(), [2]);

    let back = tracker.observe(vec![report("a", "Playing"), report("b", "Paused")], &[]);
    assert_eq!(back.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        MediaFacts::build(&back, |_| None).active_player_id,
        Some(1),
        "`a` still plays: its mark was kept, it is no new start"
    );
}

#[test]
fn a_bus_name_reported_twice_counts_once() {
    let mut tracker = Tracker::default();
    let tracked = tracker.observe(vec![report("a", "Paused"), report("a", "Playing")], &[]);
    assert_eq!(tracked.len(), 1);
    assert_eq!(tracked[0].report.status, "Paused");
}

#[test]
fn the_player_that_started_playing_last_is_active() {
    let mut tracker = Tracker::default();
    let players = tracker.observe(vec![report("a", "Playing"), report("b", "Paused")], &[]);
    assert_eq!(
        MediaFacts::build(&players, |_| None).active_player_id,
        Some(1)
    );

    // `b` starts while `a` still plays: `b` is the last that played.
    let players = tracker.observe(vec![report("a", "Playing"), report("b", "Playing")], &[]);
    assert_eq!(
        MediaFacts::build(&players, |_| None).active_player_id,
        Some(2)
    );

    // `b` pauses: it stays active, because it played last; `a` playing on is no new start.
    let players = tracker.observe(vec![report("a", "Playing"), report("b", "Paused")], &[]);
    assert_eq!(
        MediaFacts::build(&players, |_| None).active_player_id,
        Some(2)
    );

    // `a` stops and starts again: now it is the last one that started.
    tracker.observe(vec![report("a", "Paused"), report("b", "Paused")], &[]);
    let players = tracker.observe(vec![report("a", "Playing"), report("b", "Paused")], &[]);
    assert_eq!(
        MediaFacts::build(&players, |_| None).active_player_id,
        Some(1)
    );
}

#[test]
fn no_player_ever_played_means_no_active_player() {
    let mut tracker = Tracker::default();
    let players = tracker.observe(vec![report("a", "Paused"), report("b", "Stopped")], &[]);
    assert_eq!(MediaFacts::build(&players, |_| None).active_player_id, None);
}

#[test]
fn a_player_without_identity_is_named_by_its_bus_name() {
    let mut nameless = report("vlc.instance7", "Paused");
    nameless.identity = None;
    let tracked = [TrackedPlayer {
        id: 1,
        last_playing: None,
        report: nameless,
    }];
    let media = MediaFacts::build(&tracked, |_| None).state_for(true);
    assert_eq!(media.players[0].name, "vlc.instance7");
}

#[test]
fn the_icon_comes_from_the_lookup_and_titles_go_only_with_state_titles() {
    let hash = IconHash::of(b"seven");
    let tracked = [TrackedPlayer {
        id: 1,
        last_playing: Some(1),
        report: report("a", "Playing"),
    }];
    let facts = MediaFacts::build(&tracked, |_| Some(hash));
    assert_eq!(facts.state_for(true).players[0].icon, Some(hash));
    assert_eq!(
        facts.state_for(true).players[0].title.as_deref(),
        Some("title")
    );
    assert_eq!(facts.state_for(false).players[0].title, None);
    assert_eq!(
        facts.find(1).map(|fact| fact.bus_name.as_str()),
        Some("org.mpris.MediaPlayer2.a")
    );
    assert!(facts.find(2).is_none());
}

#[test]
fn the_largest_possible_state_fits_the_topic_size_limit() {
    // Four-byte scalars everywhere, every string at its longest, 16 players.
    let long = |scalars: usize| Some("\u{1F3B5}".repeat(scalars + 10));
    let tracked: Vec<TrackedPlayer> = (1..=40)
        .map(|id| TrackedPlayer {
            id,
            last_playing: Some(id),
            report: PlayerReport {
                bus_name: format!("{MPRIS_PREFIX}p{id}"),
                identity: long(MAX_PLAYER_NAME_SCALARS),
                desktop_entry: long(MAX_APP_ID_SCALARS),
                status: "Playing".to_owned(),
                title: long(MAX_TRACK_TEXT_SCALARS),
                artist: long(MAX_TRACK_TEXT_SCALARS),
                album: long(MAX_TRACK_TEXT_SCALARS),
                can_play: true,
                can_pause: true,
                can_go_next: true,
                can_go_previous: true,
            },
        })
        .collect();
    let facts = MediaFacts::build(&tracked, |_| Some(IconHash::of(b"one")));
    let media = facts.state_for(true);
    assert_eq!(media.players.len(), MAX_PLAYERS);
    assert!(media.truncated);
    assert_eq!(
        media.active_player_id,
        Some(40),
        "the active player survives the cut"
    );
    let encoded = serde_json::to_string(&media.encode()).expect("encodes");
    assert!(
        encoded.len() <= MAX_TOPIC_STATE_BYTES,
        "{} bytes",
        encoded.len()
    );
}
