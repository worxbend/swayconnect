use serde_json::json;
use swayconnect_protocol::text::is_control_or_format;

use super::*;

const HOSTILE: &str = "a\u{1b}[2Jb\u{202e}c";
const REPLACED: &str = "a\u{fffd}[2Jb\u{fffd}c";

fn object(value: Value) -> JsonObject {
    let Value::Object(object) = value else {
        panic!("not an object");
    };
    object
}

fn device(id: u64, kind: &str, description: &str) -> Value {
    json!({
        "id": id, "description": description, "kind": kind,
        "volume_pct": 40, "muted": id == 7, "available": id != 8,
    })
}

fn audio() -> Audio {
    Audio::decode(&object(json!({
        "available": true, "max_volume_pct": 150, "default_sink": 5, "default_source": 7,
        "sinks": [device(5, "speaker", "Speakers"), device(8, "hdmi", HOSTILE)],
        "sources": [device(7, "microphone", "Mic")],
        "truncated": false,
    })))
    .expect("audio state")
}

fn player(id: u64, artist: Option<&str>, title: Option<&str>) -> Value {
    json!({
        "id": id, "name": "mpv", "app_id": null, "icon": null, "status": "playing",
        "title": title, "artist": artist, "album": null,
        "can_play": true, "can_pause": true, "can_next": id == 4, "can_previous": false,
    })
}

fn media() -> Media {
    Media::decode(&object(json!({
        "players": [
            player(4, Some("Artist"), Some(HOSTILE)),
            player(9, None, Some("Only title")),
            player(11, None, None),
        ],
        "active_player_id": 4,
        "truncated": true,
    })))
    .expect("media state")
}

fn app(entry_id: &str, name: &str, terminal: bool) -> Value {
    json!({
        "entry_id": entry_id, "name": name, "generic_name": null, "keywords": [],
        "icon": null, "terminal": terminal, "risky": terminal,
    })
}

fn apps() -> Apps {
    Apps::decode(&object(json!({
        "apps": [app("foot", "Foot", true), app("notes", HOSTILE, false)],
        "recent": ["notes"],
        "truncated": false,
    })))
    .expect("apps state")
}

#[test]
fn audio_lists_sinks_then_sources_with_their_flags() {
    let expected = format!(
        "\
ID  ROLE    KIND        VOLUME  FLAGS          DESCRIPTION
5   sink    speaker     40%     default        Speakers
8   sink    hdmi        40%     unplugged      {REPLACED}
7   source  microphone  40%     default,muted  Mic"
    );

    assert_eq!(audio().table(), expected);
    assert!(audio().notes().is_empty());
}

#[test]
fn audio_json_rows_add_the_role_and_the_default() {
    let rows = audio().json_rows();

    let roles: Vec<_> = rows
        .iter()
        .map(|row| (&row["role"], &row["default"]))
        .collect();
    assert_eq!(
        roles,
        [
            (&json!("sink"), &json!(true)),
            (&json!("sink"), &json!(false)),
            (&json!("source"), &json!(true)),
        ]
    );
    assert_eq!(rows[1]["description"], HOSTILE, "JSON keeps the value");
    assert_eq!(rows[0]["volume_pct"], 40);
}

#[test]
fn an_unreachable_sound_server_shows_only_the_header_and_a_note() {
    let audio = Audio::decode(&object(json!({
        "available": false, "max_volume_pct": 100, "default_sink": null,
        "default_source": null, "sinks": [], "sources": [], "truncated": true,
    })))
    .expect("audio state");

    assert_eq!(audio.table(), "ID  ROLE  KIND  VOLUME  FLAGS  DESCRIPTION");
    assert_eq!(audio.json_rows(), Vec::<Value>::new());
    assert_eq!(
        audio.notes(),
        ["note: the sound server cannot be reached", CUT_NOTE]
    );
}

#[test]
fn media_marks_the_active_player_and_joins_artist_and_title() {
    let expected = format!(
        "\
ID  STATUS   FLAGS   CAN              NAME  TRACK
4   playing  active  play,pause,next  mpv   Artist - {REPLACED}
9   playing  -       play,pause       mpv   Only title
11  playing  -       play,pause       mpv   -"
    );

    assert_eq!(media().table(), expected);
    assert_eq!(media().notes(), [CUT_NOTE]);
    let active: Vec<_> = media()
        .json_rows()
        .iter()
        .map(|row| row["active"].clone())
        .collect();
    assert_eq!(active, [json!(true), json!(false), json!(false)]);
}

#[test]
fn a_command_without_a_player_controls_the_active_then_playing_then_paused_one() {
    let with = |active: Value, statuses: &[(u64, &str)]| {
        let players: Vec<Value> = statuses
            .iter()
            .map(|(id, status)| {
                let mut player = player(*id, None, None);
                player["status"] = json!(status);
                player
            })
            .collect();
        let state = json!({"players": players, "active_player_id": active, "truncated": false});
        controlled_player(&Media::decode(&object(state)).expect("media state"))
    };

    let paused = [(3, "stopped"), (5, "paused"), (6, "paused")];
    assert_eq!(with(Value::Null, &paused), Some(5), "the first paused one");
    let playing = [(3, "paused"), (5, "playing")];
    assert_eq!(
        with(Value::Null, &playing),
        Some(5),
        "playing before paused"
    );
    assert_eq!(with(json!(3), &playing), Some(3), "the active one first");
    assert_eq!(with(Value::Null, &[(3, "stopped")]), None);
    assert_eq!(with(Value::Null, &[]), None);
}

#[test]
fn apps_show_the_entry_its_flags_and_the_name() {
    let expected = format!(
        "\
ENTRY  FLAGS           NAME
foot   risky,terminal  Foot
notes  recent          {REPLACED}"
    );

    assert_eq!(apps().table(), expected);
    let rows = apps().json_rows();
    assert_eq!(rows[0]["recent"], false);
    assert_eq!(rows[1]["recent"], true);
    assert_eq!(rows[1]["name"], HOSTILE);
}

#[test]
fn no_table_holds_a_control_or_format_character() {
    for table in [audio().table(), media().table(), apps().table()] {
        let unsafe_lines: Vec<&str> = table
            .lines()
            .filter(|line| line.chars().any(is_control_or_format))
            .collect();
        assert!(unsafe_lines.is_empty(), "{unsafe_lines:?}");
    }
}
