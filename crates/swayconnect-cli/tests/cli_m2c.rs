//! The M2c subcommands of `swayconnect` (`docs/M2C_PLAN.md` slice G-CLI), run in this
//! process against the scripted server of the client tests: the list commands of topics
//! `audio`, `media` and `apps`, `audio default`, `media next` and `previous`,
//! `launcher open`, and the `--launcher` target of `type` and `key`.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;
mod m2_script;

use m2_script::{
    failed, ok_with, request, run_recorded, script_answering, script_offering, subscribe_request,
    subscribed,
};
use serde_json::{Value, json};
use swayconnect_protocol::text::is_control_or_format;

const HOSTILE: &str = "Mic\u{1b}[2J\u{202e}";

fn audio_state() -> Value {
    let device = |id: u64, kind: &str, description: &str| {
        json!({
            "id": id, "description": description, "kind": kind,
            "volume_pct": 40, "muted": false, "available": true,
        })
    };
    json!({
        "available": true, "max_volume_pct": 150, "default_sink": 5, "default_source": null,
        "sinks": [device(5, "speaker", "Speakers")],
        "sources": [device(7, "microphone", HOSTILE)],
        "truncated": false,
    })
}

fn media_state() -> Value {
    let player = json!({
        "id": 4, "name": "mpv", "app_id": null, "icon": null, "status": "playing",
        "title": "Song", "artist": null, "album": null,
        "can_play": true, "can_pause": true, "can_next": true, "can_previous": true,
    });
    json!({"players": [player], "active_player_id": 4, "truncated": false})
}

fn apps_state() -> Value {
    let app = json!({
        "entry_id": "foot", "name": "Foot", "generic_name": null, "keywords": [],
        "icon": null, "terminal": true, "risky": true,
    });
    json!({"apps": [app], "recent": [], "truncated": true})
}

#[tokio::test]
async fn an_action_with_its_target_sends_one_request_and_prints_ok() {
    for (arguments, op, args) in [
        (
            &["audio", "default", "7"][..],
            "audio.set_default",
            json!({"device_id": 7}),
        ),
        (
            &["media", "next", "--player", "2"],
            "media.next",
            json!({"player_id": 2}),
        ),
        (
            &["media", "previous", "--player", "2"],
            "media.previous",
            json!({"player_id": 2}),
        ),
        (
            &["key", "Escape", "--launcher"],
            "input.key",
            json!({"launcher": true, "key": "Escape"}),
        ),
    ] {
        let script = script_answering(&[ok_with(1, &json!({}))]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        outcome.result.expect("command succeeds");
        assert_eq!(outcome.output, "ok\n", "{arguments:?}");
        assert_eq!(sent, [request(1, op, &args)], "{arguments:?}");
    }
}

#[tokio::test]
async fn a_skip_without_a_player_goes_to_the_active_one() {
    for (word, op) in [("next", "media.next"), ("previous", "media.previous")] {
        let script =
            script_answering(&[subscribed("media", &media_state()), ok_with(2, &json!({}))]);

        let (outcome, sent) = run_recorded(&["media", word], "", script).await;

        outcome.result.expect("command succeeds");
        assert_eq!(outcome.output, "ok\n");
        let expected = [
            subscribe_request("media"),
            request(2, op, &json!({"player_id": 4})),
        ];
        assert_eq!(sent, expected, "{word}");
    }
}

#[tokio::test]
async fn launcher_open_says_whether_this_command_opened_it() {
    for (already_open, text) in [
        (false, "opened: type into it with --launcher\n"),
        (
            true,
            "already open: type into it with --launcher only if this device opened it\n",
        ),
    ] {
        let result = json!({"already_open": already_open});
        let script = script_answering(&[ok_with(1, &result)]);

        let (outcome, sent) = run_recorded(&["launcher", "open"], "", script).await;

        outcome.result.expect("opened");
        assert_eq!(outcome.output, text);
        // `launcher.open` has no args, and the client sends no `args` member.
        let open = json!({"type": "req", "id": 1, "op": "launcher.open"});
        assert_eq!(sent, [open]);
    }

    let script = script_answering(&[ok_with(1, &json!({"already_open": true}))]);
    let (outcome, _sent) = run_recorded(&["--json", "launcher", "open"], "", script).await;
    outcome.result.expect("opened");
    assert_eq!(outcome.output, "{\"already_open\":true}\n");

    let script = script_answering(&[ok_with(1, &json!({}))]);
    let (outcome, _sent) = run_recorded(&["launcher", "open"], "", script).await;
    let failure = outcome.result.expect_err("invalid result");
    assert_eq!(failure.exit_code(), 3, "{failure}");
}

#[tokio::test]
async fn type_into_the_launcher_sends_the_launcher_target() {
    let script = script_answering(&[ok_with(1, &json!({"delivered_chars": 4}))]);

    let (outcome, sent) = run_recorded(&["type", "--launcher"], "foot\n", script).await;

    outcome.result.expect("typed");
    assert_eq!(outcome.output, "typed 4 characters\n");
    let args = json!({"launcher": true, "text": "foot"});
    assert_eq!(sent, [request(1, "input.text", &args)]);
}

#[tokio::test]
async fn a_closed_launcher_stops_typing_with_the_count() {
    let detail = json!({"delivered_chars": 0});
    let script = script_answering(&[failed(1, "launcher_closed", Some(detail))]);

    let (outcome, _sent) = run_recorded(&["type", "--launcher"], "foot", script).await;

    let failure = outcome.result.expect_err("stopped");
    assert_eq!(failure.exit_code(), 1);
    assert_eq!(
        failure.to_string(),
        "typing stopped: launcher_closed: no (0 characters typed)"
    );
}

#[tokio::test]
async fn a_list_command_prints_its_topic_as_a_table_with_safe_text() {
    for (arguments, topic, data, expected) in [
        (
            &["audio", "devices"][..],
            "audio",
            audio_state(),
            "\
ID  ROLE    KIND        VOLUME  FLAGS    DESCRIPTION
5   sink    speaker     40%     default  Speakers
7   source  microphone  40%     -        Mic\u{fffd}[2J\u{fffd}
",
        ),
        (
            &["media", "players"],
            "media",
            media_state(),
            "\
ID  STATUS   FLAGS   CAN                       NAME  TRACK
4   playing  active  play,pause,next,previous  mpv   Song
",
        ),
        (
            &["apps"],
            "apps",
            apps_state(),
            "\
ENTRY  FLAGS           NAME
foot   risky,terminal  Foot
note: the server cut the lists short
",
        ),
    ] {
        let script = script_answering(&[subscribed(topic, &data)]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        outcome.result.expect("listed");
        assert_eq!(outcome.output, expected, "{arguments:?}");
        assert_eq!(sent, [subscribe_request(topic)], "{arguments:?}");
    }
}

#[tokio::test]
async fn a_list_command_with_json_prints_one_escaped_value_per_row() {
    let script = script_answering(&[subscribed("audio", &audio_state())]);

    let (outcome, _sent) = run_recorded(&["--json", "audio", "devices"], "", script).await;

    outcome.result.expect("listed");
    for line in outcome.output.lines() {
        assert!(!line.contains(is_control_or_format), "{line:?}");
    }
    let rows: Vec<Value> = outcome
        .output
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON value per line"))
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["role"], "sink");
    assert_eq!(rows[0]["default"], true);
    assert_eq!(rows[1]["role"], "source");
    assert_eq!(
        rows[1]["description"], HOSTILE,
        "the value survives the escape"
    );
}

#[tokio::test]
async fn a_refused_or_missing_topic_is_an_error() {
    let script = script_answering(&[failed(1, "forbidden", None)]);
    let (outcome, _sent) = run_recorded(&["apps"], "", script).await;
    let failure = outcome.result.expect_err("forbidden");
    assert_eq!(failure.exit_code(), 1, "{failure}");

    let script = script_answering(&[ok_with(1, &json!({"states": []}))]);
    let (outcome, _sent) = run_recorded(&["media", "players"], "", script).await;
    let failure = outcome.result.expect_err("no state");
    assert_eq!(failure.exit_code(), 3, "{failure}");
    assert_eq!(
        failure.to_string(),
        "the server sent no state of topic `media`"
    );
}

#[tokio::test]
async fn an_action_the_server_does_not_offer_is_not_sent() {
    for arguments in [
        &["audio", "default", "7"][..],
        &["media", "next", "--player", "2"],
        &["media", "previous", "--player", "2"],
        &["launcher", "open"],
        &["key", "Escape", "--launcher"],
    ] {
        let script = script_offering(&["ping", "state.subscribe"]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        let refused = outcome.result.expect_err("not offered");
        assert_eq!(refused.exit_code(), 1, "{refused}");
        assert!(sent.is_empty(), "{arguments:?} sent {sent:?}");
    }
}
