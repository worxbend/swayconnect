//! Helpers shared by the replays of `protocol/vectors/v1/ext`.

use serde_json::Value;
use swayconnect_protocol::{JsonObject, Message};

const EXT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1/ext");

pub fn load(file: &str) -> Value {
    let path = format!("{EXT}/{file}");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
}

pub fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("member `{member}` is not a string in {value}"))
}

pub fn entries<'a>(value: &'a Value, what: &str) -> &'a [Value] {
    let entries = value
        .as_array()
        .unwrap_or_else(|| panic!("{what} is not an array"));
    assert!(!entries.is_empty(), "{what} is empty");
    entries
}

pub fn decode_message(name: &str, payload: &str) -> (JsonObject, Message) {
    let object: JsonObject = serde_json::from_str(payload).expect("payload is a JSON object");
    let message = Message::decode(&object).unwrap_or_else(|error| panic!("{name}: {error}"));
    (object, message)
}
