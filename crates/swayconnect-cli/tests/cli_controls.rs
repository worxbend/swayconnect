//! The M2a subcommands of `swayconnect` (`docs/V2_PLAN.md` slice CLI.2a), run in this
//! process against the scripted server of the client tests: the requests they send, what
//! they print, and how they treat a result that breaks spec 17.6 or 17.8.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;

use std::time::Duration;

use clap::Parser as _;
use client_support::{
    Server, Step, auth_connector, good_config, profile, transcript, vector_fp, with_member,
};
use serde_json::{Value, json};
use swayconnect_cli::cli::{Cli, CliError, Context, ProfileStore, run};
use swayconnect_cli::client::Connector;
use tempfile::TempDir;

const PATIENCE: Duration = Duration::from_secs(20);
const M2A_OPS: [&str; 7] = [
    "ping",
    "state.subscribe",
    "window.move",
    "window.floating",
    "window.close",
    "session.lock",
    "keep_awake.set",
];

struct Outcome {
    result: Result<(), CliError>,
    output: String,
}

/// Transcript T2 with `ops` set to `ops`, then `response` to the first request.
fn script_offering(ops: &[&str], response: Option<&Value>) -> Vec<Step> {
    let t2 = transcript("t2-auth-ok");
    let welcome = with_member(&t2[2], "ops", json!(ops));
    let mut script = vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(welcome),
    ];
    if let Some(response) = response {
        script.extend([Step::Receive, Step::Send(response.to_string())]);
    }
    script
}

fn ok_with(result: &Value) -> Value {
    json!({"type": "res", "id": 1, "ok": true, "result": result})
}

/// Runs `arguments` against a server that plays `script`; returns what the client sent
/// after `auth`.
async fn run_recorded(arguments: &[&str], script: Vec<Step>) -> (Outcome, Vec<Value>) {
    let server = Server::start(good_config(), script).await;
    let client_dir = tempfile::tempdir().expect("temp dir");
    ProfileStore::new(client_dir.path().to_owned())
        .save(&profile(server.port, vector_fp()))
        .expect("profile saved");
    let outcome = run_command(arguments, auth_connector(), &client_dir).await;
    let mut frames = server.received().await.expect("server script").frames;
    frames.remove(0);
    (outcome, frames)
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
    let result = tokio::time::timeout(PATIENCE, run(&cli, context))
        .await
        .expect("the command ends");
    Outcome {
        result,
        output: String::from_utf8(output).expect("UTF-8 output"),
    }
}

#[tokio::test]
async fn every_m2a_command_sends_its_one_request_and_prints_the_answer() {
    let empty = json!({});
    let locked = json!({"already_locked": false});
    let already = json!({"already_locked": true});
    let on = json!({"remaining_ms": 5_400_000});
    let off = json!({"remaining_ms": null});
    for (arguments, op, args, result, printed) in [
        (
            &["move", "6", "3"][..],
            "window.move",
            json!({"window_id": 6, "number": 3}),
            &empty,
            "ok\n",
        ),
        (
            &["move", "6", "--id", "5"],
            "window.move",
            json!({"window_id": 6, "workspace_id": 5}),
            &empty,
            "ok\n",
        ),
        (
            &["float", "6", "on"],
            "window.floating",
            json!({"window_id": 6, "enabled": true}),
            &empty,
            "ok\n",
        ),
        (
            &["float", "6", "off"],
            "window.floating",
            json!({"window_id": 6, "enabled": false}),
            &empty,
            "ok\n",
        ),
        (
            &["close", "6", "--yes"],
            "window.close",
            json!({"window_id": 6}),
            &empty,
            "ok\n",
        ),
        (
            &["lock"],
            "session.lock",
            Value::Null,
            &locked,
            "lock started\n",
        ),
        (
            &["lock"],
            "session.lock",
            Value::Null,
            &already,
            "already locked\n",
        ),
        (
            &["awake", "on"],
            "keep_awake.set",
            json!({"enabled": true}),
            &on,
            "keep awake on for 1h 30m\n",
        ),
        (
            &["awake", "on", "--for", "2h"],
            "keep_awake.set",
            json!({"enabled": true, "duration_s": 7200}),
            &on,
            "keep awake on for 1h 30m\n",
        ),
        (
            &["awake", "off"],
            "keep_awake.set",
            json!({"enabled": false}),
            &off,
            "keep awake off\n",
        ),
    ] {
        let script = script_offering(&M2A_OPS, Some(&ok_with(result)));

        let (outcome, sent) = run_recorded(arguments, script).await;

        outcome.result.expect("command succeeds");
        assert_eq!(outcome.output, printed, "{arguments:?}");
        let mut request = json!({"type": "req", "id": 1, "op": op});
        if !args.is_null() {
            request["args"] = args;
        }
        assert_eq!(sent, [request], "{arguments:?}");
    }
}

#[tokio::test]
async fn with_json_the_result_is_printed_as_one_clean_line() {
    for (arguments, result) in [
        (
            &["--json", "lock"][..],
            json!({"already_locked": true, "x": "\u{1b}[2J"}),
        ),
        (&["--json", "awake", "on"], json!({"remaining_ms": 60_000})),
        (&["--json", "awake", "off"], json!({"remaining_ms": null})),
    ] {
        let script = script_offering(&M2A_OPS, Some(&ok_with(&result)));

        let (outcome, _sent) = run_recorded(arguments, script).await;

        outcome.result.expect("command succeeds");
        assert!(!outcome.output.contains('\u{1b}'), "{arguments:?}");
        let printed: Value = serde_json::from_str(&outcome.output).expect("one JSON value");
        assert_eq!(printed, result, "{arguments:?}");
    }
}

#[tokio::test]
async fn a_result_that_breaks_the_spec_exits_3_and_prints_nothing() {
    for (arguments, result) in [
        (&["lock"][..], json!({})),
        (&["lock"], json!({"already_locked": "yes"})),
        (&["awake", "on"], json!({})),
        (&["awake", "on"], json!({"remaining_ms": -1})),
        (&["awake", "off"], json!({"remaining_ms": "soon"})),
    ] {
        let script = script_offering(&M2A_OPS, Some(&ok_with(&result)));

        let (outcome, _sent) = run_recorded(arguments, script).await;

        let failure = outcome.result.expect_err("invalid result");
        assert_eq!(failure.exit_code(), 3, "{arguments:?}: {failure}");
        assert!(failure.to_string().contains("invalid result"), "{failure}");
        assert_eq!(outcome.output, "", "{arguments:?}");
    }
}

#[tokio::test]
async fn limit_reached_exits_1_and_prints_the_retry_delay() {
    let detail = json!({"retry_after_ms": 900_000});
    let error = json!({"code": "limit_reached", "message": "cap", "detail": detail});
    let response = json!({"type": "res", "id": 1, "ok": false, "error": error});

    let (outcome, _sent) =
        run_recorded(&["awake", "on"], script_offering(&M2A_OPS, Some(&response))).await;

    let failure = outcome.result.expect_err("limit reached");
    assert_eq!(failure.exit_code(), 1);
    assert_eq!(
        failure.to_string(),
        "the request failed: limit_reached: cap (retry after 900000 ms)"
    );
    assert_eq!(outcome.output, "");
}

#[tokio::test]
async fn a_command_the_server_does_not_offer_is_not_sent() {
    for arguments in [
        &["move", "6", "3"][..],
        &["float", "6", "on"],
        &["close", "6", "--yes"],
        &["lock"],
        &["awake", "on"],
        &["awake", "off"],
    ] {
        let script = script_offering(&["ping", "state.subscribe"], None);

        let (outcome, sent) = run_recorded(arguments, script).await;

        let refused = outcome.result.expect_err("not offered");
        assert_eq!(refused.exit_code(), 1, "{refused}");
        assert!(sent.is_empty(), "{arguments:?} sent {sent:?}");
        assert_eq!(outcome.output, "");
    }
}

#[tokio::test]
async fn close_without_yes_exits_2_before_it_reads_the_profile() {
    let client_dir = tempfile::tempdir().expect("temp dir");

    let outcome = run_command(&["close", "6"], Connector::new(None), &client_dir).await;

    let refused = outcome.result.expect_err("not confirmed");
    assert_eq!(refused.exit_code(), 2);
    assert!(refused.to_string().contains("--yes"), "{refused}");
    assert_eq!(outcome.output, "");
}
