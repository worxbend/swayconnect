//! Replays the `requests` of `protocol/vectors/v1/ext/{audio,media,apps,input}.json`
//! (`docs/PROTOCOL.md` 17.9 to 17.13). Formats: `protocol/vectors/v1/README.md`.
//!
//! Steps 2 to 6 of spec 10.2 are replayed as in `ext_m2a.rs`, with the rate classes of 17.14,
//! the server limit `max_volume_pct` and the topic scopes of 17.2. For audio, media and apps
//! the id lookup of step 8 and the extra checks that this crate's rules decide (the capture
//! rule, `launch.risky`, `unsupported_target`) are replayed against `server_states`. For input
//! the parts of 17.13 rule 1 are compared with `delivered`. The guards of 17.13 rule 2, the
//! backends, the spawns and the sway commands belong to the daemon tests.

mod support;

use serde_json::Value;
use support::{decode_message, entries, load, text};
use swayconnect_protocol::ops::{self, Args, Operation, RateClass};
use swayconnect_protocol::{
    Apps, Audio, ErrorCode, InputText, JsonObject, Media, Message, Req, Scope,
};

/// The codes of steps 2 to 6 of spec 10.2.
const EARLY_CODES: [ErrorCode; 6] = [
    ErrorCode::UnknownOp,
    ErrorCode::Forbidden,
    ErrorCode::ReadOnly,
    ErrorCode::RateLimited,
    ErrorCode::BadRequest,
    ErrorCode::UnknownTopic,
];

/// A refusal: its code and, for `forbidden`, the scope `detail.scope` names.
type Refusal = (ErrorCode, Option<Scope>);

/// What the request expects, read as `ext/requests.json` rule 6.
struct Expected {
    code: Option<ErrorCode>,
    scope: Option<Scope>,
}

impl Expected {
    fn of(name: &str, expect: &Value) -> Expected {
        let Some(code) = expect.get("error") else {
            return Expected {
                code: None,
                scope: None,
            };
        };
        let code = code.as_str().expect("error code");
        let scope = expect["detail"]["scope"].as_str().map(known_scope);
        Expected {
            code: Some(ErrorCode::from_wire(code).unwrap_or_else(|| panic!("{name}: {code}"))),
            scope,
        }
    }

    /// A refusal of steps 2 to 6. `forbidden` for `audio.capture` or `launch.risky` comes
    /// after step 8 (17.16 steps 8a, 8b).
    fn is_early(&self) -> bool {
        let is_late_scope = matches!(self.scope, Some(Scope::AudioCapture | Scope::LaunchRisky));
        self.code
            .is_some_and(|code| EARLY_CODES.contains(&code) && !is_late_scope)
    }

    fn assert_refused(&self, name: &str, (code, scope): Refusal) {
        assert_eq!(Some(code), self.code, "{name}");
        if code == ErrorCode::Forbidden {
            assert_eq!(scope, self.scope, "{name}: detail.scope");
        }
    }
}

fn known_scope(name: &str) -> Scope {
    Scope::from_wire(name).unwrap_or_else(|| panic!("unknown scope {name}"))
}

fn scopes(vector: &Value) -> Vec<Scope> {
    vector["scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .map(|scope| known_scope(scope.as_str().expect("scope name")))
        .collect()
}

/// The `buckets` key of a rate class of 17.14.
fn bucket_name(class: RateClass) -> &'static str {
    match class {
        RateClass::Actions => "actions",
        RateClass::Icons => "icons",
        RateClass::Continuous => "continuous",
        RateClass::Launch => "launch",
        RateClass::Input => "input",
        RateClass::InputText => "input_text",
    }
}

fn is_topic(topic: &str) -> bool {
    [
        ops::TOPIC_DESKTOP,
        ops::TOPIC_SESSION,
        ops::TOPIC_AUDIO,
        ops::TOPIC_MEDIA,
        ops::TOPIC_APPS,
    ]
    .contains(&topic)
}

/// Steps 2 to 6 of spec 10.2, then the topic scopes of 17.2.
fn first_steps(vector: &Value, req: &Req, max_volume_pct: u16) -> Result<Args, Refusal> {
    let operation = Operation::from_wire(&req.op).ok_or((ErrorCode::UnknownOp, None))?;
    let scopes = scopes(vector);
    let forbidden = |scope| (ErrorCode::Forbidden, Some(scope));
    if let Some(scope) = operation.required_scope().filter(|s| !scopes.contains(s)) {
        return Err(forbidden(scope));
    }
    if vector["read_only"] == true && operation.is_action() {
        return Err((ErrorCode::ReadOnly, None));
    }
    let class = operation.rate_class().map(bucket_name);
    if class.is_some_and(|class| vector["buckets"][class] == 0) {
        return Err((ErrorCode::RateLimited, None));
    }
    let bad_request = |_| (ErrorCode::BadRequest, None);
    let args = operation
        .decode_args(req.args.as_ref())
        .map_err(bad_request)?;
    match args {
        Args::Audio { device_id, change } => Ok(Args::Audio {
            device_id,
            change: change
                .check_max_volume(max_volume_pct)
                .map_err(bad_request)?,
        }),
        Args::StateSubscribe { topics } => {
            if topics.iter().any(|topic| !is_topic(topic)) {
                return Err((ErrorCode::UnknownTopic, None));
            }
            match topics
                .iter()
                .filter_map(|topic| ops::topic_scope(topic))
                .find(|s| !scopes.contains(s))
            {
                Some(scope) => Err(forbidden(scope)),
                None => Ok(Args::StateSubscribe { topics }),
            }
        }
        args => Ok(args),
    }
}

/// The server state a case names in `member`, from `server_states`.
fn server_state<'a>(file: &'a Value, vector: &Value, member: &str) -> &'a JsonObject {
    let key = text(vector, member);
    file["server_states"][key]["state"]
        .as_object()
        .unwrap_or_else(|| panic!("server state {key}"))
}

/// Step 8 and the checks after it that this crate's rules decide. `None` when they pass.
fn later_steps(file: &Value, vector: &Value, args: &Args) -> Option<Refusal> {
    let scopes = scopes(vector);
    let forbidden = |scope| Some((ErrorCode::Forbidden, Some(scope)));
    match args {
        Args::Audio { device_id, change } => {
            let audio = Audio::decode(server_state(file, vector, "audio")).expect("audio state");
            let is_id = |device: &swayconnect_protocol::Device| device.id == *device_id;
            let is_source = audio.sources.iter().any(is_id);
            if !is_source && !audio.sinks.iter().any(is_id) {
                return Some((ErrorCode::NotFound, None));
            }
            let needs_capture = is_source && change.needs_capture_on_source();
            if needs_capture && !scopes.contains(&Scope::AudioCapture) {
                return forbidden(Scope::AudioCapture);
            }
            None
        }
        Args::Media { player_id, command } => {
            let media = Media::decode(server_state(file, vector, "media")).expect("media state");
            match media.players.iter().find(|player| player.id == *player_id) {
                None => Some((ErrorCode::NotFound, None)),
                Some(player) if !player.allows(*command) => {
                    Some((ErrorCode::UnsupportedTarget, None))
                }
                Some(_) => None,
            }
        }
        Args::AppLaunch { entry_id, .. } => {
            let apps = Apps::decode(server_state(file, vector, "apps")).expect("apps state");
            match apps
                .apps
                .iter()
                .find(|app| app.entry_id == entry_id.as_str())
            {
                None => Some((ErrorCode::NotFound, None)),
                Some(app) if app.risky && !scopes.contains(&Scope::LaunchRisky) => {
                    forbidden(Scope::LaunchRisky)
                }
                Some(_) => None,
            }
        }
        _ => None,
    }
}

/// `delivered` holds the parts handed to the keyboard: a prefix of the parts of 17.13 rule 1,
/// all of them when the request succeeds. `delivered_chars`, where the answer has it, counts
/// their scalar values; an answer without it (`not_found`) delivered nothing.
fn check_delivered(name: &str, vector: &Value, text: &InputText) {
    let parts: Vec<&str> = text.parts().collect();
    let delivered: Vec<&str> = vector["delivered"]
        .as_array()
        .expect("delivered parts")
        .iter()
        .map(|part| part.as_str().expect("part"))
        .collect();
    assert!(
        parts.starts_with(&delivered),
        "{name}: {delivered:?} of {parts:?}"
    );
    let chars: usize = delivered.iter().map(|part| part.chars().count()).sum();
    let expect = &vector["expect"];
    let reported = expect["result"]["delivered_chars"]
        .as_u64()
        .or_else(|| expect["detail"]["delivered_chars"].as_u64());
    match reported {
        Some(reported) => assert_eq!(u64::try_from(chars), Ok(reported), "{name}"),
        None => assert!(delivered.is_empty(), "{name}: delivered without a count"),
    }
    if expect.get("result").is_some() {
        assert_eq!(delivered, parts, "{name}: every part");
    }
}

/// Replays one file; returns how many cases were refused at steps 2 to 6 and how many passed.
fn replay(file_name: &str) -> (usize, usize) {
    let file = load(file_name);
    let max_volume_pct = file["config"]["max_volume_pct"]
        .as_u64()
        .map_or(100, |max| u16::try_from(max).expect("max_volume_pct"));
    let (mut refused_early, mut passed) = (0, 0);
    for vector in entries(&file["requests"], "requests") {
        let name = text(vector, "name");
        assert_eq!(vector["sway_online"], true, "{name}");
        let (_, message) = decode_message(name, text(vector, "payload"));
        let Message::Req(req) = message else {
            panic!("{name}: not a req");
        };
        let expected = Expected::of(name, &vector["expect"]);
        match first_steps(vector, &req, max_volume_pct) {
            Err(refusal) => {
                expected.assert_refused(name, refusal);
                refused_early += 1;
            }
            Ok(args) => {
                assert!(
                    !expected.is_early(),
                    "{name}: passes steps 2 to 6, but expects {:?}",
                    expected.code
                );
                let again = args.to_req(req.id);
                assert_eq!(
                    args.operation().decode_args(again.args.as_ref()),
                    Ok(args.clone()),
                    "{name}"
                );
                check_later(name, &file, vector, &args, &expected);
                passed += 1;
            }
        }
    }
    (refused_early, passed)
}

fn check_later(name: &str, file: &Value, vector: &Value, args: &Args, expected: &Expected) {
    match later_steps(file, vector, args) {
        Some(refusal) => expected.assert_refused(name, refusal),
        None => {
            let decided = [
                ErrorCode::NotFound,
                ErrorCode::UnsupportedTarget,
                ErrorCode::Forbidden,
            ];
            let is_input = matches!(args, Args::InputText { .. } | Args::InputKey { .. });
            assert!(
                is_input || expected.code.is_none_or(|code| !decided.contains(&code)),
                "{name}: passes step 8, but expects {:?}",
                expected.code
            );
        }
    }
    if let Args::InputText { text, .. } = args {
        check_delivered(name, vector, text);
    }
}

#[test]
fn audio_requests() {
    let (refused_early, passed) = replay("audio.json");
    assert!(
        refused_early >= 25 && passed >= 20,
        "{refused_early} / {passed}"
    );
}

#[test]
fn media_requests() {
    let (refused_early, passed) = replay("media.json");
    assert!(
        refused_early >= 10 && passed >= 15,
        "{refused_early} / {passed}"
    );
}

#[test]
fn apps_requests() {
    let (refused_early, passed) = replay("apps.json");
    assert!(
        refused_early >= 20 && passed >= 20,
        "{refused_early} / {passed}"
    );
}

#[test]
fn input_requests() {
    let (refused_early, passed) = replay("input.json");
    assert!(
        refused_early >= 80 && passed >= 80,
        "{refused_early} / {passed}"
    );
}
