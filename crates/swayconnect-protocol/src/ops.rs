//! Operations, their arguments and results, and event payloads (`docs/PROTOCOL.md` 7, 8, 17).

use serde_json::Value;

use crate::code::wire_names;
use crate::icon::IconHash;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::{MAX_FOCUS_NUMBER, MAX_ICON_HASHES, MAX_TOPICS_PER_SUBSCRIBE};
use crate::message::{JsonObject, Req, Scope};

pub const TOPIC_DESKTOP: &str = "desktop";
pub const EVENT_STATE_CHANGED: &str = "state.changed";
pub const EVENT_PAIRING_CONFIRMED: &str = "pairing.confirmed";

/// The operation table of spec 7.2, and the operations of section 17 this version knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Ping,
    StateSubscribe,
    /// Spec 17.5. Not an action; it has its own rate class `icons`.
    IconGet,
    WorkspaceFocus,
    WindowFocus,
    WindowFullscreen,
}

wire_names!(Operation {
    Ping => "ping",
    StateSubscribe => "state.subscribe",
    IconGet => "icon.get",
    WorkspaceFocus => "workspace.focus",
    WindowFocus => "window.focus",
    WindowFullscreen => "window.fullscreen",
});

/// Validated arguments of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Args {
    Ping,
    StateSubscribe {
        topics: Vec<String>,
    },
    /// 1 to 8 distinct hashes.
    IconGet {
        hashes: Vec<IconHash>,
    },
    WorkspaceFocus(WorkspaceTarget),
    WindowFocus {
        window_id: u64,
    },
    WindowFullscreen {
        window_id: u64,
        enabled: bool,
    },
}

/// Exactly one of the two arguments of `workspace.focus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTarget {
    Id(u64),
    /// 0 to 999.
    Number(u16),
}

impl Operation {
    pub const fn required_scope(self) -> Option<Scope> {
        match self {
            Operation::Ping => None,
            Operation::StateSubscribe | Operation::IconGet => Some(Scope::StateRead),
            Operation::WorkspaceFocus | Operation::WindowFocus | Operation::WindowFullscreen => {
                Some(Scope::Control)
            }
        }
    }

    /// An action changes the desktop.
    pub const fn is_action(self) -> bool {
        matches!(
            self,
            Operation::WorkspaceFocus | Operation::WindowFocus | Operation::WindowFullscreen
        )
    }

    /// Step 6 of the check order (spec 10.2). `args` is `req.args`; absent means `{}`.
    /// An error is answered with `bad_request`. Topic names are not looked up here.
    pub fn decode_args(self, args: Option<&Value>) -> Result<Args, InvalidMember> {
        let empty = JsonObject::new();
        let args = match args {
            None => &empty,
            Some(Value::Object(args)) => args,
            Some(_) => return Err(InvalidMember("args")),
        };
        match self {
            Operation::Ping => Ok(Args::Ping),
            Operation::StateSubscribe => Ok(Args::StateSubscribe {
                topics: decode_topics(args)?,
            }),
            Operation::IconGet => Ok(Args::IconGet {
                hashes: decode_hashes(args)?,
            }),
            Operation::WorkspaceFocus => decode_workspace_target(args).map(Args::WorkspaceFocus),
            Operation::WindowFocus => Ok(Args::WindowFocus {
                window_id: json::int(args, "window_id")?,
            }),
            Operation::WindowFullscreen => Ok(Args::WindowFullscreen {
                window_id: json::int(args, "window_id")?,
                enabled: json::boolean(args, "enabled")?,
            }),
        }
    }
}

impl Args {
    pub const fn operation(&self) -> Operation {
        match self {
            Args::Ping => Operation::Ping,
            Args::StateSubscribe { .. } => Operation::StateSubscribe,
            Args::IconGet { .. } => Operation::IconGet,
            Args::WorkspaceFocus(_) => Operation::WorkspaceFocus,
            Args::WindowFocus { .. } => Operation::WindowFocus,
            Args::WindowFullscreen { .. } => Operation::WindowFullscreen,
        }
    }

    /// The request a client sends for these arguments.
    pub fn to_req(&self, id: u64) -> Req {
        Req {
            id,
            op: self.operation().as_str().to_owned(),
            args: self.encode().map(Value::Object),
        }
    }

    fn encode(&self) -> Option<JsonObject> {
        match self {
            Args::Ping => None,
            Args::StateSubscribe { topics } => Some(object_of([("topics", topics.clone().into())])),
            Args::IconGet { hashes } => {
                let hashes: Vec<Value> = hashes.iter().map(|hash| hash.encode_value()).collect();
                Some(object_of([("hashes", hashes.into())]))
            }
            Args::WorkspaceFocus(WorkspaceTarget::Id(id)) => {
                Some(object_of([("workspace_id", (*id).into())]))
            }
            Args::WorkspaceFocus(WorkspaceTarget::Number(number)) => {
                Some(object_of([("number", (*number).into())]))
            }
            Args::WindowFocus { window_id } => {
                Some(object_of([("window_id", (*window_id).into())]))
            }
            Args::WindowFullscreen { window_id, enabled } => Some(object_of([
                ("window_id", (*window_id).into()),
                ("enabled", (*enabled).into()),
            ])),
        }
    }
}

/// 1 to 16 topic names, no duplicates.
fn decode_topics(args: &JsonObject) -> Member<Vec<String>> {
    const NAME: &str = "topics";
    let topics = json::string_array(args, NAME)?;
    if (1..=MAX_TOPICS_PER_SUBSCRIBE).contains(&topics.len()) && !has_duplicate(&topics) {
        Ok(topics)
    } else {
        Err(InvalidMember(NAME))
    }
}

/// 1 to 8 distinct canonical hashes (17.5). A string that is not one is a bad request; a
/// well-formed hash the server never issued is answered in `missing`.
fn decode_hashes(args: &JsonObject) -> Member<Vec<IconHash>> {
    const NAME: &str = "hashes";
    let hashes = json::array(args, NAME)?
        .iter()
        .map(|hash| IconHash::decode_value(hash, NAME))
        .collect::<Member<Vec<IconHash>>>()?;
    if (1..=MAX_ICON_HASHES).contains(&hashes.len()) && !has_duplicate(&hashes) {
        Ok(hashes)
    } else {
        Err(InvalidMember(NAME))
    }
}

fn has_duplicate<T: PartialEq>(entries: &[T]) -> bool {
    entries
        .iter()
        .enumerate()
        .any(|(index, entry)| entries[..index].contains(entry))
}

fn decode_workspace_target(args: &JsonObject) -> Member<WorkspaceTarget> {
    match (args.get("workspace_id"), args.get("number")) {
        (Some(id), None) => json::as_int(id)
            .map(WorkspaceTarget::Id)
            .ok_or(InvalidMember("workspace_id")),
        (None, Some(number)) => json::as_int(number)
            .and_then(|number| u16::try_from(number).ok())
            .filter(|number| *number <= MAX_FOCUS_NUMBER)
            .map(WorkspaceTarget::Number)
            .ok_or(InvalidMember("number")),
        (Some(_), Some(_)) | (None, None) => Err(InvalidMember("workspace_id")),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(text: &str) -> Value {
        serde_json::from_str(text).expect("test JSON")
    }

    #[test]
    fn absent_args_mean_an_empty_object() {
        assert_eq!(Operation::Ping.decode_args(None), Ok(Args::Ping));
        assert!(Operation::WindowFocus.decode_args(None).is_err());
    }

    #[test]
    fn args_that_are_not_an_object_are_refused_for_every_operation() {
        for bad in ["null", "5", "[]", "\"x\""] {
            assert_eq!(
                Operation::Ping.decode_args(Some(&args_of(bad))),
                Err(InvalidMember("args")),
                "{bad}"
            );
        }
    }

    #[test]
    fn subscribe_topics_limits() {
        let decode = |text: &str| Operation::StateSubscribe.decode_args(Some(&args_of(text)));
        assert!(decode(r#"{"topics":["desktop"]}"#).is_ok());
        let sixteen: Vec<String> = (0..16).map(|index| format!("t{index}")).collect();
        let seventeen: Vec<String> = (0..17).map(|index| format!("t{index}")).collect();
        assert!(decode(&serde_json::json!({ "topics": sixteen }).to_string()).is_ok());
        assert!(decode(&serde_json::json!({ "topics": seventeen }).to_string()).is_err());
        for bad in [
            r#"{}"#,
            r#"{"topics":[]}"#,
            r#"{"topics":["desktop","desktop"]}"#,
            r#"{"topics":[1]}"#,
            r#"{"topics":"desktop"}"#,
            r#"{"topics":null}"#,
        ] {
            assert!(decode(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn workspace_number_range_and_types() {
        let decode = |text: &str| Operation::WorkspaceFocus.decode_args(Some(&args_of(text)));
        let number = |number| Ok(Args::WorkspaceFocus(WorkspaceTarget::Number(number)));
        assert_eq!(decode(r#"{"number":0}"#), number(0));
        assert_eq!(decode(r#"{"number":999}"#), number(999));
        for bad in [
            r#"{"number":1000}"#,
            r#"{"number":-1}"#,
            r#"{"number":2.0}"#,
            r#"{"number":1e2}"#,
            r#"{"number":"2"}"#,
            r#"{"number":null}"#,
            r#"{"number":2,"workspace_id":null}"#,
            r#"{"workspace_id":9007199254740992}"#,
        ] {
            assert!(decode(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn icon_hashes_count_duplicates_and_form() {
        let decode = |hashes: Value| {
            Operation::IconGet.decode_args(Some(&serde_json::json!({ "hashes": hashes })))
        };
        let hash = |index: u8| IconHash::of(&[index]).to_string();
        let eight: Vec<String> = (0..8).map(hash).collect();
        let nine: Vec<String> = (0..9).map(hash).collect();
        assert!(decode(eight.into()).is_ok());
        let refused = [
            nine.into(),
            Value::Array(Vec::new()),
            serde_json::json!([hash(1), hash(1)]),
            serde_json::json!(["firefox"]),
            serde_json::json!([null]),
            Value::Null,
            hash(1).into(),
        ];
        for bad in refused {
            assert_eq!(decode(bad.clone()), Err(InvalidMember("hashes")), "{bad}");
        }
        assert!(Operation::IconGet.decode_args(None).is_err());
    }

    #[test]
    fn fullscreen_needs_a_real_boolean() {
        let decode = |text: &str| Operation::WindowFullscreen.decode_args(Some(&args_of(text)));
        assert!(decode(r#"{"window_id":5,"enabled":false}"#).is_ok());
        assert!(decode(r#"{"window_id":5,"enabled":0}"#).is_err());
        assert!(decode(r#"{"window_id":5}"#).is_err());
    }

    #[test]
    fn every_args_value_survives_request_encoding() {
        let all = [
            Args::Ping,
            Args::StateSubscribe {
                topics: vec![TOPIC_DESKTOP.to_owned()],
            },
            Args::IconGet {
                hashes: vec![IconHash::of(b"a"), IconHash::of(b"b")],
            },
            Args::WorkspaceFocus(WorkspaceTarget::Id(9)),
            Args::WorkspaceFocus(WorkspaceTarget::Number(2)),
            Args::WindowFocus { window_id: 5 },
            Args::WindowFullscreen {
                window_id: 5,
                enabled: true,
            },
        ];
        for args in all {
            let req = args.to_req(1);
            let operation = Operation::from_wire(&req.op).expect("known operation");
            assert_eq!(operation.decode_args(req.args.as_ref()), Ok(args));
        }
    }

    #[test]
    fn scopes_and_actions_follow_the_operation_table() {
        assert_eq!(Operation::Ping.required_scope(), None);
        assert_eq!(
            Operation::StateSubscribe.required_scope(),
            Some(Scope::StateRead)
        );
        assert_eq!(Operation::IconGet.required_scope(), Some(Scope::StateRead));
        for read in [
            Operation::Ping,
            Operation::StateSubscribe,
            Operation::IconGet,
        ] {
            assert!(!read.is_action(), "{read:?}");
        }
        for action in [
            Operation::WorkspaceFocus,
            Operation::WindowFocus,
            Operation::WindowFullscreen,
        ] {
            assert_eq!(action.required_scope(), Some(Scope::Control));
            assert!(action.is_action());
        }
    }
}
