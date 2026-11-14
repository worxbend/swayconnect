//! Replays `protocol/vectors/v1/ext/session.json` and `ext/requests.json`, the M2a extensions
//! (`docs/PROTOCOL.md` 17.6 to 17.8). Formats: `protocol/vectors/v1/README.md`.
//!
//! The request cases are replayed up to step 6 of spec 10.2, the last step this crate can
//! decide. Steps 7 to 9, the sway commands and the keep-awake and lock rules belong to the
//! daemon tests.

mod support;

use serde_json::Value;
use support::{decode_message, entries, load, text};
use swayconnect_protocol::ops::{
    self, Args, Operation, RateClass, TOPIC_DESKTOP, TOPIC_SESSION, TopicState,
};
use swayconnect_protocol::{
    ErrorCode, InvalidMember, JsonObject, KeepAwakeResult, LockResult, Message, Req, Scope, Session,
};

/// The codes of steps 2 to 6 of spec 10.2.
const EARLY_CODES: [ErrorCode; 6] = [
    ErrorCode::UnknownOp,
    ErrorCode::Forbidden,
    ErrorCode::ReadOnly,
    ErrorCode::RateLimited,
    ErrorCode::BadRequest,
    ErrorCode::UnknownTopic,
];

/// The `data` of a `session` `state.changed` event.
fn session_data(name: &str, payload: &str) -> JsonObject {
    let (_, message) = decode_message(name, payload);
    let Message::Evt(evt) = message else {
        panic!("{name}: not an evt");
    };
    assert_eq!(evt.event, ops::EVENT_STATE_CHANGED, "{name}");
    let state = TopicState::decode(&evt.data).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(state.topic, TOPIC_SESSION, "{name}");
    state.data
}

fn decode_session(name: &str, data: &JsonObject) -> Session {
    Session::decode(data).unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn session_states() {
    let file = load("session.json");
    for vector in entries(&file["states"], "states") {
        let name = text(vector, "name");
        let data = session_data(name, text(vector, "payload"));
        let expect = &vector["expect"];
        if expect == "bad" {
            assert!(Session::decode(&data).is_err(), "{name}");
        } else if expect == "ok" {
            assert_eq!(decode_session(name, &data).encode(), data, "{name}");
        } else {
            let normal = session_data(name, text(expect, "normalized"));
            let expected = decode_session(name, &normal);
            assert_eq!(
                expected.encode(),
                normal,
                "{name}: normalized form re-encodes"
            );
            assert_eq!(decode_session(name, &data), expected, "{name}");
        }
    }
}

#[test]
fn session_results() {
    let file = load("session.json");
    for vector in entries(&file["results"], "results") {
        let name = text(vector, "name");
        let operation = Operation::from_wire(text(vector, "op")).expect("known operation");
        let (_, message) = decode_message(name, text(vector, "payload"));
        let Message::Res(res) = message else {
            panic!("{name}: not a res");
        };
        let result = res.result.unwrap_or_else(|| panic!("{name}: not ok"));
        let outcome = check_result(operation, &result);
        assert_eq!(
            outcome.is_ok(),
            vector["expect"] == "ok",
            "{name}: {outcome:?}"
        );
    }
}

/// Decodes a `result` the way a client does, and checks that it re-encodes to an equal value.
/// Every other operation of M2a answers `{}`, whose members a client ignores.
fn check_result(operation: Operation, result: &JsonObject) -> Result<(), InvalidMember> {
    match operation {
        Operation::KeepAwakeSet => {
            let decoded = KeepAwakeResult::decode(result)?;
            assert_eq!(KeepAwakeResult::decode(&decoded.encode()), Ok(decoded));
        }
        Operation::SessionLock => {
            let decoded = LockResult::decode(result)?;
            assert_eq!(LockResult::decode(&decoded.encode()), Ok(decoded));
        }
        _ => {}
    }
    Ok(())
}

#[test]
fn requests_follow_steps_2_to_6() {
    let file = load("requests.json");
    let max_duration_s = file["config"]["max_duration_s"]
        .as_u64()
        .expect("config max_duration_s");
    let (mut refused_early, mut passed) = (0, 0);
    for vector in entries(&file["requests"], "requests") {
        let name = text(vector, "name");
        let (_, message) = decode_message(name, text(vector, "payload"));
        let Message::Req(req) = message else {
            panic!("{name}: not a req");
        };
        let expected = expected_error(name, &vector["expect"]);
        match first_steps(vector, &req, max_duration_s) {
            Err(code) => {
                assert_eq!(Some(code), expected, "{name}");
                check_forbidden_detail(name, vector, &req);
                refused_early += 1;
            }
            Ok(args) => {
                assert!(
                    expected.is_none_or(|code| !EARLY_CODES.contains(&code)),
                    "{name}: passes steps 2 to 6, but expects {expected:?}"
                );
                let again = args.to_req(req.id);
                let operation = args.operation();
                assert_eq!(
                    operation.decode_args(again.args.as_ref()),
                    Ok(args),
                    "{name}"
                );
                check_expected_result(name, operation, &vector["expect"]);
                passed += 1;
            }
        }
    }
    assert!(
        refused_early >= 40 && passed >= 40,
        "{refused_early} / {passed}"
    );
}

/// `None` for `"ok"` and `{"result"}`. Every code of the file is one this version knows.
fn expected_error(name: &str, expect: &Value) -> Option<ErrorCode> {
    let code = expect.get("error")?.as_str().expect("error code");
    let code = ErrorCode::from_wire(code).unwrap_or_else(|| panic!("{name}: unknown {code}"));
    Some(code)
}

/// Steps 2 to 6 of spec 10.2 with the preconditions of the vector.
fn first_steps(vector: &Value, req: &Req, max_duration_s: u64) -> Result<Args, ErrorCode> {
    let operation = Operation::from_wire(&req.op).ok_or(ErrorCode::UnknownOp)?;
    let has_lock_command = vector["session"]["lock_command"] == true;
    if operation == Operation::SessionLock && !has_lock_command {
        return Err(ErrorCode::UnknownOp);
    }
    if operation
        .required_scope()
        .is_some_and(|scope| !scopes(vector).contains(&scope))
    {
        return Err(ErrorCode::Forbidden);
    }
    if vector["read_only"] == true && operation.is_action() {
        return Err(ErrorCode::ReadOnly);
    }
    if operation.rate_class() == Some(RateClass::Actions) && vector["actions_left"] == 0 {
        return Err(ErrorCode::RateLimited);
    }
    let bad_request = |_| ErrorCode::BadRequest;
    match operation
        .decode_args(req.args.as_ref())
        .map_err(bad_request)?
    {
        Args::KeepAwakeSet(keep_awake) => keep_awake
            .check_max_duration(max_duration_s)
            .map(Args::KeepAwakeSet)
            .map_err(bad_request),
        Args::StateSubscribe { topics } if topics.iter().any(|topic| !is_topic(topic)) => {
            Err(ErrorCode::UnknownTopic)
        }
        args => Ok(args),
    }
}

fn scopes(vector: &Value) -> Vec<Scope> {
    vector["scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .map(|scope| Scope::from_wire(scope.as_str().expect("scope name")).expect("known scope"))
        .collect()
}

fn is_topic(topic: &str) -> bool {
    topic == TOPIC_DESKTOP || topic == TOPIC_SESSION
}

fn check_forbidden_detail(name: &str, vector: &Value, req: &Req) {
    let Some(scope) = vector["expect"]["detail"].get("scope") else {
        return;
    };
    let operation = Operation::from_wire(&req.op).expect("known operation");
    let required = operation.required_scope().map(Scope::as_str);
    assert_eq!(scope.as_str(), required, "{name}");
}

/// An `expect.result` decodes as the result of its operation. A `{"min", "max"}` range is
/// read as its upper end.
fn check_expected_result(name: &str, operation: Operation, expect: &Value) {
    let Some(result) = expect.get("result") else {
        return;
    };
    let concrete: JsonObject = result
        .as_object()
        .expect("result object")
        .iter()
        .map(|(member, value)| (member.clone(), value.get("max").unwrap_or(value).clone()))
        .collect();
    assert_eq!(check_result(operation, &concrete), Ok(()), "{name}");
    assert!(
        matches!(operation, Operation::KeepAwakeSet | Operation::SessionLock),
        "{name}: only keep_awake.set and session.lock have a result"
    );
}
