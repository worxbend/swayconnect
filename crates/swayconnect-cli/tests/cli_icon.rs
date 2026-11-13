//! `swayconnect icon`, run in this process against the scripted server of the client tests.
//! The daemon has no icon in its desktop state yet, so the paths that need a PNG are here.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;

use std::path::Path;
use std::time::Duration;

use clap::Parser as _;
use client_support::{
    Server, Step, auth_connector, good_config, profile, transcript, vector_fp, with_member,
};
use serde_json::{Value, json};
use swayconnect_cli::cli::{Cli, CliError, Context, ProfileStore, run};
use swayconnect_protocol::IconHash;
use swayconnect_protocol::fingerprint::encode_base64url;
use tempfile::TempDir;

const PATIENCE: Duration = Duration::from_secs(20);
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// The first 24 bytes of a PNG of `side` x `side` pixels: all that `Icon::verify` reads.
fn png(side: u32) -> Vec<u8> {
    let mut png = PNG_SIGNATURE.to_vec();
    png.extend_from_slice(&[0, 0, 0, 13]);
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&side.to_be_bytes());
    png
}

/// A login whose `welcome` offers `ops`.
fn login(ops: &[&str]) -> Vec<Step> {
    let t2 = transcript("t2-auth-ok");
    vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(with_member(&t2[2], "ops", json!(ops))),
    ]
}

/// A login that offers `icon.get`, then `result` as the answer to the first request.
fn icon_script(result: &Value) -> Vec<Step> {
    let response = json!({"type": "res", "id": 1, "ok": true, "result": result});
    let mut script = login(&["ping", "state.subscribe", "icon.get"]);
    script.extend([Step::Receive, Step::Send(response.to_string())]);
    script
}

struct Outcome {
    result: Result<(), CliError>,
    output: String,
    /// The frames the client sent after `auth`.
    sent: Vec<Value>,
}

async fn run_icon(hash: IconHash, file: &Path, json_flag: bool, script: Vec<Step>) -> Outcome {
    let server = Server::start(good_config(), script).await;
    let client_dir = tempfile::tempdir().expect("temp dir");
    ProfileStore::new(client_dir.path().to_owned())
        .save(&profile(server.port, vector_fp()))
        .expect("profile saved");
    let hash = hash.to_string();
    let file = file.to_str().expect("UTF-8 path");
    let mut arguments = vec!["swayconnect", "icon", &hash, "-o", file];
    if json_flag {
        arguments.push("--json");
    }
    let cli = Cli::try_parse_from(arguments).expect("arguments");
    let mut output = Vec::new();
    let context = Context {
        connector: auth_connector(),
        store: ProfileStore::new(client_dir.path().to_owned()),
        input: &mut std::io::empty(),
        output: &mut output,
    };
    let result = tokio::time::timeout(PATIENCE, run(&cli, context))
        .await
        .expect("the command ends");
    let mut sent = server.received().await.expect("server script").frames;
    sent.remove(0);
    Outcome {
        result,
        output: String::from_utf8(output).expect("UTF-8 output"),
        sent,
    }
}

fn icon_entry(hash: IconHash, png: &[u8]) -> Value {
    json!({"icons": [{"hash": hash.to_string(), "png": encode_base64url(png)}], "missing": []})
}

fn target() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("temp dir");
    let file = directory.path().join("app.png");
    (directory, file)
}

#[tokio::test]
async fn a_checked_icon_is_written_to_the_file() {
    let png = png(48);
    let hash = IconHash::of(&png);
    let (_directory, file) = target();

    let outcome = run_icon(hash, &file, false, icon_script(&icon_entry(hash, &png))).await;

    outcome.result.expect("icon saved");
    assert_eq!(std::fs::read(&file).expect("file"), png);
    let expected_request = json!({"type": "req", "id": 1, "op": "icon.get",
        "args": {"hashes": [hash.to_string()]}});
    assert_eq!(outcome.sent, [expected_request]);
    let expected = format!("saved {} bytes to {}\n", png.len(), file.display());
    assert_eq!(outcome.output, expected);

    let outcome = run_icon(hash, &file, true, icon_script(&icon_entry(hash, &png))).await;
    outcome.result.expect("icon saved");
    let printed: Value = serde_json::from_str(&outcome.output).expect("one JSON value");
    assert_eq!(printed["hash"], hash.to_string());
    assert_eq!(printed["bytes"], png.len());
}

#[tokio::test]
async fn an_icon_that_fails_the_client_check_is_not_written() {
    let small = png(48);
    let hash = IconHash::of(&small);
    let large = png(257);
    let large_hash = IconHash::of(&large);
    let other = IconHash::of(b"other");
    for (wanted, result) in [
        (hash, icon_entry(hash, &png(49))),
        (large_hash, icon_entry(large_hash, &large)),
        (hash, json!({"icons": [], "missing": []})),
        (hash, icon_entry(other, &small)),
        (
            hash,
            json!({"icons": [{"hash": "x", "png": ""}], "missing": []}),
        ),
        (hash, json!({"icons": []})),
    ] {
        let (_directory, file) = target();

        let outcome = run_icon(wanted, &file, false, icon_script(&result)).await;

        let refused = outcome.result.expect_err("refused");
        assert!(
            matches!(refused, CliError::InvalidIcon(_)),
            "{result}: {refused}"
        );
        assert_eq!(refused.exit_code(), 3);
        assert!(!file.exists(), "{result}");
        assert_eq!(outcome.output, "");
    }
}

#[tokio::test]
async fn a_missing_icon_exits_1_and_a_server_without_icon_get_is_not_asked() {
    let hash = IconHash::of(b"gone");
    let (_directory, file) = target();
    let missing = json!({"icons": [], "missing": [hash.to_string()]});

    let outcome = run_icon(hash, &file, false, icon_script(&missing)).await;
    let refused = outcome.result.expect_err("missing");
    assert!(matches!(refused, CliError::NoIcon), "{refused}");
    assert_eq!(refused.exit_code(), 1);
    assert!(!file.exists());

    let outcome = run_icon(hash, &file, false, login(&["ping", "state.subscribe"])).await;
    let refused = outcome.result.expect_err("not offered");
    assert_eq!(refused.exit_code(), 1, "{refused}");
    assert!(outcome.sent.is_empty(), "sent {:?}", outcome.sent);
    assert!(!file.exists());
}
