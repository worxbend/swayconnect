//! Requests of the M2b tests: argument builders, refusals, raw frames for ops the client
//! library will not send, bucket draining and topic states.

use std::time::Duration;

use serde_json::{Value, json};
use swayconnect_cli::client::{ClientError, Event, Session};
use swayconnect_protocol::ops::Args;
use swayconnect_protocol::{AudioChange, EntryId, ErrorBody, ErrorCode, InputTarget, InputText};
use swayconnect_protocol::{JsonObject, Key, KeyPress, MediaCommand};

use crate::support::{self, refusal, soon};

/// No device, player or window has this id.
pub const UNKNOWN_ID: u64 = 999;

pub fn audio(device_id: u64, change: AudioChange) -> Args {
    Args::Audio { device_id, change }
}

pub fn media(command: MediaCommand) -> Args {
    let player_id = UNKNOWN_ID;
    Args::Media { player_id, command }
}

pub fn launch(entry_id: &str) -> Args {
    let entry_id = EntryId::parse(entry_id).expect("entry id");
    Args::AppLaunch {
        entry_id,
        number: None,
    }
}

pub fn type_text(window_id: u64, text: &str) -> Args {
    let target = InputTarget::Window(window_id);
    let text = InputText::parse(text).expect("input text");
    Args::InputText { target, text }
}

pub fn press(window_id: u64, key: &str) -> Args {
    let key = Key::from_wire(key).expect("key name");
    let press = KeyPress::new(key, Vec::new(), 1).expect("key press");
    let target = InputTarget::Window(window_id);
    Args::InputKey { target, press }
}

pub fn op_name(args: &Args) -> &'static str {
    args.operation().as_str()
}

pub async fn ask(session: &mut Session, args: &Args) -> Result<JsonObject, ClientError> {
    soon(op_name(args), session.request(args)).await
}

pub async fn refused(session: &mut Session, args: &Args, code: ErrorCode) -> ErrorBody {
    let error = refusal(ask(session, args).await);
    assert_eq!(error.code, code, "{}: {error:?}", op_name(args));
    error
}

pub fn detail(error: &ErrorBody, member: &str) -> Value {
    let detail = error.detail.as_ref().expect("detail");
    detail.get(member).cloned().unwrap_or(Value::Null)
}

/// A request the client library will not send (its op or topic is not offered), as a raw
/// frame. Returns the `error` member of the response.
pub async fn raw_refusal(
    session: &mut Session,
    id: u64,
    op: &str,
    args: &Value,
    code: ErrorCode,
) -> Value {
    let frame = support::request_frame(id, op, args);
    let answer = support::raw_exchange(session, &frame).await;
    assert_eq!(answer["id"], id, "{answer:?}");
    assert_eq!(answer["error"]["code"], code.as_str(), "{op}: {answer:?}");
    answer["error"].clone()
}

pub async fn refused_unoffered(
    session: &mut Session,
    id: u64,
    args: &Args,
    code: ErrorCode,
) -> Value {
    let request = args.to_req(id);
    let members = request.args.unwrap_or_else(|| json!({}));
    raw_refusal(session, id, &request.op, &members, code).await
}

/// A bucket of 17.14: `burst` tokens, one more every `refill`.
#[derive(Debug, Clone, Copy)]
pub struct Bucket {
    pub burst: usize,
    pub refill: Duration,
}

impl Bucket {
    pub const ACTIONS: Bucket = Bucket::new(5, 500);
    pub const CONTINUOUS: Bucket = Bucket::new(10, 100);
    pub const LAUNCH: Bucket = Bucket::new(3, 5000);
    pub const INPUT: Bucket = Bucket::new(20, 100);
    pub const INPUT_TEXT: Bucket = Bucket::new(4, 1000);

    const fn new(burst: usize, refill_ms: u64) -> Bucket {
        Bucket {
            burst,
            refill: Duration::from_millis(refill_ms),
        }
    }
}

/// Takes tokens until the bucket of `args`'s class runs empty. `args` names an id that
/// does not exist, so every request before that passes step 5 and is `not_found`. A slow
/// machine may see a refill, so at least `bucket.burst` pass.
pub async fn drain(session: &mut Session, args: &Args, bucket: Bucket) {
    let op = op_name(args);
    let mut passed = 0;
    let error = loop {
        assert!(
            passed < bucket.burst * 10,
            "{op}: the bucket never ran empty"
        );
        let error = refusal(ask(session, args).await);
        if error.code == ErrorCode::RateLimited {
            break error;
        }
        assert_eq!(error.code, ErrorCode::NotFound, "{op}: {error:?}");
        passed += 1;
    };
    assert!(passed >= bucket.burst, "{op}: {passed} passed, {bucket:?}");
    let retry_after = detail(&error, "retry_after_ms")
        .as_u64()
        .expect("retry_after_ms");
    let refill_ms = u64::try_from(bucket.refill.as_millis()).expect("small");
    assert!(
        (1..=refill_ms).contains(&retry_after),
        "{op}: retry after {retry_after} ms"
    );
}

/// Subscribes to `topic` and returns the first state that satisfies `wanted`.
pub async fn topic_state(
    session: &mut Session,
    topic: &str,
    wanted: impl Fn(&JsonObject) -> bool,
) -> JsonObject {
    let states = soon("subscribe", session.subscribe(&[topic])).await;
    let [first] = states.expect("subscribed").try_into().expect("one state");
    if wanted(&first.data) {
        return first.data;
    }
    loop {
        match soon("state.changed", session.next_event()).await {
            Ok(Event::State(state)) if state.topic == topic && wanted(&state.data) => {
                return state.data;
            }
            Ok(Event::State(_)) => {}
            other => panic!("not a state event: {other:?}"),
        }
    }
}
