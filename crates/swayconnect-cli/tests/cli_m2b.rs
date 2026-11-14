//! The M2b subcommands of `swayconnect` (`docs/V2_PLAN.md` slice CLI.2b), run in this
//! process against the scripted server of the client tests: the requests they send, the
//! states they read for a default target, what they print, and how `type` reads stdin.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;
mod m2_script;

use clap::Parser as _;
use m2_script::{
    failed, ok_with, request, run_offline, run_recorded, script_answering, script_offering,
    subscribe_request, subscribed,
};
use serde_json::{Value, json};
use swayconnect_cli::cli::{Cli, Context, ProfileStore, run};
use swayconnect_cli::client::Connector;

fn audio_state(available: bool, default_sink: Option<u64>) -> Value {
    let sink = json!({
        "id": 5, "description": "Speakers", "kind": "speaker",
        "volume_pct": 40, "muted": false, "available": true,
    });
    let sinks = if available { json!([sink]) } else { json!([]) };
    json!({
        "available": available, "max_volume_pct": 150, "default_sink": default_sink,
        "default_source": null, "sinks": sinks, "sources": [], "truncated": false,
    })
}

fn media_state(active_player_id: Option<u64>) -> Value {
    media_state_with("paused", active_player_id)
}

/// One player, id 4, with `status`.
fn media_state_with(status: &str, active_player_id: Option<u64>) -> Value {
    let player = json!({
        "id": 4, "name": "Player", "app_id": null, "icon": null, "status": status,
        "title": null, "artist": null, "album": null,
        "can_play": true, "can_pause": true, "can_next": false, "can_previous": false,
    });
    json!({"players": [player], "active_player_id": active_player_id, "truncated": false})
}

fn apps_state() -> Value {
    let app = |entry_id: &str, terminal: bool| {
        json!({
            "entry_id": entry_id, "name": entry_id, "generic_name": null, "keywords": [],
            "icon": null, "terminal": terminal, "risky": terminal,
        })
    };
    let apps = [app("notes.desktop", false), app("term.desktop", true)];
    json!({"apps": apps, "recent": [], "truncated": false})
}

#[tokio::test]
async fn a_command_with_its_target_sends_one_request_and_prints_the_answer() {
    let empty = json!({});
    for (arguments, op, args) in [
        (
            &["volume", "30", "--device", "3"][..],
            "audio.set_volume",
            json!({"device_id": 3, "volume_pct": 30}),
        ),
        (
            &["mute", "on", "--device", "3"],
            "audio.set_mute",
            json!({"device_id": 3, "muted": true}),
        ),
        (
            &["play", "--player", "2"],
            "media.play_pause",
            json!({"player_id": 2}),
        ),
        (
            &["key", "Return", "--window", "6"],
            "input.key",
            json!({"window_id": 6, "key": "Return"}),
        ),
        (
            &[
                "key",
                "t",
                "--mod",
                "ctrl,shift",
                "--repeat",
                "2",
                "--window",
                "6",
            ],
            "input.key",
            json!({"window_id": 6, "key": "t", "modifiers": ["ctrl", "shift"], "repeat": 2}),
        ),
    ] {
        let script = script_answering(&[ok_with(1, &empty)]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        outcome.result.expect("command succeeds");
        assert_eq!(outcome.output, "ok\n", "{arguments:?}");
        assert_eq!(sent, [request(1, op, &args)], "{arguments:?}");
    }
}

#[tokio::test]
async fn without_a_target_the_default_sink_or_active_player_is_read_from_the_state() {
    let empty = json!({});
    for (arguments, topic, data, op, args) in [
        (
            &["volume", "30"][..],
            "audio",
            audio_state(true, Some(5)),
            "audio.set_volume",
            json!({"device_id": 5, "volume_pct": 30}),
        ),
        (
            &["mute", "off"],
            "audio",
            audio_state(true, Some(5)),
            "audio.set_mute",
            json!({"device_id": 5, "muted": false}),
        ),
        (
            &["play"],
            "media",
            media_state(Some(4)),
            "media.play_pause",
            json!({"player_id": 4}),
        ),
        // No player has played yet: the first paused one (PROTOCOL 17.10).
        (
            &["play"],
            "media",
            media_state(None),
            "media.play_pause",
            json!({"player_id": 4}),
        ),
    ] {
        let script = script_answering(&[subscribed(topic, &data), ok_with(2, &empty)]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        outcome.result.expect("command succeeds");
        assert_eq!(outcome.output, "ok\n", "{arguments:?}");
        let expected = [subscribe_request(topic), request(2, op, &args)];
        assert_eq!(sent, expected, "{arguments:?}");
    }
}

#[tokio::test]
async fn no_default_target_in_the_state_exits_1_and_sends_no_action() {
    for (arguments, topic, data, message) in [
        (
            &["volume", "30"][..],
            "audio",
            audio_state(true, None),
            "no default sink: choose a device with --device",
        ),
        (
            &["mute", "on"],
            "audio",
            audio_state(false, None),
            "no default sink: the sound server cannot be reached",
        ),
        (
            &["play"],
            "media",
            media_state_with("stopped", None),
            "no active media player: choose one with --player",
        ),
    ] {
        let script = script_answering(&[subscribed(topic, &data)]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        let failure = outcome.result.expect_err("no target");
        assert_eq!(failure.exit_code(), 1, "{failure}");
        assert_eq!(failure.to_string(), message);
        assert_eq!(sent, [subscribe_request(topic)], "{arguments:?}");
        assert_eq!(outcome.output, "");
    }
}

#[tokio::test]
async fn an_invalid_state_exits_3() {
    let script = script_answering(&[subscribed("audio", &json!({"available": true}))]);

    let (outcome, _sent) = run_recorded(&["volume", "30"], "", script).await;

    let failure = outcome.result.expect_err("invalid state");
    assert_eq!(failure.exit_code(), 3, "{failure}");
    assert!(
        failure.to_string().contains("invalid `audio` state"),
        "{failure}"
    );
}

#[tokio::test]
async fn launch_finds_the_entry_in_the_apps_state_and_asks_before_a_risky_one() {
    let started = [
        (
            &["launch", "notes.desktop", "--workspace", "3"][..],
            json!({"entry_id": "notes.desktop", "number": 3}),
        ),
        (
            &["launch", "term.desktop", "--yes"],
            json!({"entry_id": "term.desktop"}),
        ),
    ];
    for (arguments, args) in started {
        let script = script_answering(&[subscribed("apps", &apps_state()), ok_with(2, &json!({}))]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        outcome.result.expect("launch succeeds");
        assert_eq!(outcome.output, "started\n", "{arguments:?}");
        let expected = [subscribe_request("apps"), request(2, "app.launch", &args)];
        assert_eq!(sent, expected, "{arguments:?}");
    }

    let refused: [(&[&str], u8, &str); 2] = [
        (&["launch", "term.desktop"], 2, "--yes"),
        (
            &["launch", "gone.desktop"],
            1,
            "the server lists no app gone.desktop",
        ),
    ];
    for (arguments, exit_code, message) in refused {
        let script = script_answering(&[subscribed("apps", &apps_state())]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        let failure = outcome.result.expect_err("not launched");
        assert_eq!(failure.exit_code(), exit_code, "{failure}");
        assert!(failure.to_string().contains(message), "{failure}");
        assert_eq!(sent, [subscribe_request("apps")], "{arguments:?}");
    }
}

#[tokio::test]
async fn type_sends_the_stdin_text_without_its_final_newline() {
    let script = script_answering(&[ok_with(1, &json!({"delivered_chars": 7}))]);

    let (outcome, sent) = run_recorded(&["type", "--window", "6"], "привет!\n", script).await;

    outcome.result.expect("typed");
    assert_eq!(outcome.output, "typed 7 characters\n");
    let args = json!({"window_id": 6, "text": "привет!"});
    assert_eq!(sent, [request(1, "input.text", &args)]);
}

#[tokio::test]
async fn longer_text_goes_in_requests_of_1024_bytes_and_the_counts_add_up() {
    let text = "a".repeat(1500);
    let script = script_answering(&[
        ok_with(1, &json!({"delivered_chars": 1024})),
        ok_with(2, &json!({"delivered_chars": 476})),
    ]);

    let (outcome, sent) = run_recorded(&["--json", "type", "--window", "6"], &text, script).await;

    outcome.result.expect("typed");
    assert_eq!(outcome.output, "{\"delivered_chars\":1500}\n");
    let first = json!({"window_id": 6, "text": &text[..1024]});
    let second = json!({"window_id": 6, "text": &text[1024..]});
    assert_eq!(
        sent,
        [
            request(1, "input.text", &first),
            request(2, "input.text", &second)
        ]
    );
}

#[tokio::test]
async fn a_refusal_stops_typing_and_reports_every_typed_character() {
    let text = "b".repeat(1100);
    let detail = json!({"delivered_chars": 16, "focused_window_id": 7});
    let script = script_answering(&[
        ok_with(1, &json!({"delivered_chars": 1024})),
        failed(2, "focus_changed", Some(detail)),
    ]);

    let (outcome, sent) = run_recorded(&["type", "--window", "6"], &text, script).await;

    let failure = outcome.result.expect_err("stopped");
    assert_eq!(failure.exit_code(), 1);
    assert_eq!(
        failure.to_string(),
        "typing stopped: focus_changed: no (1040 characters typed; window 7 has the focus now)"
    );
    assert_eq!(sent.len(), 2);
    assert_eq!(outcome.output, "");
}

#[tokio::test]
async fn rate_limited_text_goes_again_after_the_delay() {
    let script = script_answering(&[
        failed(1, "rate_limited", Some(json!({"retry_after_ms": 20}))),
        ok_with(2, &json!({"delivered_chars": 2})),
    ]);

    let (outcome, sent) = run_recorded(&["type", "--window", "6"], "hi", script).await;

    outcome.result.expect("typed after the delay");
    assert_eq!(outcome.output, "typed 2 characters\n");
    let args = json!({"window_id": 6, "text": "hi"});
    assert_eq!(
        sent,
        [
            request(1, "input.text", &args),
            request(2, "input.text", &args)
        ]
    );
}

#[tokio::test]
async fn a_delay_longer_than_the_timeout_is_not_waited_for() {
    let detail = json!({"retry_after_ms": 50_000});
    let script = script_answering(&[failed(1, "rate_limited", Some(detail))]);

    let (outcome, sent) = run_recorded(&["type", "--window", "6"], "hi", script).await;

    let failure = outcome.result.expect_err("rate limited");
    assert_eq!(failure.exit_code(), 1);
    assert!(
        failure.to_string().contains("retry after 50000 ms"),
        "{failure}"
    );
    assert_eq!(sent.len(), 1);
}

#[tokio::test]
async fn type_refuses_text_in_arguments_and_bad_stdin_before_connecting() {
    let too_long = "a".repeat(4097);
    for (arguments, stdin, message) in [
        (
            &["type", "--window", "6", "hunter2"][..],
            "",
            "read from stdin",
        ),
        (&["type", "--window", "6"], "", "no text"),
        (&["type", "--window", "6"], "\n", "no text"),
        (&["type", "--window", "6"], "ls\nrm", "send Enter with"),
        (&["type", "--window", "6"], "a\tb", "control or format"),
        (
            &["type", "--window", "6"],
            &too_long,
            "longer than 4096 bytes",
        ),
        (
            &["key", "F2", "--mod", "ctrl,alt", "--window", "6"],
            "",
            "virtual terminal",
        ),
        (
            &["key", "a", "--mod", "ctrl,ctrl", "--window", "6"],
            "",
            "distinct",
        ),
    ] {
        let failure = run_offline(arguments, stdin).await;

        assert_eq!(failure.exit_code(), 2, "{arguments:?}: {failure}");
        let text = failure.to_string();
        assert!(text.contains(message), "{arguments:?}: {text}");
        assert!(!text.contains("hunter2"), "{text}");
    }
}

#[tokio::test]
async fn stdin_that_is_not_utf8_is_refused() {
    let client_dir = tempfile::tempdir().expect("temp dir");
    let cli = Cli::try_parse_from(["swayconnect", "type", "--window", "6"]).expect("arguments");
    let mut output = Vec::new();
    let context = Context {
        connector: Connector::new(None),
        store: ProfileStore::new(client_dir.path().to_owned()),
        input: &mut &b"caf\xe9"[..],
        output: &mut output,
    };

    let failure = run(&cli, context).await.expect_err("not UTF-8");

    assert_eq!(failure.exit_code(), 2);
    assert_eq!(failure.to_string(), "the text is not UTF-8");
}

#[tokio::test]
async fn a_command_the_server_does_not_offer_is_not_sent() {
    for arguments in [
        &["volume", "30", "--device", "3"][..],
        &["mute", "on", "--device", "3"],
        &["play", "--player", "2"],
        &["key", "a", "--window", "6"],
    ] {
        let script = script_offering(&["ping", "state.subscribe"]);

        let (outcome, sent) = run_recorded(arguments, "", script).await;

        let refused = outcome.result.expect_err("not offered");
        assert_eq!(refused.exit_code(), 1, "{refused}");
        assert!(sent.is_empty(), "{arguments:?} sent {sent:?}");
    }
}
