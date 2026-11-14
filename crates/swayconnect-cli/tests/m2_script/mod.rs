//! Runs one M2 subcommand of `swayconnect` in this process against the scripted server of
//! the client tests, shared by `cli_m2b.rs` and `cli_m2c.rs`.

#![allow(
    dead_code,
    reason = "every test binary compiles this module and uses a part of it"
)]

use std::time::Duration;

use clap::Parser as _;
use serde_json::{Value, json};
use swayconnect_cli::cli::{Cli, CliError, Context, ProfileStore, run};
use swayconnect_cli::client::Connector;
use tempfile::TempDir;

use crate::client_support::{
    Server, Step, auth_connector, good_config, profile, transcript, vector_fp, with_member,
};

const PATIENCE: Duration = Duration::from_secs(20);
/// Every op of M2b and M2c that a client sends.
const M2_OPS: [&str; 12] = [
    "ping",
    "state.subscribe",
    "audio.set_volume",
    "audio.set_mute",
    "audio.set_default",
    "media.play_pause",
    "media.next",
    "media.previous",
    "app.launch",
    "launcher.open",
    "input.text",
    "input.key",
];

pub struct Outcome {
    pub result: Result<(), CliError>,
    pub output: String,
}

/// Transcript T2 with `ops` set to [`M2_OPS`], then one response per entry of `responses`,
/// each after one request.
pub fn script_answering(responses: &[Value]) -> Vec<Step> {
    let mut script = script_offering(&M2_OPS);
    for response in responses {
        script.extend([Step::Receive, Step::Send(response.to_string())]);
    }
    script
}

/// Transcript T2 with `ops` set to `ops`, and nothing after `welcome`.
pub fn script_offering(ops: &[&str]) -> Vec<Step> {
    let t2 = transcript("t2-auth-ok");
    let welcome = with_member(&t2[2], "ops", json!(ops));
    vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(welcome),
    ]
}

pub fn ok_with(id: u64, result: &Value) -> Value {
    json!({"type": "res", "id": id, "ok": true, "result": result})
}

pub fn failed(id: u64, code: &str, detail: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": "no"});
    if let Some(detail) = detail {
        error["detail"] = detail;
    }
    json!({"type": "res", "id": id, "ok": false, "error": error})
}

pub fn subscribed(topic: &str, data: &Value) -> Value {
    let state = json!({"topic": topic, "rev": 1, "data": data});
    ok_with(1, &json!({"states": [state]}))
}

pub fn subscribe_request(topic: &str) -> Value {
    json!({"type": "req", "id": 1, "op": "state.subscribe", "args": {"topics": [topic]}})
}

pub fn request(id: u64, op: &str, args: &Value) -> Value {
    json!({"type": "req", "id": id, "op": op, "args": args})
}

/// Runs `arguments` with `stdin` against a server that plays `script`; returns what the
/// client sent after `auth`.
pub async fn run_recorded(
    arguments: &[&str],
    stdin: &str,
    script: Vec<Step>,
) -> (Outcome, Vec<Value>) {
    let server = Server::start(good_config(), script).await;
    let client_dir = tempfile::tempdir().expect("temp dir");
    ProfileStore::new(client_dir.path().to_owned())
        .save(&profile(server.port, vector_fp()))
        .expect("profile saved");
    let outcome = run_command(arguments, stdin, auth_connector(), &client_dir).await;
    let mut frames = server.received().await.expect("server script").frames;
    frames.remove(0);
    (outcome, frames)
}

pub async fn run_command(
    arguments: &[&str],
    stdin: &str,
    connector: Connector,
    client_dir: &TempDir,
) -> Outcome {
    let cli = Cli::try_parse_from([&["swayconnect"], arguments].concat()).expect("arguments");
    let mut output = Vec::new();
    let context = Context {
        connector,
        store: ProfileStore::new(client_dir.path().to_owned()),
        input: &mut stdin.as_bytes(),
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

/// Runs a command that must fail before it connects: no profile is stored.
pub async fn run_offline(arguments: &[&str], stdin: &str) -> CliError {
    let client_dir = tempfile::tempdir().expect("temp dir");
    let outcome = run_command(arguments, stdin, Connector::new(None), &client_dir).await;
    assert_eq!(outcome.output, "", "{arguments:?}");
    outcome.result.expect_err("refused before connecting")
}
