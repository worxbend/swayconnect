//! Tests of the section 17 additions in `dispatch.rs`: the operation table, `welcome`
//! `known_*`, and `protocol/vectors/v1/ext/icons.json` and `ext/welcome.json`.

use std::collections::BTreeSet;
use std::fs;

use serde_json::{Value, json};
use swayconnect_protocol::fingerprint::{decode_base64url_bytes, encode_base64url};
use swayconnect_protocol::limits::ICON_BURST;
use swayconnect_protocol::{DecodeError, Message};
use tokio::time::Instant;

use super::tests::assert_detail_matches;
use super::*;
use crate::icons::IconKey;
use crate::icons::store::Store;
use crate::throttle::Rate;

const EXT_VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1/ext");

fn ext_vector_file(name: &str) -> Value {
    let text = fs::read_to_string(format!("{EXT_VECTORS}/{name}")).expect("vector file");
    serde_json::from_str(&text).expect("vector JSON")
}

fn scopes_of(names: &Value) -> BTreeSet<Scope> {
    let names = names.as_array().expect("scope names");
    names
        .iter()
        .map(|name| Scope::from_wire(name.as_str().expect("scope name")).expect("a scope"))
        .collect()
}

fn names(list: &Value) -> BTreeSet<String> {
    let list = list.as_array().expect("a list of names");
    list.iter()
        .map(|name| name.as_str().expect("a name").to_owned())
        .collect()
}

fn request(op: &str, args: Value) -> Req {
    Req {
        id: 1,
        op: op.to_owned(),
        args: Some(args),
    }
}

#[test]
fn each_served_operation_has_its_scope_rate_class_and_action_flag() {
    let table = [
        (Operation::Ping, None, None, false),
        (
            Operation::StateSubscribe,
            Some(Scope::StateRead),
            None,
            false,
        ),
        (
            Operation::IconGet,
            Some(Scope::StateRead),
            Some(RateClass::Icons),
            false,
        ),
        (
            Operation::WorkspaceFocus,
            Some(Scope::Control),
            Some(RateClass::Actions),
            true,
        ),
        (
            Operation::WindowFocus,
            Some(Scope::Control),
            Some(RateClass::Actions),
            true,
        ),
        (
            Operation::WindowFullscreen,
            Some(Scope::Control),
            Some(RateClass::Actions),
            true,
        ),
    ];
    let served: BTreeSet<&str> = SERVED_OPERATIONS.iter().map(|op| op.as_str()).collect();
    let tabled: BTreeSet<&str> = table.iter().map(|(op, ..)| op.as_str()).collect();
    assert_eq!(served, tabled, "every served operation is in the table");

    let spec_table = &ext_vector_file("welcome.json")["op_table"];
    for (operation, scope, class, is_action) in table {
        let name = operation.as_str();
        assert_eq!(operation.required_scope(), scope, "{name}");
        assert_eq!(rate_class(operation), class, "{name}");
        assert_eq!(operation.is_action(), is_action, "{name}");
        let spec = &spec_table[name];
        assert_eq!(spec["scope"], json!(scope.map(Scope::as_str)), "{name}");
        assert_eq!(spec["action"], json!(is_action), "{name}");
    }
}

#[test]
fn known_ops_and_scopes_name_everything_this_daemon_serves_and_knows() {
    let ops = known_ops();
    let every_scope: BTreeSet<Scope> = KNOWN_SCOPES.into_iter().collect();
    assert_eq!(ops, offered_ops(&every_scope, false));
    assert_eq!(
        known_scopes(),
        ["state.read", "state.titles", "control"].map(str::to_owned)
    );
    // Breaks the build when a scope is added without a place in `KNOWN_SCOPES`.
    let is_listed = |scope: Scope| match scope {
        Scope::StateRead | Scope::StateTitles | Scope::Control => KNOWN_SCOPES.contains(&scope),
    };
    assert!(KNOWN_SCOPES.into_iter().all(is_listed));
}

#[test]
fn read_only_refuses_every_action_and_still_answers_icon_get() {
    let every_scope: BTreeSet<Scope> = KNOWN_SCOPES.into_iter().collect();
    let hash = encode_base64url(&[0; 16]);
    let icon_get = request("icon.get", json!({"hashes": [hash]}));
    let outcome = check(&every_scope, true, |_| Ok(()), &icon_get);
    assert!(matches!(outcome, Ok(Decision::Icons(_))));

    for operation in SERVED_OPERATIONS.into_iter().filter(|op| op.is_action()) {
        let request = request(operation.as_str(), json!({}));
        let take_token = |_| panic!("read-only is checked before the rate limit");
        let error = check(&every_scope, true, take_token, &request).expect_err("refused");
        assert_eq!(error.code, ErrorCode::ReadOnly, "{}", operation.as_str());
    }
}

#[test]
fn icon_get_takes_a_token_from_the_icons_bucket_only() {
    let scopes = BTreeSet::from([Scope::StateRead]);
    let hash = encode_base64url(&[0; 16]);
    let icon_get = request("icon.get", json!({"hashes": [hash]}));
    let mut classes = Vec::new();
    let take_token = |class| {
        classes.push(class);
        Ok(())
    };
    check(&scopes, false, take_token, &icon_get).expect("answered");
    assert_eq!(classes, [RateClass::Icons]);

    let error = check(
        &scopes,
        false,
        |_| Err(Duration::from_millis(500)),
        &icon_get,
    )
    .expect_err("limited");
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert_eq!(error.message, "too many icon requests from this device");
}

#[test]
fn the_server_ops_vectors_of_this_milestone_match_the_offered_ops() {
    let file = ext_vector_file("welcome.json");
    let cases = file["server_ops"].as_array().expect("server_ops");
    let ours: BTreeSet<String> = known_ops().into_iter().collect();
    let mut replayed = 0;
    for case in cases {
        let name = case["name"].as_str().expect("name");
        // Cases of later milestones name operations this daemon does not serve yet.
        if names(&case["known_ops"]) != ours {
            continue;
        }
        let read_only = case["read_only"].as_bool().expect("read_only");
        let offered = offered_ops(&scopes_of(&case["scopes"]), read_only);
        let offered: BTreeSet<String> = offered.into_iter().collect();
        assert_eq!(offered, names(&case["expect_ops"]), "{name}");
        replayed += 1;
    }
    assert_eq!(replayed, 5, "cases 03 to 07 are the M1 cases");
}

/// The icon store of `icons.json`: its PNGs and no others.
fn vector_store(file: &Value) -> Store {
    let mut store = Store::default();
    for (index, entry) in file["store"].as_array().expect("store").iter().enumerate() {
        let png = decode_base64url_bytes(entry["png_b64url"].as_str().expect("png_b64url"))
            .expect("base64url");
        let key = IconKey {
            app_id: Some(format!("app{index}")),
            ..IconKey::default()
        };
        let hash = store.record_produced(key, png);
        assert_eq!(hash.to_string(), entry["hash"].as_str().expect("hash"));
    }
    store
}

/// What the daemon answers to one frame of `icons.json`, in the words of that file.
fn replay_icons(store: &Store, vector: &Value) -> Value {
    let payload = vector["payload"].as_str().expect("payload");
    let frame: JsonObject = serde_json::from_str(payload).expect("a JSON object");
    let request = match Message::decode(&frame) {
        Ok(Message::Req(request)) => request,
        Err(DecodeError::UnknownType) => return json!("ignored"),
        Ok(_) | Err(_) => return json!({"bye": "protocol_error"}),
    };
    let scopes = scopes_of(&vector["scopes"]);
    let is_read_only = vector["read_only"].as_bool().expect("read_only");

    // The real bucket, with `icons_left` tokens in it.
    let now = Instant::now();
    let mut full_again_at = now;
    let tokens_left = vector["icons_left"].as_u64().expect("icons_left");
    for _ in tokens_left..u64::from(ICON_BURST) {
        Rate::ICONS
            .take(&mut full_again_at, now)
            .expect("the bucket holds a token");
    }
    let take_token = |class| {
        assert_eq!(class, RateClass::Icons);
        Rate::ICONS.take(&mut full_again_at, now)
    };
    match check(&scopes, is_read_only, take_token, &request) {
        Ok(Decision::Icons(hashes)) => {
            let result = store.icons(&hashes);
            for icon in &result.icons {
                assert_eq!(IconHash::of(&icon.png), icon.hash, "served PNG matches");
            }
            let hashes = |list: Vec<IconHash>| list.iter().map(ToString::to_string).collect();
            let served: Vec<String> = hashes(result.icons.iter().map(|icon| icon.hash).collect());
            let missing: Vec<String> = hashes(result.missing);
            json!({"result": {"icons": served, "missing": missing}})
        }
        Ok(_) => panic!("`icon.get` was answered as another operation"),
        Err(refusal) => {
            let detail = refusal.detail.map_or(Value::Null, Value::Object);
            json!({"error": refusal.code.as_str(), "detail": detail})
        }
    }
}

#[test]
fn the_icon_request_vectors_are_answered_as_the_spec_says() {
    let file = ext_vector_file("icons.json");
    let store = vector_store(&file);
    let vectors = file["requests"].as_array().expect("requests");
    assert!(vectors.len() >= 28, "the file holds cases 01 to 28");
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        let expect = &vector["expect"];
        let answered = replay_icons(&store, vector);
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
