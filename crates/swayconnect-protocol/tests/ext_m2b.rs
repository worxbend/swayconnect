//! Replays the `states`, `server_states`, `results` and `errors` of
//! `protocol/vectors/v1/ext/{audio,media,apps,input}.json`, the M2b extensions
//! (`docs/PROTOCOL.md` 17.9 to 17.13). Formats: `protocol/vectors/v1/README.md`.
//! The request cases are in `ext_m2b_requests.rs`. The media `server` cases and the apps `cut`
//! cases are server rules (17.10, 17.11) and belong to the daemon tests.

mod support;

use std::fmt::Debug;

use serde_json::Value;
use support::{decode_message, entries, load, text};
use swayconnect_protocol::ops::{self, Operation, TopicState};
use swayconnect_protocol::{
    Apps, Audio, ErrorBody, ErrorCode, InputRefusal, InputTextResult, InvalidMember, JsonObject,
    LauncherOpenResult, Media, Message,
};

/// One topic: its file, its name and the typed state.
struct Topic<T> {
    file: &'static str,
    name: &'static str,
    decode: fn(&JsonObject) -> Result<T, InvalidMember>,
    encode: fn(&T) -> JsonObject,
}

const AUDIO: Topic<Audio> = Topic {
    file: "audio.json",
    name: ops::TOPIC_AUDIO,
    decode: Audio::decode,
    encode: Audio::encode,
};
const MEDIA: Topic<Media> = Topic {
    file: "media.json",
    name: ops::TOPIC_MEDIA,
    decode: Media::decode,
    encode: Media::encode,
};
const APPS: Topic<Apps> = Topic {
    file: "apps.json",
    name: ops::TOPIC_APPS,
    decode: Apps::decode,
    encode: Apps::encode,
};

impl<T: Debug + PartialEq> Topic<T> {
    /// The typed state of a `state.changed` payload, or why it is refused. A payload whose
    /// `data` is not an object is refused by [`TopicState::decode`].
    fn decode_payload(&self, name: &str, payload: &str) -> Result<(JsonObject, T), InvalidMember> {
        let (_, message) = decode_message(name, payload);
        let Message::Evt(evt) = message else {
            panic!("{name}: not an evt");
        };
        assert_eq!(evt.event, ops::EVENT_STATE_CHANGED, "{name}");
        let state = TopicState::decode(&evt.data)?;
        assert_eq!(state.topic, self.name, "{name}");
        let typed = (self.decode)(&state.data)?;
        Ok((state.data, typed))
    }

    fn accepted(&self, name: &str, payload: &str) -> (JsonObject, T) {
        self.decode_payload(name, payload)
            .unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    /// `"ok"` re-encodes to an equal value; `{"normalized"}` decodes to the decoded normal
    /// form, which re-encodes to itself; `"bad"` is refused.
    fn replay_states(&self) -> usize {
        let file = load(self.file);
        let states = entries(&file["states"], "states");
        for vector in states {
            let name = text(vector, "name");
            let payload = text(vector, "payload");
            let expect = &vector["expect"];
            if expect == "bad" {
                let outcome = self.decode_payload(name, payload);
                assert!(outcome.is_err(), "{name}: {outcome:?}");
            } else if expect == "ok" {
                let (data, typed) = self.accepted(name, payload);
                assert_eq!((self.encode)(&typed), data, "{name}");
            } else {
                let (normal, expected) = self.accepted(name, text(expect, "normalized"));
                assert_eq!((self.encode)(&expected), normal, "{name}: normal form");
                assert_eq!(self.accepted(name, payload).1, expected, "{name}");
            }
        }
        states.len()
    }
}

#[test]
fn audio_states() {
    assert!(AUDIO.replay_states() >= 30);
}

#[test]
fn media_states() {
    assert!(MEDIA.replay_states() >= 25);
}

#[test]
fn apps_states() {
    assert!(APPS.replay_states() >= 20);
}

/// Every `server_states` entry is a state the server could publish: it decodes and re-encodes.
#[test]
fn server_states_are_valid_topic_states() {
    fn check<T: Debug + PartialEq>(topic: &Topic<T>) {
        let file = load(topic.file);
        let states = file["server_states"].as_object().expect("server_states");
        for (name, entry) in states {
            let state = entry["state"].as_object().expect("state object");
            let typed = (topic.decode)(state).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(&(topic.encode)(&typed), state, "{name}");
        }
    }
    check(&AUDIO);
    check(&MEDIA);
    check(&APPS);
}

/// The `result` of `op` as a client decodes it. Every other M2b operation answers `{}`, whose
/// members a client ignores.
fn check_result(operation: Operation, result: &JsonObject) -> Result<(), InvalidMember> {
    match operation {
        Operation::LauncherOpen => {
            let decoded = LauncherOpenResult::decode(result)?;
            assert_eq!(LauncherOpenResult::decode(&decoded.encode()), Ok(decoded));
        }
        Operation::InputText => {
            let decoded = InputTextResult::decode(result)?;
            assert_eq!(InputTextResult::decode(&decoded.encode()), Ok(decoded));
        }
        _ => {}
    }
    Ok(())
}

#[test]
fn results() {
    let mut replayed = 0;
    for file in ["audio.json", "media.json", "apps.json", "input.json"] {
        let file = load(file);
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
            replayed += 1;
        }
    }
    assert!(replayed >= 20, "{replayed}");
}

#[test]
fn errors() {
    let mut replayed = 0;
    for file in ["audio.json", "input.json"] {
        let file = load(file);
        for vector in entries(&file["errors"], "errors") {
            let name = text(vector, "name");
            let (_, message) = decode_message(name, text(vector, "payload"));
            let Message::Res(res) = message else {
                panic!("{name}: not a res");
            };
            let error = res.error.unwrap_or_else(|| panic!("{name}: not an error"));
            check_error(name, &error, &vector["expect"]);
            replayed += 1;
        }
    }
    assert!(replayed >= 8, "{replayed}");
}

/// `expect` lists `code` and the `detail` members a client reads.
fn check_error(name: &str, error: &ErrorBody, expect: &Value) {
    assert_eq!(error.code.as_str(), text(expect, "code"), "{name}");
    if error.code == ErrorCode::Forbidden {
        let scope = error.detail.as_ref().and_then(|detail| detail.get("scope"));
        assert_eq!(scope, expect.get("scope"), "{name}");
    }
    let Some(refusal) = InputRefusal::decode(error).unwrap_or_else(|e| panic!("{name}: {e}"))
    else {
        return;
    };
    assert_eq!(
        Value::from(refusal.delivered_chars()),
        expect["delivered_chars"],
        "{name}"
    );
    if let InputRefusal::FocusChanged {
        focused_window_id, ..
    } = refusal
    {
        assert_eq!(
            Value::from(focused_window_id),
            expect["focused_window_id"],
            "{name}"
        );
    }
}
