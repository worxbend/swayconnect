//! Tests of the M2a answers (`session/answers.rs`): topic `session`, `keep_awake.set`,
//! `session.lock`, the audit lines, and the release on revoke. Fake programs only: the
//! runner starts nothing.

use std::fs;

use serde_json::{Value, json};

use super::harness::{Client, Daemon, EVERY_SCOPE};
use super::*;
use crate::audit::audit_file;
use crate::daemon::controls;
use crate::lockstate::add_fake_process;
use crate::proc::Exit;

const TEST_DEADLINE: Duration = Duration::from_secs(10);

fn request(id: u64, op: &str, args: Value) -> Value {
    json!({"type": "req", "id": id, "op": op, "args": args})
}

fn subscribe_to(id: u64, topics: &[&str]) -> Value {
    request(id, "state.subscribe", json!({"topics": topics}))
}

/// The `result` of a successful `res`.
async fn result_of(client: &mut Client) -> JsonObject {
    let res = client.receive().await;
    assert_eq!(res["ok"], true, "{res:?}");
    res["result"].as_object().expect("result").clone()
}

async fn error_code(client: &mut Client) -> String {
    let res = client.receive().await;
    assert_eq!(res["ok"], false, "{res:?}");
    res["error"]["code"].as_str().expect("code").to_owned()
}

/// The next `state.changed` of topic `session`, skipping other frames.
async fn next_session_state(client: &mut Client) -> JsonObject {
    loop {
        let frame = timeout(TEST_DEADLINE, client.receive())
            .await
            .expect("a frame in time");
        if frame["type"] == "evt" && frame["data"]["topic"] == "session" {
            return frame["data"]["data"].as_object().expect("state").clone();
        }
    }
}

fn audit_lines(daemon: &Daemon) -> Vec<Value> {
    let paths = crate::paths::Paths {
        config_file: daemon.dirs.config_file(),
        data_dir: daemon.dirs.data_dir(),
        runtime_dir: daemon.dirs.runtime_dir(),
    };
    let text = fs::read_to_string(audit_file(&paths)).expect("audit file");
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

#[tokio::test]
async fn subscribe_returns_each_topic_in_request_order_and_session_is_per_device() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, _) = daemon.ready(&device).await;

    client.send(&subscribe_to(1, &["session", "desktop"])).await;
    let result = result_of(&mut client).await;

    let states = result["states"].as_array().expect("states");
    assert_eq!(states.len(), 2);
    assert_eq!(states[0]["topic"], "session");
    assert_eq!(states[1]["topic"], "desktop");
    let keep_awake = &states[0]["data"]["keep_awake"];
    assert_eq!(keep_awake["active"], false);
    assert_eq!(keep_awake["holder"], Value::Null);
    assert_eq!(keep_awake["max_duration_s"], 7200);
    // This daemon offers input and the launcher (M2b), so both members are present.
    assert_eq!(
        states[0]["data"]["input"],
        json!({"active": false, "holder": null})
    );
    assert_eq!(
        states[0]["data"]["launcher"],
        json!({"open": false, "holder": null})
    );
}

#[tokio::test]
async fn keep_awake_on_reaches_the_inhibitor_holder_and_audit_log_and_off_ends_it() {
    let daemon = Daemon::start().await;
    let holder = daemon.paired_device(&EVERY_SCOPE).await;
    let watcher = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut holding, _) = daemon.ready(&holder).await;
    let (mut watching, _) = daemon.ready(&watcher).await;
    watching.send(&subscribe_to(1, &["session"])).await;
    result_of(&mut watching).await;

    let on = json!({"enabled": true, "duration_s": 600});
    holding.send(&request(2, "keep_awake.set", on)).await;
    let result = result_of(&mut holding).await;

    assert_eq!(result["remaining_ms"], 600_000);
    let argv = daemon.runner.argvs();
    assert_eq!(argv[0][0], "/fake/systemd-inhibit");
    assert_eq!(argv[0].last().map(String::as_str), Some("600"));
    let seen = loop {
        let state = next_session_state(&mut watching).await;
        if state["keep_awake"]["active"] == true {
            break state;
        }
    };
    assert_eq!(seen["keep_awake"]["holder"], "other_device");

    holding
        .send(&request(3, "keep_awake.set", json!({"enabled": false})))
        .await;
    assert_eq!(result_of(&mut holding).await["remaining_ms"], Value::Null);
    assert!(daemon.runner.was_killed(0));

    let lines = audit_lines(&daemon);
    let ops: Vec<_> = lines.iter().map(|line| line["op"].clone()).collect();
    assert_eq!(ops, [json!("keep_awake.set"), json!("keep_awake.set")]);
    assert_eq!(lines[0]["duration_s"], 600);
    assert_eq!(lines[0]["device"], holder.id.as_str());
    assert_eq!(lines[0]["result"], "ok");
    assert!(lines[1].get("duration_s").is_none());
}

#[tokio::test]
async fn a_refused_keep_awake_is_audited_with_its_code_and_spawns_nothing() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;

    let too_long = json!({"enabled": true, "duration_s": 7201});
    client.send(&request(1, "keep_awake.set", too_long)).await;
    assert_eq!(error_code(&mut client).await, "bad_request");
    daemon.runner.fail_next_spawn();
    client
        .send(&request(2, "keep_awake.set", json!({"enabled": true})))
        .await;
    assert_eq!(error_code(&mut client).await, "spawn_failed");

    let lines = audit_lines(&daemon);
    assert_eq!(lines[0]["result"], "bad_request");
    assert_eq!(lines[1]["result"], "spawn_failed");
    assert_eq!(lines[1]["duration_s"], 3600, "the default duration");
}

#[tokio::test]
async fn window_close_is_audited_with_its_target_even_when_sway_refuses_it() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;

    client
        .send(&request(1, "window.close", json!({"window_id": 6})))
        .await;
    assert_eq!(error_code(&mut client).await, "sway_unavailable");
    client
        .send(&request(
            2,
            "window.floating",
            json!({"window_id": 6, "enabled": true}),
        ))
        .await;
    assert_eq!(error_code(&mut client).await, "sway_unavailable");

    let lines = audit_lines(&daemon);
    assert_eq!(lines.len(), 1, "window.floating is tier 1: {lines:?}");
    assert_eq!(lines[0]["op"], "window.close");
    assert_eq!(lines[0]["targets"], json!([6]));
    assert_eq!(lines[0]["result"], "sway_unavailable");
    assert_eq!(lines[0]["peer"], "127.0.0.1:40000");
}

#[tokio::test]
async fn session_lock_starts_one_lock_command_and_reports_a_running_one_or_a_locked_screen() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;

    client.send(&request(1, "session.lock", json!({}))).await;
    assert_eq!(result_of(&mut client).await["already_locked"], false);
    client.send(&request(2, "session.lock", json!({}))).await;
    assert_eq!(result_of(&mut client).await["already_locked"], true);
    assert_eq!(daemon.runner.argvs(), [["/fake/lock.sh"]]);

    daemon.runner.end_child(0, Exit::Code(0));
    let uid = rustix::process::getuid().as_raw();
    add_fake_process(&daemon.proc_root, 77, "swaylock", uid);
    client.send(&request(3, "session.lock", json!({}))).await;
    assert_eq!(result_of(&mut client).await["already_locked"], true);
    assert_eq!(daemon.runner.spawned_count(), 1);
    assert!(audit_lines(&daemon).is_empty(), "session.lock is tier 1");
}

#[tokio::test]
async fn read_only_refuses_keep_awake_and_lock_and_starts_nothing() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    daemon.shared.read_only.store(true, Ordering::Relaxed);
    let (mut client, welcome) = daemon.ready(&device).await;
    assert_eq!(
        welcome["ops"],
        json!(["ping", "state.subscribe", "icon.get"])
    );

    client
        .send(&request(1, "keep_awake.set", json!({"enabled": true})))
        .await;
    assert_eq!(error_code(&mut client).await, "read_only");
    client.send(&request(2, "session.lock", json!({}))).await;
    assert_eq!(error_code(&mut client).await, "read_only");
    client.send(&subscribe_to(3, &["session"])).await;
    result_of(&mut client).await;

    assert_eq!(daemon.runner.calls(), Vec::new());
}

#[tokio::test]
async fn revoking_the_holder_ends_keep_awake_and_another_device_keeps_it() {
    let daemon = Daemon::start().await;
    let keep_awake = daemon.shared.controls.keep_awake.clone();
    let guard = controls::release_on_revoke(
        keep_awake.clone(),
        daemon.shared.security.clone(),
        daemon.shared.shutdown.clone(),
    );
    tokio::spawn(guard);
    let holder = daemon.paired_device(&EVERY_SCOPE).await;
    let other = daemon.paired_device(&EVERY_SCOPE).await;
    let on = swayconnect_protocol::ops::KeepAwake::On { duration_s: None };
    keep_awake.set(&holder.id, on).await.expect("on");

    daemon.shared.security.revoke(&other.id).expect("revoked");
    tokio::task::yield_now().await;
    assert!(keep_awake.view().borrow().hold.is_some());

    daemon.shared.security.revoke(&holder.id).expect("revoked");
    let mut view = keep_awake.view();
    timeout(TEST_DEADLINE, view.wait_for(|view| view.hold.is_none()))
        .await
        .expect("released in time")
        .expect("keep-awake runs");
    assert!(daemon.runner.was_killed(0));
}
