//! The M2b additions of `dispatch.rs`: the typed decisions, the topic scopes of
//! `state.subscribe`, and a replay of `protocol/vectors/v1/ext/{audio,media,apps,input}.json`
//! through steps 2 to 6, the `max_volume_pct` limit and, for `input.*`, the target window of
//! steps 7 and 8. What a service answers after that (step 8 of the other ops, 8a to 8c, the
//! backend) is tested with the services and in `session/m2b_tests.rs`.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use swayconnect_protocol::limits::{
    ACTION_BURST, CONTINUOUS_BURST, ICON_BURST, INPUT_BURST, INPUT_TEXT_BURST, LAUNCH_BURST,
};
use swayconnect_protocol::{DecodeError, Desktop, Message};
use tokio::time::Instant;

use super::tests::{assert_detail_matches, mode, payload_of, vector_file};
use super::*;
use crate::actions;

fn request(op: &str, args: Value) -> Req {
    Req {
        id: 1,
        op: op.to_owned(),
        args: Some(args),
    }
}

fn every_scope() -> BTreeSet<Scope> {
    KNOWN_SCOPES.into_iter().collect()
}

#[test]
fn the_m2b_requests_leave_as_their_typed_decisions() {
    let table = [
        (
            request(
                "audio.set_volume",
                json!({"device_id": 3, "volume_pct": 55}),
            ),
            Decision::Audio {
                device_id: 3,
                change: AudioChange::Volume(55),
            },
        ),
        (
            request("audio.set_default", json!({"device_id": 3})),
            Decision::Audio {
                device_id: 3,
                change: AudioChange::Default,
            },
        ),
        (
            request("media.previous", json!({"player_id": 2})),
            Decision::Media {
                player_id: 2,
                command: MediaCommand::Previous,
            },
        ),
        (
            request("app.launch", json!({"entry_id": "firefox", "number": 3})),
            Decision::Launch {
                entry_id: EntryId::parse("firefox").expect("entry id"),
                number: Some(3),
            },
        ),
        (request("launcher.open", json!({})), Decision::OpenLauncher),
        (
            request("input.text", json!({"launcher": true, "text": "ls"})),
            Decision::Type {
                target: InputTarget::Launcher,
                text: InputText::parse("ls").expect("text"),
            },
        ),
        (
            request(
                "state.subscribe",
                json!({"topics": ["apps", "audio", "media"]}),
            ),
            Decision::Subscribe(vec![Topic::Apps, Topic::Audio, Topic::Media]),
        ),
    ];
    for (request, decision) in table {
        let outcome = check(&every_scope(), mode(false), |_| Ok(()), &request);
        assert_eq!(outcome, Ok(decision), "{request:?}");
    }
}

#[test]
fn a_topic_needs_its_scope_after_every_name_is_known() {
    let reader = BTreeSet::from([Scope::StateRead, Scope::Audio]);
    let subscribe = |topics: Value| request("state.subscribe", json!({"topics": topics}));

    let error = check(
        &reader,
        mode(false),
        |_| Ok(()),
        &subscribe(json!(["audio", "media"])),
    )
    .expect_err("media needs its scope");
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(error.detail, detail("scope", "media"));

    let error = check(
        &reader,
        mode(false),
        |_| Ok(()),
        &subscribe(json!(["media", "x"])),
    )
    .expect_err("unknown name");
    assert_eq!(error.code, ErrorCode::UnknownTopic);

    assert_eq!(
        offered_topics(&reader),
        ["desktop", "session", "audio"].map(str::to_owned)
    );
    let without_state_read = BTreeSet::from([Scope::Audio, Scope::Media, Scope::Launch]);
    assert!(offered_topics(&without_state_read).is_empty());
}

/// The state that `state_ref` `ext-desktop-NN` names: case `NN` of `ext/desktop.json`.
fn ext_desktop(state_ref: &str) -> Desktop {
    let number = state_ref
        .strip_prefix("ext-desktop-")
        .expect("an ext-desktop ref");
    let cases = vector_file("ext/desktop.json");
    let case = cases
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| {
            case["name"]
                .as_str()
                .is_some_and(|name| name.starts_with(number))
        })
        .expect("the named case");
    let payload = Value::Object(payload_of(case));
    let state = payload.pointer("/data/data").and_then(Value::as_object);
    Desktop::decode(state.expect("desktop state")).expect("a valid desktop")
}

/// The `buckets` key of a rate class, and its burst (spec 17.14).
fn bucket_of(class: RateClass) -> (&'static str, u32) {
    match class {
        RateClass::Actions => ("actions", ACTION_BURST),
        RateClass::Icons => ("icons", ICON_BURST),
        RateClass::Continuous => ("continuous", CONTINUOUS_BURST),
        RateClass::Launch => ("launch", LAUNCH_BURST),
        RateClass::Input => ("input", INPUT_BURST),
        RateClass::InputText => ("input_text", INPUT_TEXT_BURST),
    }
}

/// The real bucket of `class`, holding the tokens the case gives it.
fn bucket_take(vector: &Value, class: RateClass) -> Result<(), Duration> {
    let (name, burst) = bucket_of(class);
    let rate = class.rate();
    let now = Instant::now();
    let mut full_again_at = now;
    let tokens_left = vector["buckets"][name]
        .as_u64()
        .expect("the case names this bucket");
    for _ in tokens_left..u64::from(burst) {
        rate.take(&mut full_again_at, now).expect("a token");
    }
    rate.take(&mut full_again_at, now)
}

/// The answer to one case, in the words of the file. `"later"`: the request passed every
/// check this file decides, and a service answers the rest.
fn replay(file: &Value, vector: &Value) -> Value {
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
    let is_read_only = vector["read_only"].as_bool().expect("read_only");
    let take_token = |class| bucket_take(vector, class);
    let refusal = match check(&scopes, mode(is_read_only), take_token, &request) {
        Ok(Decision::Audio { change, .. }) => {
            let max = file["config"]["max_volume_pct"].as_u64().expect("max");
            let max = u16::try_from(max).expect("a u16");
            match change.check_max_volume(max) {
                Ok(_) => return json!("later"),
                Err(_) => volume_refusal(),
            }
        }
        Ok(Decision::Type { target, .. } | Decision::Key { target, .. }) => {
            let InputTarget::Window(window_id) = target else {
                return json!("later");
            };
            let desktop = ext_desktop(vector["state_ref"].as_str().expect("state_ref"));
            match actions::check_input_window(window_id, &desktop) {
                Ok(()) => return json!("later"),
                Err(error) => action_refusal(Action::FocusWindow(window_id), error),
            }
        }
        Ok(_) => return json!("later"),
        Err(refusal) => refusal,
    };
    let detail = refusal.detail.map_or(Value::Null, Value::Object);
    json!({"error": refusal.code.as_str(), "detail": detail})
}

/// Codes only a service gives, after the checks replayed here.
const SERVICE_CODES: [&str; 9] = [
    "not_found",
    "unsupported_target",
    "backend_unavailable",
    "timeout",
    "spawn_failed",
    "focus_changed",
    "locked",
    "keyboard_grabbed",
    "launcher_closed",
];
/// The scopes of 17.16 steps 8a and 8b, which the services check.
const LATER_SCOPES: [&str; 2] = ["audio.capture", "launch.risky"];

fn is_for_a_service(expect: &Value) -> bool {
    if expect == "ok" || expect.get("result").is_some() {
        return true;
    }
    let code = expect["error"].as_str().unwrap_or_default();
    let scope = expect["detail"]["scope"].as_str().unwrap_or_default();
    SERVICE_CODES.contains(&code) || (code == "forbidden" && LATER_SCOPES.contains(&scope))
}

#[test]
fn the_m2b_request_vectors_pass_or_fail_the_dispatch_checks_as_the_spec_says() {
    let mut replayed = 0;
    for name in ["audio", "media", "apps", "input"] {
        let file = vector_file(&format!("ext/{name}.json"));
        let vectors = file["requests"].as_array().expect("requests");
        for vector in vectors {
            let case = format!("{name}/{}", vector["name"].as_str().expect("name"));
            let expect = &vector["expect"];
            let answered = replay(&file, vector);
            replayed += 1;
            if answered == "later" {
                assert!(is_for_a_service(expect), "{case}: {expect}");
                continue;
            }
            assert!(
                !is_for_a_service(expect) || expect["error"] == "not_found",
                "{case}"
            );
            assert_eq!(answered["error"], expect["error"], "{case}");
            if let Some(expected_detail) = expect.get("detail").and_then(Value::as_object) {
                assert_detail_matches(&case, expected_detail, &answered["detail"]);
            }
        }
    }
    assert!(replayed >= 300, "cases of the four files: {replayed}");
}
