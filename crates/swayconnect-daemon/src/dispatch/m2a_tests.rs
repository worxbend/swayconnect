//! Replay of `protocol/vectors/v1/ext/requests.json` through steps 2 to 8 of spec 10.2: the
//! dispatch table, then the target checks of `actions.rs` for the sway actions. What the
//! keep-awake task and the lock command answer after step 6 is tested in `keep_awake`,
//! `session_topic` and `session/m2a_tests.rs`; the exact sway text in the integration tests.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use swayconnect_protocol::limits::ACTION_BURST;
use swayconnect_protocol::{DecodeError, Desktop, Message};
use tokio::time::Instant;

use super::tests::{assert_detail_matches, payload_of, referenced_desktop, vector_file};
use super::*;
use crate::actions;
use crate::throttle::Rate;

/// The state of `ext/desktop.json` case `03`, which `state_ref` `ext-desktop-03` names.
fn ext_desktop_03() -> Desktop {
    let cases = vector_file("ext/desktop.json");
    let cases = cases.as_array().expect("cases");
    let case = cases
        .iter()
        .find(|case| {
            case["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("03-"))
        })
        .expect("case 03");
    let payload = Value::Object(payload_of(case));
    let state = payload.pointer("/data/data").and_then(Value::as_object);
    Desktop::decode(state.expect("desktop state")).expect("a valid desktop")
}

fn desktop_of(state_ref: &str) -> Desktop {
    match state_ref {
        "ext-desktop-03" => ext_desktop_03(),
        other => referenced_desktop(other),
    }
}

/// The answer to one case, in the words of the file. `"later"`: the request passed step 6
/// and the rest is decided by the keep-awake task or the lock command.
fn replay(vector: &Value, max_duration_s: u64) -> Value {
    let request = match Message::decode(&payload_of(vector)) {
        Ok(Message::Req(request)) => request,
        Err(DecodeError::UnknownType) => return json!("ignored"),
        Ok(_) | Err(_) => return json!({"bye": "protocol_error"}),
    };
    let scopes: BTreeSet<Scope> = vector["scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .map(|name| Scope::from_wire(name.as_str().expect("scope name")).expect("a scope"))
        .collect();
    let mode = Mode {
        is_read_only: vector["read_only"].as_bool().expect("read_only"),
        features: Features {
            lock: vector["session"]["lock_command"]
                .as_bool()
                .expect("lock_command"),
        },
    };
    let desktop = desktop_of(vector["state_ref"].as_str().expect("state_ref"));
    assert_eq!(vector["sway_online"], desktop.sway.online);

    let now = Instant::now();
    let mut full_again_at = now;
    let tokens_left = vector["actions_left"].as_u64().expect("actions_left");
    for _ in tokens_left..u64::from(ACTION_BURST) {
        Rate::ACTIONS
            .take(&mut full_again_at, now)
            .expect("the bucket holds a token");
    }
    let take_token = |class| {
        assert_eq!(
            class,
            RateClass::Actions,
            "every op of the file is an action"
        );
        Rate::ACTIONS.take(&mut full_again_at, now)
    };

    let refusal = match check(&scopes, mode, take_token, &request) {
        Ok(Decision::Act(action)) => match actions::command_for(action, &desktop) {
            Ok(_) => return json!("ok"),
            Err(error) => action_refusal(action, error),
        },
        Ok(Decision::KeepAwake(args)) => match args.check_max_duration(max_duration_s) {
            Ok(_) => return json!("later"),
            Err(_) => refusal(ErrorCode::BadRequest, "invalid arguments", None),
        },
        Ok(Decision::Lock) => return json!("later"),
        Ok(_) => return json!("ok"),
        Err(refusal) => refusal,
    };
    let detail = refusal.detail.map_or(Value::Null, Value::Object);
    json!({"error": refusal.code.as_str(), "detail": detail})
}

/// Errors that only the keep-awake task or the lock command can give.
const LATER_ERRORS: [&str; 3] = ["limit_reached", "spawn_failed", "backend_unavailable"];

#[test]
fn the_m2a_request_vectors_pass_or_fail_steps_2_to_8_as_the_spec_says() {
    let file = vector_file("ext/requests.json");
    let max_duration_s = file["config"]["max_duration_s"]
        .as_u64()
        .expect("max_duration_s");
    let vectors = file["requests"].as_array().expect("requests");
    assert!(vectors.len() >= 100, "cases 01 to 125");
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let expect = &vector["expect"];
        let answered = replay(vector, max_duration_s);
        let sway = vector.get("sway").and_then(Value::as_array);
        if answered == "later" {
            let is_result = expect.get("result").is_some();
            let is_later_error = expect["error"]
                .as_str()
                .is_some_and(|code| LATER_ERRORS.contains(&code));
            assert!(is_result || is_later_error, "{name}: {expect}");
            continue;
        }
        if answered == "ok" {
            assert!(
                expect == "ok" || expect.get("result").is_some(),
                "{name}: {expect}"
            );
            if request_op(vector).starts_with("window.") || request_op(vector) == "scratchpad.show"
            {
                assert!(
                    sway.is_some_and(|sway| sway.len() == 1),
                    "{name}: one command"
                );
            }
            continue;
        }
        assert!(
            sway.is_none_or(Vec::is_empty),
            "{name}: a refusal sends nothing"
        );
        let expected_error = expect
            .get("error")
            .unwrap_or_else(|| panic!("{name}: {expect}"));
        assert_eq!(&answered["error"], expected_error, "{name}");
        if let Some(expected_detail) = expect.get("detail").and_then(Value::as_object) {
            assert_detail_matches(name, expected_detail, &answered["detail"]);
        }
    }
}

fn request_op(vector: &Value) -> String {
    let payload = payload_of(vector);
    payload["op"].as_str().unwrap_or_default().to_owned()
}
