use serde_json::Value;
use swayconnect_protocol::message::JsonObject;

use super::*;

const APPS_VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../protocol/vectors/v1/ext/apps.json"
);

fn object(json: &str) -> JsonObject {
    match serde_json::from_str(json).expect("vector JSON") {
        Value::Object(object) => object,
        other => panic!("not an object: {other}"),
    }
}

fn indexed(id: &str, name: &str) -> IndexedApp {
    IndexedApp {
        entry_id: id.to_owned(),
        name: name.to_owned(),
        generic_name: Some(format!("{name} tool")),
        keywords: vec!["k".to_owned()],
        has_icon: id == "firefox",
        terminal: false,
        risky: false,
        argv: vec![format!("/opt/{id}")],
    }
}

#[test]
fn every_cut_vector_matches() {
    let vectors: Value =
        serde_json::from_str(&std::fs::read_to_string(APPS_VECTORS).expect("apps.json"))
            .expect("apps.json is JSON");
    let cases = vectors["cut"].as_array().expect("cut cases");
    assert_eq!(cases.len(), 5);
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let input_text = case["input"].as_str().expect("input");
        let input = Apps::decode(&object(input_text)).expect("input decodes");
        assert_eq!(
            encoded_len(&input),
            case["input_bytes"],
            "{name}: input size"
        );
        let published = cut(input.clone());
        let expected = match case["expect"].as_str() {
            Some(text) => Apps::decode(&object(text)).expect("expect decodes"),
            None => input,
        };
        assert_eq!(published, expected, "{name}");
        assert_eq!(
            encoded_len(&published),
            case["expect_bytes"],
            "{name}: size"
        );
    }
}

#[test]
fn build_state_keeps_listed_recent_ids_and_asks_icons() {
    let apps = [indexed("firefox", "Firefox"), indexed("mpv", "mpv")];
    let recent = ["gone".to_owned(), "mpv".to_owned(), "firefox".to_owned()];
    let hash = IconHash::of(b"png");
    let state = build_state(&apps, &recent, |app| app.has_icon.then_some(hash));
    assert_eq!(state.recent, ["mpv", "firefox"]);
    assert!(!state.truncated);
    assert_eq!(state.apps[0].icon, Some(hash));
    assert_eq!(state.apps[1].icon, None);
    assert_eq!(state.apps[1].generic_name.as_deref(), Some("mpv tool"));
}

#[test]
fn the_count_cap_drops_ids_from_recent() {
    let apps: Vec<IndexedApp> = (0..515)
        .map(|i| indexed(&format!("app{i:04}"), &format!("App {i:04}")))
        .collect();
    let recent = ["app0514".to_owned(), "app0001".to_owned()];
    let state = build_state(&apps, &recent, |_| None);
    assert_eq!(state.apps.len(), 512);
    assert_eq!(state.recent, ["app0001"]);
    assert!(state.truncated);
}
