//! Topic states as `state.subscribe` and `state.changed` carry them (`docs/PROTOCOL.md` 7.2,
//! 8.1).

use serde_json::Value;

use crate::json::{self, InvalidMember, object_of};
use crate::message::JsonObject;

/// One topic state with its revision: an entry of the `state.subscribe` result and the `data`
/// of a `state.changed` event (spec 7.2, 8.1).
#[derive(Debug, Clone, PartialEq)]
pub struct TopicState {
    pub topic: String,
    pub rev: u64,
    pub data: JsonObject,
}

impl TopicState {
    pub fn encode(&self) -> JsonObject {
        object_of([
            ("topic", self.topic.as_str().into()),
            ("rev", self.rev.into()),
            ("data", self.data.clone().into()),
        ])
    }

    pub fn decode(object: &JsonObject) -> Result<TopicState, InvalidMember> {
        Ok(TopicState {
            topic: json::string(object, "topic")?.to_owned(),
            rev: json::int(object, "rev")?,
            data: json::object(object, "data")?.clone(),
        })
    }
}

/// The `result` of `state.subscribe`: one entry per requested topic, in request order.
pub fn encode_subscribe_result(states: &[TopicState]) -> JsonObject {
    let states: Vec<Value> = states
        .iter()
        .map(|state| Value::Object(state.encode()))
        .collect();
    object_of([("states", states.into())])
}

pub fn decode_subscribe_result(result: &JsonObject) -> Result<Vec<TopicState>, InvalidMember> {
    json::object_array(result, "states", TopicState::decode)
}
