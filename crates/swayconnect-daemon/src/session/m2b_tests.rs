//! Tests of the M2b answers (`session/services.rs`, `session/feeds.rs`): topics `audio`,
//! `media`, `apps` and the new `session` members, and the ops of 17.9 to 17.13 through
//! their services, with the audit lines. Fake programs, a fake keyboard and a fake media
//! backend only; there is no sway, so every focus read fails.

use std::fs;

use serde_json::{Value, json};

use super::harness::{Client, Daemon};
use super::*;
use crate::audit::audit_file;
use crate::media::PlayerReport;
use swayconnect_protocol::MediaCommand;

const TEST_DEADLINE: Duration = Duration::from_secs(10);
const ALL_M2B: [Scope; 8] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Audio,
    Scope::Media,
    Scope::Launch,
    Scope::InputKeys,
    Scope::InputText,
    Scope::Control,
];

fn request(id: u64, op: &str, args: Value) -> Value {
    json!({"type": "req", "id": id, "op": op, "args": args})
}

fn subscribe_to(id: u64, topics: &[&str]) -> Value {
    request(id, "state.subscribe", json!({"topics": topics}))
}

async fn result_of(client: &mut Client) -> JsonObject {
    let res = timeout(TEST_DEADLINE, client.receive())
        .await
        .expect("a res in time");
    assert_eq!(res["ok"], true, "{res:?}");
    res["result"].as_object().expect("result").clone()
}

/// The `error` of a failed `res`.
async fn error_of(client: &mut Client) -> JsonObject {
    let res = timeout(TEST_DEADLINE, client.receive())
        .await
        .expect("a res in time");
    assert_eq!(res["ok"], false, "{res:?}");
    res["error"].as_object().expect("error").clone()
}

/// The next `state.changed` of `topic` whose state passes `wanted`, skipping other frames.
async fn state_where(
    client: &mut Client,
    topic: &str,
    wanted: impl Fn(&Value) -> bool,
) -> (u64, Value) {
    loop {
        let frame = timeout(TEST_DEADLINE, client.receive())
            .await
            .expect("a frame in time");
        let data = &frame["data"];
        if frame["type"] == "evt" && data["topic"] == topic && wanted(&data["data"]) {
            return (data["rev"].as_u64().expect("rev"), data["data"].clone());
        }
    }
}

fn audit_text(daemon: &Daemon) -> String {
    let paths = crate::paths::Paths {
        config_file: daemon.dirs.config_file(),
        data_dir: daemon.dirs.data_dir(),
        runtime_dir: daemon.dirs.runtime_dir(),
    };
    fs::read_to_string(audit_file(&paths)).unwrap_or_default()
}

fn audit_lines(daemon: &Daemon) -> Vec<Value> {
    audit_text(daemon)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

fn player(bus_name: &str, title: &str) -> PlayerReport {
    PlayerReport {
        bus_name: bus_name.to_owned(),
        identity: Some("Player".to_owned()),
        desktop_entry: None,
        status: "Playing".to_owned(),
        title: Some(title.to_owned()),
        artist: None,
        album: None,
        can_play: true,
        can_pause: true,
        can_go_next: false,
        can_go_previous: false,
    }
}

#[tokio::test]
async fn each_m2b_topic_needs_state_read_and_its_own_scope_and_unknown_names_go_first() {
    let daemon = Daemon::start().await;
    let reader = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, welcome) = daemon.ready(&reader).await;
    assert_eq!(welcome["topics"], json!(["desktop", "session"]));

    client.send(&subscribe_to(1, &["desktop", "apps"])).await;
    let error = error_of(&mut client).await;
    assert_eq!(error["code"], "forbidden");
    assert_eq!(error["detail"], json!({"scope": "launch"}));

    client.send(&subscribe_to(2, &["audio", "clipboard"])).await;
    assert_eq!(error_of(&mut client).await["code"], "unknown_topic");

    let full = daemon.paired_device(&ALL_M2B).await;
    let (_, welcome) = daemon.ready(&full).await;
    assert_eq!(
        welcome["topics"],
        json!(["desktop", "session", "audio", "media", "apps"])
    );
}

#[tokio::test]
async fn the_m2b_states_start_at_their_own_revision_and_session_names_input_and_launcher() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let (mut client, _) = daemon.ready(&device).await;

    client
        .send(&subscribe_to(1, &["audio", "media", "apps", "session"]))
        .await;
    let result = result_of(&mut client).await;

    let states = result["states"].as_array().expect("states");
    let topics: Vec<&Value> = states.iter().map(|state| &state["topic"]).collect();
    assert_eq!(
        topics,
        [
            &json!("audio"),
            &json!("media"),
            &json!("apps"),
            &json!("session")
        ]
    );
    assert!(states.iter().all(|state| state["rev"] == 1), "{states:?}");
    assert_eq!(states[0]["data"]["max_volume_pct"], 100);
    assert_eq!(states[1]["data"]["players"], json!([]));
    let apps: Vec<&Value> = states[2]["data"]["apps"]
        .as_array()
        .expect("apps")
        .iter()
        .map(|app| &app["entry_id"])
        .collect();
    assert_eq!(apps, [&json!("notes"), &json!("term")]);
    assert_eq!(states[2]["data"]["apps"][1]["risky"], true);
    assert_eq!(
        states[3]["data"]["input"],
        json!({"active": false, "holder": null})
    );
    assert_eq!(
        states[3]["data"]["launcher"],
        json!({"open": false, "holder": null})
    );

    // A second subscription never goes back in revisions (spec 8 rule 10).
    client.send(&subscribe_to(2, &["apps"])).await;
    let again = result_of(&mut client).await;
    assert_eq!(again["states"][0]["rev"], 2);
}

#[tokio::test]
async fn a_media_command_reaches_the_player_and_titles_follow_state_titles() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let without_titles = daemon
        .paired_device(&[Scope::StateRead, Scope::Media])
        .await;
    let (mut client, _) = daemon.ready(&device).await;
    let (mut other, _) = daemon.ready(&without_titles).await;
    client.send(&subscribe_to(1, &["media"])).await;
    result_of(&mut client).await;
    other.send(&subscribe_to(1, &["media"])).await;
    result_of(&mut other).await;

    let bus_name = "org.mpris.MediaPlayer2.fake";
    daemon
        .media
        .set_players(Ok(vec![player(bus_name, "A private title")]));
    let has_player = |state: &Value| state["players"].as_array().is_some_and(|p| !p.is_empty());
    let (rev, state) = state_where(&mut client, "media", has_player).await;
    assert!(rev > 1);
    assert_eq!(state["players"][0]["title"], "A private title");
    let (_, hidden) = state_where(&mut other, "media", has_player).await;
    assert_eq!(hidden["players"][0]["title"], Value::Null);

    let player_id = state["players"][0]["id"].clone();
    let play = json!({"player_id": player_id});
    client.send(&request(2, "media.play_pause", play)).await;
    assert_eq!(result_of(&mut client).await, JsonObject::new());
    assert_eq!(
        daemon.media.sent(),
        [(bus_name.to_owned(), MediaCommand::PlayPause)]
    );

    client
        .send(&request(3, "media.next", json!({"player_id": player_id})))
        .await;
    assert_eq!(error_of(&mut client).await["code"], "unsupported_target");
    client
        .send(&request(4, "media.next", json!({"player_id": 99})))
        .await;
    assert_eq!(error_of(&mut client).await["code"], "not_found");
    assert!(audit_lines(&daemon).is_empty(), "media is tier 1");
}

#[tokio::test]
async fn audio_refuses_a_volume_above_the_limit_first_and_then_an_unknown_device() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let (mut client, _) = daemon.ready(&device).await;

    let too_loud = json!({"device_id": 1, "volume_pct": 101});
    client.send(&request(1, "audio.set_volume", too_loud)).await;
    let error = error_of(&mut client).await;
    assert_eq!(error["code"], "bad_request");
    assert_eq!(error["message"], "volume_pct above max_volume_pct");

    let mute = json!({"device_id": 1, "muted": true});
    client.send(&request(2, "audio.set_mute", mute)).await;
    assert_eq!(error_of(&mut client).await["code"], "not_found");
    let writes = daemon.services_runner.argvs();
    assert!(
        writes
            .iter()
            .all(|argv| !argv.iter().any(|word| word.starts_with("set-"))),
        "no pactl write: {writes:?}"
    );
}

#[tokio::test]
async fn a_launch_runs_systemd_run_and_a_risky_entry_is_refused_before_the_workspace_switch() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let (mut client, _) = daemon.ready(&device).await;

    client
        .send(&request(1, "app.launch", json!({"entry_id": "notes"})))
        .await;
    assert_eq!(result_of(&mut client).await, JsonObject::new());
    let started = daemon.services_runner.argvs();
    let launch = started
        .iter()
        .find(|argv| argv[0] == "/fake/systemd-run")
        .expect("systemd-run started");
    let tail: Vec<&str> = launch
        .iter()
        .map(String::as_str)
        .skip_while(|w| *w != "--")
        .collect();
    assert_eq!(tail, ["--", "/opt/notes", "--new"]);

    let risky = json!({"entry_id": "term", "number": 3});
    client.send(&request(2, "app.launch", risky)).await;
    let error = error_of(&mut client).await;
    assert_eq!(error["code"], "forbidden");
    assert_eq!(error["detail"], json!({"scope": "launch.risky"}));
    assert_eq!(daemon.services_runner.argvs().len(), started.len());

    client
        .send(&request(3, "app.launch", json!({"entry_id": "gone"})))
        .await;
    assert_eq!(error_of(&mut client).await["code"], "not_found");

    let lines = audit_lines(&daemon);
    let results: Vec<(&Value, &Value)> = lines
        .iter()
        .map(|line| (&line["op"], &line["result"]))
        .collect();
    assert_eq!(
        results,
        [
            (&json!("app.launch"), &json!("ok")),
            (&json!("app.launch"), &json!("forbidden")),
            (&json!("app.launch"), &json!("not_found")),
        ]
    );
}

#[tokio::test]
async fn launcher_open_without_a_focus_read_starts_nothing() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let (mut client, _) = daemon.ready(&device).await;

    client.send(&request(1, "launcher.open", json!({}))).await;

    assert_eq!(error_of(&mut client).await["code"], "sway_unavailable");
    let started = daemon.services_runner.argvs();
    assert!(
        started.iter().all(|argv| argv[0] != "/fake/fuzzel"),
        "{started:?}"
    );
    assert_eq!(audit_lines(&daemon)[0]["op"], "launcher.open");
}

#[tokio::test]
async fn input_checks_the_target_window_first_and_the_audit_holds_only_the_length() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&ALL_M2B).await;
    let (mut client, _) = daemon.ready(&device).await;
    let secret = "hunter2 is my password";

    let unknown = json!({"window_id": 999, "text": secret});
    client.send(&request(1, "input.text", unknown)).await;
    let error = error_of(&mut client).await;
    assert_eq!(error["code"], "not_found");
    assert_eq!(error["message"], "no window with id 999");

    // Window 6 is in the state; the guard's focus read finds no sway.
    let known = json!({"window_id": 6, "text": secret});
    client.send(&request(2, "input.text", known)).await;
    assert_eq!(error_of(&mut client).await["code"], "sway_unavailable");
    let key = json!({"window_id": 6, "key": "Return"});
    client.send(&request(3, "input.key", key)).await;
    assert_eq!(error_of(&mut client).await["code"], "sway_unavailable");

    assert!(daemon.keyboard.deliveries().is_empty());
    let lines = audit_lines(&daemon);
    let ops: Vec<&Value> = lines.iter().map(|line| &line["op"]).collect();
    assert_eq!(
        ops,
        [
            &json!("input.text"),
            &json!("input.text"),
            &json!("input.key")
        ]
    );
    assert_eq!(lines[0]["text_len"], secret.len());
    assert_eq!(lines[1]["targets"], json!([6]));
    assert!(!audit_text(&daemon).contains("hunter2"));
}

#[tokio::test]
async fn a_launcher_opened_by_another_device_shows_in_session_as_held_by_it() {
    let daemon = Daemon::start().await;
    let watcher = daemon.paired_device(&ALL_M2B).await;
    let (mut watching, _) = daemon.ready(&watcher).await;
    watching.send(&subscribe_to(1, &["session"])).await;
    result_of(&mut watching).await;

    daemon
        .shared
        .services
        .launcher
        .open(
            &DeviceId::parse("AAAAAAAAAAAAAAAAAAAAAA").expect("id"),
            Some(6),
        )
        .expect("launcher started");

    let (rev, state) = state_where(&mut watching, "session", |state| {
        state["launcher"]["open"] == true
    })
    .await;
    assert!(rev >= 2);
    assert_eq!(state["launcher"]["holder"], "other_device");
}
