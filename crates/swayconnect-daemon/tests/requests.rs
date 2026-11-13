//! `protocol/vectors/v1/requests.json` replayed against a running daemon, each line with its
//! preconditions (`docs/PROTOCOL.md` 15.14). The unit test of `dispatch.rs` replays the same
//! file without a session, a rate limiter or sway.

mod support;

use serde_json::{Value, json};
use support::vectors::{payload_of, show_the_t3_desktop, vector_file};
use support::{EVERY_SCOPE, Harness, raw_exchange, soon, subscribed_desktop};
use swayconnect_cli::client::Session;
use swayconnect_protocol::limits::ACTION_BURST;
use swayconnect_protocol::{Desktop, JsonObject, Scope};
use swayconnect_testkit::FakeSway;

const ACTIONS: [&str; 3] = ["workspace.focus", "window.focus", "window.fullscreen"];
const PROBE_ID: u64 = 9_000;
const DRAIN_ID: u64 = 9_001;
/// Far more than the bucket holds; reached only when the bucket never runs empty.
const MAX_DRAIN_REQUESTS: u32 = 50;

/// The `desktop` state that `state_ref` of `requests.json` names.
fn referenced_desktop(state_ref: &str) -> Desktop {
    let (transcript, frame, path) = match state_ref {
        "t3-rev7" => ("t3-session", 1, "/result/states/0/data"),
        "offline" => ("t6-sway-offline", 0, "/data/data"),
        other => panic!("unknown state_ref `{other}`"),
    };
    let transcript = vector_file(&format!("transcripts/{transcript}.json"));
    let payload = Value::Object(payload_of(&transcript["frames"][frame]));
    let state = payload.pointer(path).and_then(Value::as_object);
    Desktop::decode(state.expect("desktop state")).expect("a valid desktop")
}

/// A daemon whose read-only mode and whose `desktop` state are the ones `vector` names.
async fn daemon_for(vector: &Value) -> Harness {
    let is_read_only = vector["read_only"].as_bool().expect("read_only");
    let config = format!("read_only = {is_read_only}\n");
    let show_rev_7 = |sway: &FakeSway| show_the_t3_desktop(sway, 1);
    let mut harness = Harness::start_prepared(&config, show_rev_7).await;
    let is_sway_online = vector["sway_online"].as_bool().expect("sway_online");
    if !is_sway_online {
        harness.stop_sway();
    }
    let expected = referenced_desktop(vector["state_ref"].as_str().expect("state_ref"));
    assert_eq!(expected.sway.online, is_sway_online);
    let observer = harness.pair(&EVERY_SCOPE).await;
    let mut observer = harness.connect(&observer).await;
    subscribed_desktop(&mut observer, |desktop| *desktop == expected).await;
    harness
}

/// Empties the action bucket of the device. A `workspace.focus` without a target takes a
/// token and reaches sway with nothing (spec 10.2 step 5 comes before step 6).
async fn empty_the_action_bucket(session: &mut Session) {
    let no_target = support::request_frame(DRAIN_ID, "workspace.focus", &json!({}));
    for _request in 0..MAX_DRAIN_REQUESTS {
        let answer = raw_exchange(session, &no_target).await;
        match answer["error"]["code"].as_str() {
            Some("bad_request") => {}
            Some("rate_limited") => return,
            _ => panic!("the device cannot spend action tokens: {answer:?}"),
        }
    }
    panic!("the action bucket never ran empty");
}

/// A session of a new device with the scopes and the action tokens that `vector` names.
async fn session_for(vector: &Value, harness: &Harness) -> Session {
    let scopes = vector["scopes"].as_array().expect("scopes").iter();
    let scopes: Vec<Scope> = scopes
        .map(|name| Scope::from_wire(name.as_str().expect("scope name")).expect("a scope"))
        .collect();
    let profile = harness.pair(&scopes).await;
    let mut session = harness.connect(&profile).await;
    // A new device has a full bucket.
    match vector["actions_left"].as_u64() {
        Some(left) if left == u64::from(ACTION_BURST) => {}
        Some(0) => empty_the_action_bucket(&mut session).await,
        other => panic!("this test cannot leave {other:?} action tokens"),
    }
    session
}

/// What the daemon answers to the frame of `vector`, in the words of `requests.json`, with
/// the `detail` of an error beside it.
async fn answer_to(vector: &Value, session: &mut Session) -> (Value, Value) {
    let payload = vector["payload"].as_str().expect("payload");
    let sent = soon("vector frame", session.send_raw_frame(payload.as_bytes())).await;
    sent.expect("frame sent");
    if vector["expect"] == "ignored" {
        // Answers come in request order, so a pong first means the frame got none.
        let probe = support::request_frame(PROBE_ID, "ping", &json!({}));
        let pong = raw_exchange(session, &probe).await;
        assert_eq!((&pong["id"], &pong["ok"]), (&json!(PROBE_ID), &json!(true)));
        return (json!("ignored"), Value::Null);
    }
    let answer = soon("answer", session.receive_raw_frame()).await;
    let answer = answer.expect("an answer frame");
    if answer["type"] == "bye" {
        return (json!({"bye": answer["code"]}), Value::Null);
    }
    assert_eq!(answer["type"], "res", "{answer:?}");
    assert_eq!(answer["id"], payload_of(vector)["id"], "{answer:?}");
    if answer["ok"] == true {
        return (json!("ok"), Value::Null);
    }
    let error = &answer["error"];
    (json!({"error": error["code"]}), error["detail"].clone())
}

fn assert_detail_matches(name: &str, expected: &JsonObject, answered: &Value) {
    for (member, expected) in expected {
        let answered = &answered[member];
        if member == "retry_after_ms" {
            let delay = answered.as_u64().expect("retry_after_ms");
            let (min, max) = (expected["min"].as_u64(), expected["max"].as_u64());
            assert!(min <= Some(delay) && Some(delay) <= max, "{name}: {delay}");
        } else {
            assert_eq!(answered, expected, "{name}: {member}");
        }
    }
}

/// An action reaches sway exactly when the vector expects success.
fn expected_commands(vector: &Value) -> usize {
    let payload = payload_of(vector);
    let op = payload.get("op").and_then(Value::as_str);
    let is_action = op.is_some_and(|op| ACTIONS.contains(&op));
    usize::from(is_action && vector["expect"] == "ok")
}

#[tokio::test]
async fn every_request_vector_is_answered_by_a_daemon_as_the_spec_says() {
    let file = vector_file("requests.json");
    let vectors = file.as_array().expect("an array of cases");
    assert!(
        vectors.len() >= 24,
        "the file holds the cases of spec 15.14"
    );
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let harness = daemon_for(vector).await;
        let mut session = session_for(vector, &harness).await;

        let (answered, detail) = answer_to(vector, &mut session).await;

        let expect = &vector["expect"];
        let mut expected_answer = expect.clone();
        if let Some(expected_detail) = expected_answer
            .as_object_mut()
            .and_then(|expected| expected.remove("detail"))
        {
            let expected_detail = expected_detail.as_object().expect("detail object");
            assert_detail_matches(name, expected_detail, &detail);
        }
        assert_eq!(answered, expected_answer, "{name}");
        if vector["sway_online"] == true {
            let reached_sway = harness.sway().commands();
            assert_eq!(reached_sway.len(), expected_commands(vector), "{name}");
        }
    }
}
