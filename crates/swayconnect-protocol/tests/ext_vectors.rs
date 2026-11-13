//! Replays `protocol/vectors/v1/ext` for the M1 extensions (`docs/PROTOCOL.md` 17.3 to 17.5,
//! 17.15). Formats: `protocol/vectors/v1/README.md`, section "Extensions".
//!
//! Server checks that need a session (scope, read-only, rate limit, the icon store) are
//! replayed by the daemon tests; this file covers decode, encode and the client rules.

use std::collections::BTreeSet;

use serde_json::Value;
use swayconnect_protocol::fingerprint::decode_base64url_bytes;
use swayconnect_protocol::frame::encode_frame;
use swayconnect_protocol::limits::MAX_FRAME_S2C_BYTES;
use swayconnect_protocol::ops::{self, Args, Operation, TopicState};
use swayconnect_protocol::{
    Availability, ByeCode, Desktop, IconGetResult, IconHash, IconRejection, JsonObject, Message,
    Scope, Welcome,
};

const EXT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1/ext");

fn load(file: &str) -> Value {
    let path = format!("{EXT}/{file}");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("member `{member}` is not a string in {value}"))
}

fn entries<'a>(value: &'a Value, what: &str) -> &'a [Value] {
    let entries = value
        .as_array()
        .unwrap_or_else(|| panic!("{what} is not an array"));
    assert!(!entries.is_empty(), "{what} is empty");
    entries
}

/// A list that may be empty.
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{value} is not a list"))
        .iter()
        .map(|entry| entry.as_str().expect("string entry").to_owned())
        .collect()
}

/// `null` means the member is absent.
fn optional_strings(value: &Value) -> Option<Vec<String>> {
    (!value.is_null()).then(|| strings(value))
}

fn hashes(value: &Value) -> Vec<IconHash> {
    value
        .as_array()
        .expect("hash list")
        .iter()
        .map(|hash| IconHash::parse(hash.as_str().expect("hash string")).expect("vector hash"))
        .collect()
}

fn decode_message(name: &str, payload: &str) -> (JsonObject, Message) {
    let object: JsonObject = serde_json::from_str(payload).expect("payload is a JSON object");
    let message = Message::decode(&object).unwrap_or_else(|error| panic!("{name}: {error}"));
    (object, message)
}

/// The `data` of a `desktop` `state.changed` event.
fn desktop_data(name: &str, payload: &str) -> JsonObject {
    let (_, message) = decode_message(name, payload);
    let Message::Evt(evt) = message else {
        panic!("{name}: not an evt");
    };
    assert_eq!(evt.event, ops::EVENT_STATE_CHANGED, "{name}");
    let state = TopicState::decode(&evt.data).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(state.topic, ops::TOPIC_DESKTOP, "{name}");
    state.data
}

fn decode_desktop(name: &str, data: &JsonObject) -> Desktop {
    Desktop::decode(data).unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn desktop_file() {
    let file = load("desktop.json");
    for vector in entries(&file, "desktop.json") {
        let name = text(vector, "name");
        let data = desktop_data(name, text(vector, "payload"));
        let expect = &vector["expect"];
        if expect == "bad" {
            assert!(Desktop::decode(&data).is_err(), "{name}");
        } else if expect == "ok" {
            assert_eq!(decode_desktop(name, &data).encode(), data, "{name}");
        } else {
            let normal = desktop_data(name, text(expect, "normalized"));
            let expected = decode_desktop(name, &normal);
            assert_eq!(
                expected.encode(),
                normal,
                "{name}: normalized form re-encodes"
            );
            let mut desktop = decode_desktop(name, &data);
            desktop.normalize();
            assert_eq!(desktop, expected, "{name}");
        }
    }
}

#[test]
fn icon_store_hashes() {
    let file = load("icons.json");
    for entry in entries(&file["store"], "store") {
        let png = decode_base64url_bytes(text(entry, "png_b64url")).expect("store PNG");
        assert_eq!(IconHash::of(&png).to_string(), text(entry, "hash"));
    }
}

/// The argument check of each request; the other checks belong to the daemon.
#[test]
fn icon_requests() {
    let file = load("icons.json");
    for vector in entries(&file["requests"], "requests") {
        let name = text(vector, "name");
        let (_, message) = decode_message(name, text(vector, "payload"));
        let Message::Req(req) = message else {
            panic!("{name}: not a req");
        };
        let operation = Operation::from_wire(&req.op).expect("icon.get is known");
        assert_eq!(operation, Operation::IconGet, "{name}");
        let args = operation.decode_args(req.args.as_ref());
        let expect = &vector["expect"];
        if let Some(result) = expect.get("result") {
            let Ok(Args::IconGet { hashes: requested }) = &args else {
                panic!("{name}: {args:?}");
            };
            let mut answered = hashes(&result["icons"]);
            answered.extend(hashes(&result["missing"]));
            assert_eq!(
                BTreeSet::from_iter(requested),
                BTreeSet::from_iter(&answered),
                "{name}"
            );
            // Unknown members are ignored, so compare the decoded arguments of the re-encoding.
            let again = Args::IconGet {
                hashes: requested.clone(),
            }
            .to_req(req.id);
            assert_eq!(operation.decode_args(again.args.as_ref()), args, "{name}");
        } else if expect["error"] == "bad_request" {
            assert!(args.is_err(), "{name}");
        } else if let Some(scope) = expect["detail"].get("scope") {
            let required = operation.required_scope().map(Scope::as_str);
            assert_eq!(scope.as_str(), required, "{name}");
        }
    }
}

#[test]
fn icon_results() {
    let file = load("icons.json");
    for vector in entries(&file["results"], "results") {
        let name = text(vector, "name");
        let payload = text(vector, "payload");
        let (object, message) = decode_message(name, payload);
        let Message::Res(res) = message else {
            panic!("{name}: not a res");
        };
        let result = res.result.expect("ok result");
        let decoded = IconGetResult::decode(&result);
        let expect = &vector["expect"];
        if expect == "bad" {
            assert!(decoded.is_err(), "{name}: {decoded:?}");
            continue;
        }
        let decoded = decoded.unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(decoded.encode(), result, "{name}");
        assert!(encode_frame(&object, MAX_FRAME_S2C_BYTES).is_ok(), "{name}");

        let (mut accepted, mut dropped) = (Vec::new(), Vec::new());
        for icon in &decoded.icons {
            match icon.verify() {
                Ok(()) => accepted.push(icon.hash),
                Err(rejection) => dropped.push((icon.hash, reason(rejection))),
            }
        }
        assert_eq!(accepted, hashes(&expect["accepted"]), "{name}");
        let expected_dropped: Vec<(IconHash, &str)> = expect["dropped"]
            .as_array()
            .expect("dropped list")
            .iter()
            .map(|entry| {
                (
                    hashes(&Value::from(vec![entry["hash"].clone()]))[0],
                    text(entry, "reason"),
                )
            })
            .collect();
        assert_eq!(dropped, expected_dropped, "{name}");
        assert_eq!(decoded.missing, hashes(&expect["missing"]), "{name}");
    }
}

fn reason(rejection: IconRejection) -> &'static str {
    match rejection {
        IconRejection::HashMismatch => "hash_mismatch",
        IconRejection::NotPng => "not_png",
        IconRejection::Bounds => "bounds",
    }
}

/// Unknown scope names are dropped from the typed `scopes` (13.2 rule 6); this version knows
/// only the scopes of section 7.1. `known_scopes`, `ops` and `known_ops` keep every name.
#[test]
fn welcome_frames() {
    let file = load("welcome.json");
    for vector in entries(&file["frames"], "frames") {
        let name = text(vector, "name");
        let object: JsonObject = serde_json::from_str(text(vector, "payload")).expect("JSON");
        let decoded = Message::decode(&object);
        let expect = &vector["expect"];
        if expect == "bad" {
            assert!(decoded.is_err(), "{name}: {decoded:?}");
            continue;
        }
        let Ok(Message::Welcome(welcome)) = decoded else {
            panic!("{name}: {decoded:?}");
        };
        let fields = &expect["fields"];
        let scope_names = strings(&fields["scopes"]);
        let known: Vec<Scope> = scope_names
            .iter()
            .filter_map(|scope| Scope::from_wire(scope))
            .collect();
        assert_eq!(welcome.scopes, known, "{name}");
        assert_eq!(welcome.ops, strings(&fields["ops"]), "{name}");
        let optional = |member: &str| optional_strings(&fields[member]);
        assert_eq!(welcome.known_scopes, optional("known_scopes"), "{name}");
        assert_eq!(welcome.known_ops, optional("known_ops"), "{name}");
        assert_eq!(
            Value::from(welcome.read_only),
            fields["read_only"],
            "{name}"
        );
        if known.len() == scope_names.len() {
            assert_eq!(Message::Welcome(welcome).encode(), object, "{name}");
        }
    }
}

/// The cases whose operation this version knows. The others name operations of M2a and M2b.
#[test]
fn welcome_availability() {
    let file = load("welcome.json");
    let base = welcome_of(&file["frames"][0]);
    let mut replayed = 0;
    for vector in entries(&file["availability"], "availability") {
        let name = text(vector, "name");
        let Some(operation) = Operation::from_wire(text(vector, "op")) else {
            continue;
        };
        let welcome = Welcome {
            scopes: strings(&vector["scopes"])
                .iter()
                .filter_map(|scope| Scope::from_wire(scope))
                .collect(),
            ops: strings(&vector["ops"]),
            known_scopes: optional_strings(&vector["known_scopes"]),
            known_ops: optional_strings(&vector["known_ops"]),
            read_only: vector["read_only"].as_bool().expect("read_only"),
            ..base.clone()
        };
        let expected = match text(vector, "expect") {
            "available" => Availability::Available,
            "read_only" => Availability::ReadOnly,
            "unsupported" => Availability::Unsupported,
            "locked" => {
                Availability::Locked(Scope::from_wire(text(vector, "scope")).expect("scope"))
            }
            other => panic!("{name}: unknown expect {other}"),
        };
        assert_eq!(welcome.availability(operation), expected, "{name}");
        replayed += 1;
    }
    assert!(replayed >= 8, "only {replayed} availability cases replayed");
}

fn welcome_of(frame: &Value) -> Welcome {
    let (_, message) = decode_message("frames[0]", text(frame, "payload"));
    let Message::Welcome(welcome) = message else {
        panic!("frames[0] is not a welcome");
    };
    welcome
}

/// `op_table` agrees with this crate for every operation it knows.
#[test]
fn op_table_matches_operations() {
    let file = load("welcome.json");
    let table = file["op_table"].as_object().expect("op_table object");
    let mut known = 0;
    for (op, row) in table {
        let Some(operation) = Operation::from_wire(op) else {
            continue;
        };
        let scope = operation.required_scope().map(Scope::as_str);
        assert_eq!(row["scope"].as_str(), scope, "{op}");
        assert_eq!(row["action"].as_bool(), Some(operation.is_action()), "{op}");
        known += 1;
    }
    assert_eq!(
        known, 6,
        "ping, state.subscribe, icon.get and the three v1 actions"
    );
}

#[test]
fn bye_codes() {
    let file = load("welcome.json");
    for vector in entries(&file["bye"], "bye") {
        let name = text(vector, "name");
        let (object, message) = decode_message(name, text(vector, "payload"));
        let Message::Bye(bye) = message else {
            panic!("{name}: not a bye");
        };
        let reaction = match bye.code {
            ByeCode::ScopesChanged => "reconnect_now",
            ByeCode::Revoked => "forget",
            _ => "reconnect_backoff",
        };
        assert_eq!(reaction, text(vector, "expect"), "{name}");
        if ByeCode::from_wire(text(&Value::Object(object.clone()), "code")).is_some() {
            assert_eq!(Message::Bye(bye).encode(), object, "{name}");
        }
    }
}
