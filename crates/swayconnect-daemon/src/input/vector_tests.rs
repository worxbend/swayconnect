//! Replays the input cases of `protocol/vectors/v1/ext/input.json` that reach step 8c
//! (`docs/PROTOCOL.md` 17.16): each `guard` entry is set on the fakes before its part, and
//! the result or refusal, the delivered parts and the key presses must match. Cases that
//! stop earlier (decode, scope, read-only, rate, window lookup) belong to dispatch.

use serde_json::{Value, json};
use swayconnect_protocol::ErrorCode;
use swayconnect_protocol::ops::{Args, Operation};

use super::tests::{Grab, Rig, Scene, other_device, this_device};

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../protocol/vectors/v1/ext/input.json"
);
const GUARD_CODES: [ErrorCode; 4] = [
    ErrorCode::Locked,
    ErrorCode::LauncherClosed,
    ErrorCode::KeyboardGrabbed,
    ErrorCode::FocusChanged,
];

/// Entry `part` of a guard member: a list holds one entry per part and its last entry
/// holds for later parts (vectors README).
fn at(value: &Value, part: usize) -> &Value {
    match value.as_array() {
        Some(list) => list.get(part).or(list.last()).expect("a non-empty list"),
        None => value,
    }
}

fn apply(scene: &Scene, guard: &Value, part: usize) {
    let grab = match at(&guard["grabber"], part).as_str() {
        Some("none") => Grab::None,
        Some("launcher") => Grab::Launcher,
        Some("other") => Grab::Other,
        other => panic!("grabber {other:?}"),
    };
    scene.set_grab(grab);
    // Last: unknown lock state removes the whole fake /proc.
    scene.set_locked(at(&guard["locked"], part).as_bool());
    scene.focus(at(&guard["focus"], part).as_u64());
    let launcher = &guard["launcher"];
    let open = at(&launcher["open"], part).as_bool() == Some(true);
    let holder = match launcher["holder"].as_str() {
        Some("other_device") => other_device(),
        _ => this_device(),
    };
    scene.set_launcher(open.then(|| (holder, launcher["focus_window_id"].as_u64())));
}

/// The request's args when the case expects a result or a refusal of step 8c.
fn replayable(vector: &Value) -> Option<(Operation, Args)> {
    let expect = &vector["expect"];
    let reaches_guard = match expect["error"].as_str() {
        None => true,
        Some(code) => ErrorCode::from_wire(code).is_some_and(|code| GUARD_CODES.contains(&code)),
    };
    if !reaches_guard {
        return None;
    }
    let payload: Value =
        serde_json::from_str(vector["payload"].as_str().expect("payload")).expect("JSON");
    let operation = Operation::from_wire(payload["op"].as_str().expect("op")).expect("known op");
    let args = operation
        .decode_args(payload.get("args"))
        .expect("a replayed case decodes");
    Some((operation, args))
}

async fn replay(name: &str, vector: &Value, args: Args) {
    let rig = Rig::new();
    let guard = vector["guard"].clone();
    apply(&rig.scene, &guard, 0);
    let scene = rig.scene.clone();
    rig.keyboard
        .after_delivery(move |delivered| apply(&scene, &guard, delivered));
    let input = rig.service.connection(this_device());
    let outcome = match args {
        Args::InputText { target, text } => input
            .type_text(target, &text)
            .await
            .map(|result| Value::Object(result.encode())),
        Args::InputKey { target, press } => {
            input.press_key(target, &press).await.map(|()| json!({}))
        }
        other => panic!("{name}: not an input op: {other:?}"),
    };
    let expect = &vector["expect"];
    match outcome {
        Ok(result) => assert_eq!(&result, &expect["result"], "{name}"),
        Err(error) => {
            let body = error.error_body();
            assert_eq!(Some(body.code.as_str()), expect["error"].as_str(), "{name}");
            let detail = body.detail.map(Value::Object).unwrap_or(Value::Null);
            assert_eq!(&detail, &expect["detail"], "{name}");
        }
    }
    let delivered = &vector["delivered"];
    match delivered.as_u64() {
        Some(presses) => assert_eq!(rig.keyboard.presses() as u64, presses, "{name}"),
        None => assert_eq!(
            Value::from(rig.keyboard.typed_parts()),
            *delivered,
            "{name}"
        ),
    }
}

#[tokio::test]
async fn guard_cases_of_the_input_vectors_replay_exactly() {
    let text = std::fs::read_to_string(VECTORS).expect("input vectors");
    let file: Value = serde_json::from_str(&text).expect("JSON");
    let mut replayed = 0;
    for vector in file["requests"].as_array().expect("requests") {
        let name = vector["name"].as_str().expect("name");
        let Some((operation, args)) = replayable(vector) else {
            continue;
        };
        let scopes = vector["scopes"].as_array().expect("scopes");
        let scope = operation.required_scope().expect("input ops need a scope");
        assert!(
            scopes.iter().any(|known| known == scope.as_str()),
            "{name}: a replayed case holds its scope"
        );
        replay(name, vector, args).await;
        replayed += 1;
    }
    assert!(replayed >= 60, "only {replayed} cases replayed");
}
