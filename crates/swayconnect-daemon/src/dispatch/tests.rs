//! Tests of `dispatch.rs`: the check order, and `protocol/vectors/v1/requests.json`.

use std::cell::Cell;
use std::fs;

use serde_json::json;
use swayconnect_protocol::limits::ACTION_BURST;
use swayconnect_protocol::{DecodeError, Desktop, Message};
use tokio::time::Instant;

use super::*;
use crate::actions;
use crate::throttle::{Rate, RateClass};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1");
const EVERY_SCOPE: [Scope; 3] = [Scope::StateRead, Scope::StateTitles, Scope::Control];
const RETRY_AFTER: Duration = Duration::from_millis(500);

fn request(op: &str, args: Option<Value>) -> Req {
    Req {
        id: 1,
        op: op.to_owned(),
        args,
    }
}

/// One valid request per action form.
fn action_requests() -> [(Req, Action); 5] {
    let fullscreen = |enabled| Action::Fullscreen {
        window_id: 6,
        enabled,
    };
    [
        (
            request("workspace.focus", Some(json!({"number": 3}))),
            Action::FocusWorkspaceNumber(3),
        ),
        (
            request("workspace.focus", Some(json!({"workspace_id": 4}))),
            Action::FocusWorkspaceId(4),
        ),
        (
            request("window.focus", Some(json!({"window_id": 6}))),
            Action::FocusWindow(6),
        ),
        (
            request(
                "window.fullscreen",
                Some(json!({"window_id": 6, "enabled": true})),
            ),
            fullscreen(true),
        ),
        (
            request(
                "window.fullscreen",
                Some(json!({"window_id": 6, "enabled": false})),
            ),
            fullscreen(false),
        ),
    ]
}

/// A daemon that is not read-only and a bucket that is never empty.
fn checked(scopes: &[Scope], request: &Req) -> Result<Decision, ErrorBody> {
    let scopes = scopes.iter().copied().collect();
    check(&scopes, false, |_| Ok(()), request)
}

fn refused(scopes: &[Scope], op: &str, args: Option<Value>) -> ErrorBody {
    checked(scopes, &request(op, args)).expect_err("the request is refused")
}

#[test]
fn ping_needs_no_scope_and_ignores_missing_args() {
    assert_eq!(checked(&[], &request("ping", None)), Ok(Decision::Pong));
}

#[test]
fn subscribe_to_desktop_is_granted_with_state_read() {
    let subscribe = request("state.subscribe", Some(json!({"topics": ["desktop"]})));
    let outcome = checked(&[Scope::StateRead], &subscribe);
    assert_eq!(outcome, Ok(Decision::SubscribeDesktop));
}

#[test]
fn each_action_is_handed_on_as_its_typed_value_and_takes_one_token() {
    let scopes = BTreeSet::from([Scope::Control]);
    for (request, action) in action_requests() {
        let taken = Cell::new(0);
        let take_token = |_| {
            taken.set(taken.get() + 1);
            Ok(())
        };
        let outcome = check(&scopes, false, take_token, &request);
        assert_eq!(outcome, Ok(Decision::Act(action)), "{request:?}");
        assert_eq!(taken.get(), 1, "{request:?}");
    }
}

#[test]
fn in_read_only_mode_no_action_is_handed_on_and_no_token_is_taken() {
    let scopes = EVERY_SCOPE.into_iter().collect();
    for (request, _) in action_requests() {
        let take_token = |_| panic!("read-only is checked before the rate limit");
        let outcome = check(&scopes, true, take_token, &request);
        let error = outcome.expect_err("no `Decision::Act` in read-only mode");
        assert_eq!(error.code, ErrorCode::ReadOnly, "{request:?}");
        assert_eq!(error.detail, None);
    }
}

#[test]
fn read_only_mode_leaves_ping_and_subscribe_alone() {
    let scopes = EVERY_SCOPE.into_iter().collect();
    let take_token = |_| panic!("only an action takes a token");
    let subscribe = request("state.subscribe", Some(json!({"topics": ["desktop"]})));
    assert_eq!(
        check(&scopes, true, take_token, &subscribe),
        Ok(Decision::SubscribeDesktop)
    );
    assert_eq!(
        check(&scopes, true, take_token, &request("ping", None)),
        Ok(Decision::Pong)
    );
}

#[test]
fn a_missing_scope_is_reported_before_read_only_and_takes_no_token() {
    let scopes = BTreeSet::from([Scope::StateRead]);
    for (request, _) in action_requests() {
        let take_token = |_| panic!("the scope is checked before the rate limit");
        let error = check(&scopes, true, take_token, &request).expect_err("forbidden");
        assert_eq!(error.code, ErrorCode::Forbidden, "{request:?}");
        assert_eq!(
            Value::Object(error.detail.expect("detail")),
            json!({"scope": "control"})
        );
    }
}

#[test]
fn an_empty_bucket_is_reported_with_the_retry_delay_before_bad_arguments() {
    let scopes = BTreeSet::from([Scope::Control]);
    let no_arguments = request("window.focus", Some(json!("not an object")));
    let error = check(&scopes, false, |_| Err(RETRY_AFTER), &no_arguments).expect_err("limited");
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(
        Value::Object(error.detail.expect("detail")),
        json!({"retry_after_ms": 500})
    );
}

#[test]
fn the_retry_delay_is_never_reported_as_zero() {
    let scopes = BTreeSet::from([Scope::Control]);
    let (focus, _) = &action_requests()[2];
    let almost_now = |_| Err(Duration::from_micros(1));
    let error = check(&scopes, false, almost_now, focus).expect_err("limited");
    assert_eq!(error.detail.expect("detail")["retry_after_ms"], 1);
}

#[test]
fn an_action_with_bad_arguments_still_takes_its_token() {
    let scopes = BTreeSet::from([Scope::Control]);
    let bad_arguments = [
        ("workspace.focus", json!({})),
        ("workspace.focus", json!({"number": 1000})),
        ("workspace.focus", json!({"number": 1, "workspace_id": 4})),
        ("workspace.focus", json!({"number": "1; exec true"})),
        ("window.focus", json!({"window_id": -1})),
        ("window.focus", json!({"window_id": "6"})),
        ("window.fullscreen", json!({"window_id": 6})),
        ("window.fullscreen", json!({"window_id": 6, "enabled": 1})),
    ];
    for (op, args) in bad_arguments {
        let taken = Cell::new(0);
        let take_token = |_| {
            taken.set(taken.get() + 1);
            Ok(())
        };
        let outcome = check(&scopes, false, take_token, &request(op, Some(args.clone())));
        let error = outcome.expect_err("bad arguments");
        assert_eq!(error.code, ErrorCode::BadRequest, "{op} {args}");
        assert_eq!(taken.get(), 1, "{op} {args}");
    }
}

#[test]
fn the_offered_operations_follow_the_scopes_and_the_mode() {
    let every_scope = EVERY_SCOPE.into_iter().collect();
    let without_control = BTreeSet::from([Scope::StateRead, Scope::StateTitles]);
    let read_side = ["ping", "state.subscribe", "icon.get"];
    let actions = ["workspace.focus", "window.focus", "window.fullscreen"];

    let all = ["ping", "state.subscribe"]
        .into_iter()
        .chain(actions)
        .chain(["icon.get"]);
    assert_eq!(offered_ops(&every_scope, false), Vec::from_iter(all));
    assert_eq!(offered_ops(&every_scope, true), read_side);
    assert_eq!(offered_ops(&without_control, false), read_side);
    assert_eq!(
        offered_ops(&BTreeSet::from([Scope::Control]), false),
        [&["ping"][..], &actions].concat()
    );
}

#[test]
fn a_device_without_scopes_is_offered_ping_and_no_topic() {
    assert_eq!(offered_ops(&BTreeSet::new(), false), ["ping"]);
    assert_eq!(offered_topics(&BTreeSet::new()), Vec::<String>::new());
    let with_read = BTreeSet::from([Scope::StateRead]);
    assert_eq!(offered_topics(&with_read), ["desktop"]);
}

#[test]
fn an_unknown_operation_is_reported_before_its_arguments() {
    let error = refused(&[], "desktop.explode", Some(json!("not an object")));
    assert_eq!(error.code, ErrorCode::UnknownOp);
}

#[test]
fn a_missing_scope_is_reported_before_bad_arguments_and_names_the_scope() {
    let error = refused(&[Scope::Control], "state.subscribe", Some(json!(7)));
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(
        Value::Object(error.detail.expect("detail")),
        json!({"scope": "state.read"})
    );
}

#[test]
fn arguments_that_are_not_an_object_are_a_bad_request() {
    for args in [json!(null), json!([]), json!("x"), json!(1)] {
        let error = refused(&[], "ping", Some(args));
        assert_eq!(error.code, ErrorCode::BadRequest);
    }
    let error = refused(&[Scope::StateRead], "state.subscribe", None);
    assert_eq!(error.code, ErrorCode::BadRequest);
}

#[test]
fn bad_arguments_are_reported_before_an_unknown_topic() {
    let twice = json!({"topics": ["nope", "nope"]});
    let error = refused(&[Scope::StateRead], "state.subscribe", Some(twice));
    assert_eq!(error.code, ErrorCode::BadRequest);
}

#[test]
fn an_unknown_topic_fails_the_whole_request_and_is_named() {
    let topics = json!({"topics": ["desktop", "outputs"]});
    let error = refused(&[Scope::StateRead], "state.subscribe", Some(topics));
    assert_eq!(error.code, ErrorCode::UnknownTopic);
    assert_eq!(
        Value::Object(error.detail.expect("detail")),
        json!({"topic": "outputs"})
    );
}

#[test]
fn each_refusal_of_the_sway_half_has_its_error_code() {
    let table = [
        (ActionError::NotFound, ErrorCode::NotFound),
        (ActionError::UnsupportedTarget, ErrorCode::UnsupportedTarget),
        (ActionError::SwayUnavailable, ErrorCode::SwayUnavailable),
        (ActionError::Timeout, ErrorCode::Timeout),
    ];
    for (error, code) in table {
        let body = action_refusal(Action::FocusWindow(5), error);
        assert_eq!((body.code, body.detail), (code, None));
        assert!(!body.message.is_empty());
    }
    let sway_text = ActionError::SwayError("No matching node.".to_owned());
    let body = action_refusal(Action::FocusWindow(5), sway_text);
    assert_eq!(body.code, ErrorCode::SwayError);
    assert_eq!(body.message, "No matching node.");
}

#[test]
fn not_found_names_the_kind_of_target_and_its_id() {
    let table = [
        (Action::FocusWindow(999), "no window with id 999"),
        (
            Action::Fullscreen {
                window_id: 12,
                enabled: true,
            },
            "no window with id 12",
        ),
        (Action::FocusWorkspaceId(5), "no workspace with id 5"),
        (
            Action::FocusWorkspaceNumber(3),
            "no workspace with number 3",
        ),
    ];
    for (action, message) in table {
        let body = action_refusal(action, ActionError::NotFound);
        assert_eq!(body.message, message);
    }
}

fn vector_file(name: &str) -> Value {
    let text = fs::read_to_string(format!("{VECTORS}/{name}")).expect("vector file");
    serde_json::from_str(&text).expect("vector JSON")
}

fn payload_of(frame: &Value) -> JsonObject {
    serde_json::from_str(frame["payload"].as_str().expect("payload")).expect("a JSON object")
}

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

/// What the daemon answers to one frame of `requests.json`, in the words of that file.
/// Sway is assumed to accept every command: its answer (step 9) is no precondition there.
fn replay(vector: &Value) -> Value {
    let request = match Message::decode(&payload_of(vector)) {
        Ok(Message::Req(request)) => request,
        Err(DecodeError::UnknownType) => return json!("ignored"),
        Ok(_) | Err(_) => return json!({"bye": "protocol_error"}),
    };
    let scopes = vector["scopes"].as_array().expect("scopes").iter();
    let scopes: BTreeSet<Scope> = scopes
        .map(|name| Scope::from_wire(name.as_str().expect("scope name")).expect("a scope"))
        .collect();
    let is_read_only = vector["read_only"].as_bool().expect("read_only");
    let desktop = referenced_desktop(vector["state_ref"].as_str().expect("state_ref"));
    assert_eq!(vector["sway_online"], desktop.sway.online);

    // The real bucket, with `actions_left` tokens in it.
    let now = Instant::now();
    let mut full_again_at = now;
    let tokens_left = vector["actions_left"].as_u64().expect("actions_left");
    for _ in tokens_left..u64::from(ACTION_BURST) {
        let spent = Rate::ACTIONS.take(&mut full_again_at, now);
        spent.expect("the bucket holds a token");
    }
    let take_token = |class| {
        assert_eq!(
            class,
            RateClass::Actions,
            "requests.json holds no `icon.get`"
        );
        Rate::ACTIONS.take(&mut full_again_at, now)
    };

    let refusal = match check(&scopes, is_read_only, take_token, &request) {
        Ok(Decision::Act(action)) => match actions::command_for(action, &desktop) {
            Ok(_) => return json!("ok"),
            Err(error) => action_refusal(action, error),
        },
        Ok(_) => return json!("ok"),
        Err(refusal) => refusal,
    };
    let detail = refusal.detail.map_or(Value::Null, Value::Object);
    json!({"error": refusal.code.as_str(), "detail": detail})
}

pub(super) fn assert_detail_matches(name: &str, expected: &JsonObject, answered: &Value) {
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

#[test]
fn the_request_vectors_are_answered_as_the_spec_says() {
    let file = vector_file("requests.json");
    let vectors = file.as_array().expect("an array of cases");
    assert!(
        vectors.len() >= 24,
        "the file holds the cases of spec 15.14"
    );
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let expect = &vector["expect"];
        let answered = replay(vector);
        let Some(expected_error) = expect.get("error") else {
            assert_eq!(&answered, expect, "{name}");
            continue;
        };
        assert_eq!(&answered["error"], expected_error, "{name}");
        if let Some(expected_detail) = expect.get("detail").and_then(Value::as_object) {
            assert_detail_matches(name, expected_detail, &answered["detail"]);
        }
    }
}
