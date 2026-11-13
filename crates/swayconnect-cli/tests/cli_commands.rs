//! The commands of `swayconnect`, run in this process against the scripted server of the
//! client tests. The server here breaks its guarantee and sends control and format
//! characters in a state (spec 9.3 rule 4): no command may pass them on.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;

use std::future::Future as _;
use std::pin::pin;
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::Duration;

use clap::Parser as _;
use client_support::{
    Server, Step, auth_connector, good_config, profile, transcript, vector_fp, with_member,
};
use serde_json::{Value, json};
use swayconnect_cli::cli::{Cli, CliError, Context, ProfileStore, run};
use swayconnect_cli::client::Connector;
use swayconnect_protocol::text::is_control_or_format;
use tempfile::TempDir;
use tokio::net::TcpListener;

const HOSTILE: &str = "a\u{1b}[2Jb\u{9b}c\u{202e}d";
const PATIENCE: Duration = Duration::from_secs(20);
const UNSAFE_CHARACTERS: [char; 3] = ['\u{1b}', '\u{9b}', '\u{202e}'];

fn hostile_state(rev: u64) -> Value {
    json!({"topic": "desktop", "rev": rev, "data": {
        "sway": {"online": true, "version": HOSTILE},
        "workspaces": [{
            "id": 4, "number": 1, "name": HOSTILE, "output": HOSTILE,
            "focused": true, "visible": true, "urgent": false,
        }],
        "windows": [{
            "id": 9, "workspace_id": 4, "app_id": HOSTILE, "title": HOSTILE,
            "focused": true, "urgent": false, "floating": false, "fullscreen": false,
        }],
        "truncated": false,
    }})
}

/// Transcript T2: the server side of a login that succeeds.
fn login_script() -> Vec<Step> {
    let t2 = transcript("t2-auth-ok");
    vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(t2[2].clone()),
    ]
}

/// A login, then `response` as the answer to the first request.
fn answer_script(response: &Value) -> Vec<Step> {
    let mut script = login_script();
    script.extend([Step::Receive, Step::Send(response.to_string())]);
    script
}

/// A login, then the hostile state as the answer to `state.subscribe`.
fn hostile_script() -> Vec<Step> {
    let states = json!({"states": [hostile_state(7)]});
    answer_script(&json!({"type": "res", "id": 1, "ok": true, "result": states}))
}

/// The hostile script, then one more hostile state as an event, then `bye`: the scripted
/// server never closes by itself, and `watch` ends only with the session.
fn hostile_watch_script() -> Vec<Step> {
    let event = json!({"type": "evt", "event": "state.changed", "data": hostile_state(8)});
    let bye = json!({"type": "bye", "code": "shutdown", "message": "scripted end"});
    let mut script = hostile_script();
    script.extend([Step::Send(event.to_string()), Step::Send(bye.to_string())]);
    script
}

/// What one run of the binary's `run` gave: its result and its stdout.
struct Outcome {
    result: Result<(), CliError>,
    output: String,
}

async fn run_command(arguments: &[&str], connector: Connector, client_dir: &TempDir) -> Outcome {
    let cli = Cli::try_parse_from([&["swayconnect"], arguments].concat()).expect("arguments");
    let mut output = Vec::new();
    let context = Context {
        connector,
        store: ProfileStore::new(client_dir.path().to_owned()),
        input: &mut std::io::empty(),
        output: &mut output,
    };
    // A command that waits for a frame the script never sends fails here, not by hanging.
    let result = tokio::time::timeout(PATIENCE, run(&cli, context))
        .await
        .expect("the command ends");
    Outcome {
        result,
        output: String::from_utf8(output).expect("UTF-8 output"),
    }
}

/// A server that plays `script`, and a client directory with the profile for it.
async fn server_with_profile(script: Vec<Step>) -> (Server, TempDir) {
    let server = Server::start(good_config(), script).await;
    let client_dir = tempfile::tempdir().expect("temp dir");
    ProfileStore::new(client_dir.path().to_owned())
        .save(&profile(server.port, vector_fp()))
        .expect("profile saved");
    (server, client_dir)
}

/// Runs one command against a server that plays `script`, with a stored profile for it.
async fn run_against(arguments: &[&str], script: Vec<Step>) -> Outcome {
    let (_server, client_dir) = server_with_profile(script).await;
    run_command(arguments, auth_connector(), &client_dir).await
}

/// As `run_against`, and returns the frames the client sent after `auth`.
async fn run_recorded(arguments: &[&str], script: Vec<Step>) -> (Outcome, Vec<Value>) {
    let (server, client_dir) = server_with_profile(script).await;
    let outcome = run_command(arguments, auth_connector(), &client_dir).await;
    let mut frames = server.received().await.expect("server script").frames;
    frames.remove(0);
    (outcome, frames)
}

fn ok_response() -> Value {
    json!({"type": "res", "id": 1, "ok": true, "result": {}})
}

fn error_response(error: &Value) -> Value {
    json!({"type": "res", "id": 1, "ok": false, "error": error})
}

fn assert_clean(output: &str, arguments: &[&str]) {
    assert!(!output.is_empty(), "{arguments:?} printed nothing");
    for unsafe_character in UNSAFE_CHARACTERS {
        assert!(!output.contains(unsafe_character), "{arguments:?}");
    }
    let has_other_unsafe_text = output
        .lines()
        .any(|line| line.contains(is_control_or_format));
    assert!(!has_other_unsafe_text, "{arguments:?}: {output:?}");
}

#[tokio::test]
async fn a_hostile_state_reaches_no_table_and_no_json_output() {
    for arguments in [
        &["workspaces"][..],
        &["windows"],
        &["state"],
        &["workspaces", "--json"],
        &["windows", "--json"],
        &["state", "--json"],
    ] {
        let outcome = run_against(arguments, hostile_script()).await;

        outcome.result.expect("command succeeds");
        assert_clean(&outcome.output, arguments);
        assert!(outcome.output.contains("a\u{fffd}[2Jb\u{fffd}c\u{fffd}d"));
    }
}

#[tokio::test]
async fn json_output_is_one_value_per_line() {
    for (arguments, script, expected_lines) in [
        (&["state", "--json"][..], hostile_script(), 1),
        (&["workspaces", "--json"], hostile_script(), 1),
        (&["windows", "--json"], hostile_script(), 1),
        (&["info", "--json"], login_script(), 1),
        (&["watch", "--json"], hostile_watch_script(), 2),
    ] {
        let outcome = run_against(arguments, script).await;

        let lines: Vec<&str> = outcome.output.lines().collect();
        assert_eq!(lines.len(), expected_lines, "{arguments:?}");
        for line in lines {
            serde_json::from_str::<Value>(line).expect("one JSON value");
        }
    }
}

#[tokio::test]
async fn watch_prints_a_clean_line_per_state_and_ends_with_the_session() {
    for arguments in [&["watch"][..], &["watch", "--json"]] {
        let outcome = run_against(arguments, hostile_watch_script()).await;

        let ended = outcome.result.expect_err("the scripted server says bye");
        assert_eq!(ended.exit_code(), 3, "{ended}");
        assert_clean(&outcome.output, arguments);
        assert_eq!(outcome.output.lines().count(), 2, "{}", outcome.output);
    }
}

#[tokio::test]
async fn a_server_error_exits_1_and_prints_its_code_with_escaped_text() {
    let error = json!({"code": "forbidden", "message": "no \u{1b}[31mscope"});
    let response = json!({"type": "res", "id": 1, "ok": false, "error": error});

    let outcome = run_against(&["windows"], answer_script(&response)).await;

    let failure = outcome.result.expect_err("server error");
    assert_eq!(failure.exit_code(), 1);
    let text = failure.to_string();
    assert!(text.contains("forbidden"), "{text}");
    assert!(text.contains("\\u{1b}[31mscope"), "{text}");
    assert!(!text.contains('\u{1b}'));
    assert_eq!(outcome.output, "");
}

#[tokio::test]
async fn pair_with_a_uri_argument_exits_2_and_connects_to_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local address").port();
    let secret = "c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2U";
    let uri = format!("swayconnect://pair?v=1&host=127.0.0.1&port={port}&fp={secret}&s={secret}");
    let client_dir = tempfile::tempdir().expect("temp dir");

    let outcome = run_command(&["pair", &uri], Connector::new(None), &client_dir).await;

    let refused = outcome.result.expect_err("URI argument");
    assert_eq!(refused.exit_code(), 2);
    assert!(!refused.to_string().contains(secret));
    assert_eq!(outcome.output, "");
    // A connection to a listening loopback socket is complete when `connect` returns, so one
    // poll of `accept` sees it if the command made one.
    let accept = pin!(listener.accept());
    let polled = accept.poll(&mut TaskContext::from_waker(Waker::noop()));
    assert!(matches!(polled, Poll::Pending), "the command connected");
    assert!(
        std::fs::read_dir(client_dir.path())
            .expect("client dir")
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn a_command_without_a_profile_exits_2() {
    let client_dir = tempfile::tempdir().expect("temp dir");

    for arguments in [
        &["info"][..],
        &["state"],
        &["workspaces"],
        &["windows"],
        &["watch"],
        &["workspace", "3"],
        &["focus", "9"],
        &["fullscreen", "9", "on"],
        &["raw", "{}"],
    ] {
        let outcome = run_command(arguments, Connector::new(None), &client_dir).await;

        let refused = outcome.result.expect_err("no profile");
        assert_eq!(refused.exit_code(), 2, "{arguments:?}: {refused}");
        assert_eq!(outcome.output, "");
    }
}

#[tokio::test]
async fn every_action_sends_its_one_request_and_prints_ok() {
    for (arguments, op, args) in [
        (
            &["workspace", "3"][..],
            "workspace.focus",
            json!({"number": 3}),
        ),
        (
            &["workspace", "--id", "4"],
            "workspace.focus",
            json!({"workspace_id": 4}),
        ),
        (&["focus", "9"], "window.focus", json!({"window_id": 9})),
        (
            &["fullscreen", "9", "on"],
            "window.fullscreen",
            json!({"window_id": 9, "enabled": true}),
        ),
        (
            &["fullscreen", "9", "off"],
            "window.fullscreen",
            json!({"window_id": 9, "enabled": false}),
        ),
    ] {
        let (outcome, sent) = run_recorded(arguments, answer_script(&ok_response())).await;

        outcome.result.expect("action succeeds");
        assert_eq!(outcome.output, "ok\n", "{arguments:?}");
        let request = json!({"type": "req", "id": 1, "op": op, "args": args});
        assert_eq!(sent, [request], "{arguments:?}");
    }
}

#[tokio::test]
async fn an_action_with_json_prints_the_result_object() {
    let (outcome, _sent) =
        run_recorded(&["focus", "9", "--json"], answer_script(&ok_response())).await;

    outcome.result.expect("action succeeds");
    assert_eq!(outcome.output, "{}\n");
}

#[tokio::test]
async fn a_refused_action_exits_1_and_prints_the_code() {
    for code in [
        "forbidden",
        "read_only",
        "not_found",
        "unsupported_target",
        "sway_unavailable",
        "sway_error",
        "timeout",
    ] {
        let error = json!({"code": code, "message": "no \u{9b}31m"});
        let script = answer_script(&error_response(&error));

        let outcome = run_against(&["workspace", "--id", "4"], script).await;

        let failure = outcome.result.expect_err("server error");
        assert_eq!(failure.exit_code(), 1, "{code}");
        let text = failure.to_string();
        assert_eq!(text, format!("the request failed: {code}: no \\u{{9b}}31m"));
        assert_eq!(outcome.output, "");
    }
}

#[tokio::test]
async fn a_rate_limited_action_exits_1_and_prints_the_retry_delay() {
    let detail = json!({"retry_after_ms": 250});
    let error = json!({"code": "rate_limited", "message": "too many actions", "detail": detail});

    let outcome = run_against(&["focus", "9"], answer_script(&error_response(&error))).await;

    let failure = outcome.result.expect_err("rate limited");
    assert_eq!(failure.exit_code(), 1);
    assert_eq!(
        failure.to_string(),
        "the request failed: rate_limited: too many actions (retry after 250 ms)"
    );
    assert_eq!(outcome.output, "");
}

#[tokio::test]
async fn an_action_the_server_does_not_offer_is_not_sent() {
    for arguments in [
        &["workspace", "3"][..],
        &["focus", "9"],
        &["fullscreen", "9", "on"],
    ] {
        let welcome = &transcript("t2-auth-ok")[2];
        let read_only_welcome = with_member(welcome, "ops", json!(["ping", "state.subscribe"]));
        let mut script = login_script();
        script[2] = Step::Send(read_only_welcome);

        let (outcome, sent) = run_recorded(arguments, script).await;

        let refused = outcome.result.expect_err("not offered");
        assert_eq!(refused.exit_code(), 1, "{refused}");
        assert!(sent.is_empty(), "{arguments:?} sent {sent:?}");
        assert_eq!(outcome.output, "");
    }
}

/// Takes the 2 seconds of the reply window: the scripted server does not close.
#[tokio::test]
async fn raw_sends_the_text_as_one_frame_and_prints_every_reply_cleanly() {
    let payload = r#"{"type":"req","id":7,"op":"no.such.op"}"#;
    let error = json!({"code": "unknown_op", "message": HOSTILE});
    let response = json!({"type": "res", "id": 7, "ok": false, "error": error});
    let event = json!({"type": "evt", "event": "state.changed", "data": hostile_state(8)});
    let bye = json!({"type": "bye", "code": "protocol_error", "message": "scripted end"});
    let mut script = answer_script(&response);
    script.extend([Step::Send(event.to_string()), Step::Send(bye.to_string())]);
    let arguments = ["raw", payload];

    let (outcome, sent) = run_recorded(&arguments, script).await;

    outcome
        .result
        .expect("raw succeeds whatever the server answers");
    let sent_payload: Value = serde_json::from_str(payload).expect("payload");
    assert_eq!(sent, [sent_payload]);
    assert_clean(&outcome.output, &arguments);
    let replies: Vec<Value> = outcome
        .output
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON value per line"))
        .collect();
    assert_eq!(replies, [response, event, bye]);
}
