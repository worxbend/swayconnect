//! Operation table and request checks, in the order of `docs/PROTOCOL.md` 10.2.
//!
//! Steps 2 to 6 are decided here, with the rate classes of 17.14 and the additions of 17.16.
//! A sway action that passes them leaves as [`Decision::Act`]; steps 7 to 9 belong to the sway
//! half (`actions.rs`, `supervisor.rs`). Keep awake, lock and the M2b operations leave as
//! their own decisions; their services do step 8 and the checks 8a to 8c of 17.16.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;
use swayconnect_protocol::limits::MAX_INT;
use swayconnect_protocol::ops::{
    self, Args, KeepAwake, Operation, TOPIC_APPS, TOPIC_AUDIO, TOPIC_DESKTOP, TOPIC_MEDIA,
    TOPIC_SESSION, WorkspaceTarget,
};
use swayconnect_protocol::{
    AudioChange, DeviceId, EntryId, ErrorBody, ErrorCode, IconHash, InputTarget, InputText,
    JsonObject, KeyPress, MediaCommand, Req, Scope,
};

use crate::security::Security;
use crate::sway_handle::Action;
use crate::throttle::RateClass;

mod refusals;

pub use self::refusals::{
    action_refusal, audio_refusal, keep_awake_refusal, launch_refusal, launcher_refusal,
    lock_refusal, media_refusal, read_only_refusal, sway_unavailable, volume_refusal,
};

/// In the order of `welcome.ops`: the version 1 operations first, `icon.get` after them,
/// then the M2a operations of spec 17.6 to 17.8, then the M2b ones of 17.9 to 17.13.
const SERVED_OPERATIONS: [Operation; 24] = [
    Operation::Ping,
    Operation::StateSubscribe,
    Operation::WorkspaceFocus,
    Operation::WindowFocus,
    Operation::WindowFullscreen,
    Operation::IconGet,
    Operation::WindowMove,
    Operation::WindowFloating,
    Operation::WindowLayout,
    Operation::WindowScratchpad,
    Operation::ScratchpadShow,
    Operation::SessionLock,
    Operation::WindowClose,
    Operation::KeepAwakeSet,
    Operation::AudioSetVolume,
    Operation::AudioSetMute,
    Operation::AudioSetDefault,
    Operation::MediaPlayPause,
    Operation::MediaNext,
    Operation::MediaPrevious,
    Operation::AppLaunch,
    Operation::LauncherOpen,
    Operation::InputText,
    Operation::InputKey,
];

/// Every scope this version knows (`welcome.known_scopes`), in the order of spec 17.2.
const KNOWN_SCOPES: [Scope; 12] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Control,
    Scope::IdleInhibit,
    Scope::Destructive,
    Scope::Audio,
    Scope::AudioCapture,
    Scope::Media,
    Scope::Launch,
    Scope::LaunchRisky,
    Scope::InputKeys,
    Scope::InputText,
];

/// Every topic, in the order of `welcome.topics`.
const TOPICS: [Topic; 5] = [
    Topic::Desktop,
    Topic::Session,
    Topic::Audio,
    Topic::Media,
    Topic::Apps,
];

/// What the owner configured beyond the fixed operation table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// `[session] lock` is set, so `session.lock` is served (spec 17.6 rule 1).
    pub lock: bool,
}

impl Features {
    fn serves(self, operation: Operation) -> bool {
        SERVED_OPERATIONS.contains(&operation) && (operation != Operation::SessionLock || self.lock)
    }

    fn served(self) -> impl Iterator<Item = Operation> {
        SERVED_OPERATIONS
            .into_iter()
            .filter(move |operation| self.serves(*operation))
    }
}

/// A topic of `state.subscribe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Desktop,
    /// Spec 17.8, built per device.
    Session,
    /// Spec 17.9 to 17.11.
    Audio,
    Media,
    Apps,
}

impl Topic {
    fn from_wire(name: &str) -> Option<Topic> {
        TOPICS.into_iter().find(|topic| topic.as_str() == name)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Topic::Desktop => TOPIC_DESKTOP,
            Topic::Session => TOPIC_SESSION,
            Topic::Audio => TOPIC_AUDIO,
            Topic::Media => TOPIC_MEDIA,
            Topic::Apps => TOPIC_APPS,
        }
    }

    /// Every topic needs `state.read`; these also need their own scope (spec 17.2).
    fn is_granted(self, scopes: &BTreeSet<Scope>) -> bool {
        let extra = ops::topic_scope(self.as_str());
        scopes.contains(&Scope::StateRead) && extra.is_none_or(|scope| scopes.contains(&scope))
    }
}

/// What the session does with one request.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// The device was revoked since it authenticated: `bye` with `revoked`.
    Revoked,
    Refuse(ErrorBody),
    Pong,
    /// The topics in request order, each once (the protocol crate refuses duplicates).
    Subscribe(Vec<Topic>),
    /// Answer `icon.get` from the icon store, in request order.
    Icons(Vec<IconHash>),
    /// Hand the action to the sway half; answer with its outcome.
    Act(Action),
    /// `keep_awake.set`. The upper end of `duration_s` is checked by the keep-awake task,
    /// which owns `max_duration_s` (still step 6: nothing runs in between).
    KeepAwake(KeepAwake),
    /// `session.lock`.
    Lock,
    /// `audio.*`. The upper end of `volume_pct` is checked against the audio service's
    /// `max_volume_pct` before anything else (still step 6).
    Audio {
        device_id: u64,
        change: AudioChange,
    },
    /// `media.*`.
    Media {
        player_id: u64,
        command: MediaCommand,
    },
    /// `app.launch`.
    Launch {
        entry_id: EntryId,
        number: Option<u16>,
    },
    /// `launcher.open`.
    OpenLauncher,
    /// `input.text`. Its characters were taken from the device's budget per minute.
    Type {
        target: InputTarget,
        text: InputText,
    },
    /// `input.key`.
    Key {
        target: InputTarget,
        press: KeyPress,
    },
}

/// Who sends the request, and what the daemon allows right now.
pub struct Caller<'a> {
    pub security: &'a Security,
    pub device: &'a DeviceId,
    pub scopes: &'a BTreeSet<Scope>,
    pub is_read_only: bool,
    pub features: Features,
}

/// `welcome.ops` for a device with `scopes`. A read-only daemon offers no action.
pub fn offered_ops(
    scopes: &BTreeSet<Scope>,
    is_read_only: bool,
    features: Features,
) -> Vec<String> {
    features
        .served()
        .filter(|operation| is_granted(*operation, scopes))
        .filter(|operation| !(is_read_only && operation.is_action()))
        .map(|operation| operation.as_str().to_owned())
        .collect()
}

/// `welcome.known_ops`: the operations a device with every scope gets when the daemon is not
/// read-only (spec 17.3).
pub fn known_ops(features: Features) -> Vec<String> {
    features
        .served()
        .map(|operation| operation.as_str().to_owned())
        .collect()
}

/// `welcome.known_scopes`.
pub fn known_scopes() -> Vec<String> {
    KNOWN_SCOPES
        .into_iter()
        .map(|scope| scope.as_str().to_owned())
        .collect()
}

/// The rate class of spec 17.14 whose bucket `operation` takes a token from (step 5): the
/// protocol crate's table, as the daemon's bucket kinds.
pub const fn rate_class(operation: Operation) -> Option<RateClass> {
    match operation.rate_class() {
        None => None,
        Some(ops::RateClass::Actions) => Some(RateClass::Actions),
        Some(ops::RateClass::Icons) => Some(RateClass::Icons),
        Some(ops::RateClass::Continuous) => Some(RateClass::Continuous),
        Some(ops::RateClass::Launch) => Some(RateClass::Launch),
        Some(ops::RateClass::Input) => Some(RateClass::Input),
        Some(ops::RateClass::InputText) => Some(RateClass::InputText),
    }
}

/// `welcome.topics` for a device with `scopes`: every topic needs `state.read`, `audio`,
/// `media` and `apps` also their own scope.
pub fn offered_topics(scopes: &BTreeSet<Scope>) -> Vec<String> {
    TOPICS
        .into_iter()
        .filter(|topic| topic.is_granted(scopes))
        .map(|topic| topic.as_str().to_owned())
        .collect()
}

pub fn decide(caller: &Caller<'_>, request: &Req) -> Decision {
    let device = caller.device;
    if !caller.security.is_active(device) {
        return Decision::Revoked;
    }
    let take_token = |class| caller.security.take_token(device, class);
    let mode = Mode {
        is_read_only: caller.is_read_only,
        features: caller.features,
    };
    let decision = check(caller.scopes, mode, take_token, request)
        .and_then(|decision| spend_text_budget(caller, decision))
        .unwrap_or_else(Decision::Refuse);
    // `take_token` logs an empty action bucket itself. A request is never logged as a whole:
    // its arguments can hold typed text.
    match &decision {
        Decision::Act(action) => {
            tracing::info!(device = device.as_str(), ?action, "action accepted");
        }
        Decision::KeepAwake(args) => {
            tracing::info!(
                device = device.as_str(),
                ?args,
                "keep-awake request accepted"
            );
        }
        Decision::Lock => tracing::info!(device = device.as_str(), "lock request accepted"),
        Decision::Audio { .. }
        | Decision::Media { .. }
        | Decision::Launch { .. }
        | Decision::OpenLauncher
        | Decision::Type { .. }
        | Decision::Key { .. } => {
            // A served operation name by now, not free client text.
            tracing::info!(
                device = device.as_str(),
                op = request.op,
                "request accepted"
            );
        }
        _ => {}
    }
    decision
}

/// The second limit of class `input_text` (spec 17.14), once the text is known to be valid.
fn spend_text_budget(caller: &Caller<'_>, decision: Decision) -> Result<Decision, ErrorBody> {
    if let Decision::Type { text, .. } = &decision {
        let chars = text.as_str().chars().count();
        caller
            .security
            .take_text_budget(caller.device, chars)
            .map_err(|retry_after| rate_limited(RateClass::InputText, retry_after))?;
    }
    Ok(decision)
}

/// `unknown_op` (step 2): not served by this daemon, as configured.
pub fn unknown_op() -> ErrorBody {
    refusal(ErrorCode::UnknownOp, "unknown operation", None)
}

fn is_granted(operation: Operation, scopes: &BTreeSet<Scope>) -> bool {
    operation
        .required_scope()
        .is_none_or(|scope| scopes.contains(&scope))
}

/// The daemon's state that steps 2 and 4 read.
#[derive(Debug, Clone, Copy)]
struct Mode {
    is_read_only: bool,
    features: Features,
}

/// Steps 2 to 6 of spec 10.2. `take_token` is called for an operation with a rate class
/// that reaches step 5, and only then.
fn check(
    scopes: &BTreeSet<Scope>,
    mode: Mode,
    take_token: impl FnOnce(RateClass) -> Result<(), Duration>,
    request: &Req,
) -> Result<Decision, ErrorBody> {
    let Mode {
        is_read_only,
        features,
    } = mode;
    let operation = Operation::from_wire(&request.op)
        .filter(|operation| features.serves(*operation))
        .ok_or_else(unknown_op)?;
    if let Some(scope) = operation
        .required_scope()
        .filter(|scope| !scopes.contains(scope))
    {
        return Err(forbidden(scope));
    }
    if is_read_only && operation.is_action() {
        return Err(read_only_refusal());
    }
    if let Some(class) = rate_class(operation) {
        take_token(class).map_err(|retry_after| rate_limited(class, retry_after))?;
    }
    let args = operation
        .decode_args(request.args.as_ref())
        .map_err(|_| refusal(ErrorCode::BadRequest, "invalid arguments", None))?;
    match args {
        Args::Ping => Ok(Decision::Pong),
        Args::IconGet { hashes } => Ok(Decision::Icons(hashes)),
        Args::StateSubscribe { topics } => subscription(scopes, &topics).map(Decision::Subscribe),
        Args::WorkspaceFocus(WorkspaceTarget::Number(number)) => {
            Ok(Decision::Act(Action::FocusWorkspaceNumber(number)))
        }
        Args::WorkspaceFocus(WorkspaceTarget::Id(id)) => {
            Ok(Decision::Act(Action::FocusWorkspaceId(id)))
        }
        Args::WindowFocus { window_id } => Ok(Decision::Act(Action::FocusWindow(window_id))),
        Args::WindowFullscreen { window_id, enabled } => {
            Ok(Decision::Act(Action::Fullscreen { window_id, enabled }))
        }
        Args::WindowMove { window_id, target } => Ok(Decision::Act(Action::MoveWindow {
            window_id,
            to: target,
        })),
        Args::WindowFloating { window_id, enabled } => {
            Ok(Decision::Act(Action::Floating { window_id, enabled }))
        }
        Args::WindowLayout { window_id, layout } => {
            Ok(Decision::Act(Action::Layout { window_id, layout }))
        }
        Args::WindowScratchpad { window_id } => {
            Ok(Decision::Act(Action::MoveToScratchpad(window_id)))
        }
        Args::ScratchpadShow { window_id } => Ok(Decision::Act(Action::ScratchpadShow(window_id))),
        Args::WindowClose { window_id } => Ok(Decision::Act(Action::Close(window_id))),
        Args::SessionLock => Ok(Decision::Lock),
        Args::KeepAwakeSet(args) => Ok(Decision::KeepAwake(args)),
        Args::Audio { device_id, change } => Ok(Decision::Audio { device_id, change }),
        Args::Media { player_id, command } => Ok(Decision::Media { player_id, command }),
        Args::AppLaunch { entry_id, number } => Ok(Decision::Launch { entry_id, number }),
        Args::LauncherOpen => Ok(Decision::OpenLauncher),
        Args::InputText { target, text } => Ok(Decision::Type { target, text }),
        Args::InputKey { target, press } => Ok(Decision::Key { target, press }),
    }
}

/// The topics of `state.subscribe`: every name must be known (`unknown_topic`), then each
/// topic's own scope held (`forbidden`, spec 17.2), both over the whole list in that order.
fn subscription(scopes: &BTreeSet<Scope>, names: &[String]) -> Result<Vec<Topic>, ErrorBody> {
    let topics = names
        .iter()
        .map(|name| {
            Topic::from_wire(name).ok_or_else(|| {
                refusal(
                    ErrorCode::UnknownTopic,
                    "unknown topic",
                    detail("topic", name.as_str()),
                )
            })
        })
        .collect::<Result<Vec<Topic>, _>>()?;
    let missing = topics
        .iter()
        .find_map(|topic| ops::topic_scope(topic.as_str()).filter(|scope| !scopes.contains(scope)));
    match missing {
        Some(scope) => Err(forbidden(scope)),
        None => Ok(topics),
    }
}

/// `rate_limited` for an empty bucket of `class` (step 5).
fn rate_limited(class: RateClass, retry_after: Duration) -> ErrorBody {
    // At least 1: a client that waits 0 ms would be refused again.
    let retry_after_ms =
        u64::try_from(retry_after.as_millis()).map_or(MAX_INT, |millis| millis.clamp(1, MAX_INT));
    let message = match class {
        RateClass::Actions => "too many actions from this device",
        RateClass::Icons => "too many icon requests from this device",
        RateClass::Continuous => "too many volume changes from this device",
        RateClass::Launch => "too many launches from this device",
        RateClass::Input => "too many key presses from this device",
        RateClass::InputText => "too much typed text from this device",
    };
    refusal(
        ErrorCode::RateLimited,
        message,
        detail("retry_after_ms", retry_after_ms),
    )
}

/// `forbidden` naming the missing scope (step 3, and 8a/8b of spec 17.16).
fn forbidden(scope: Scope) -> ErrorBody {
    refusal(
        ErrorCode::Forbidden,
        "the device lacks the scope",
        detail("scope", scope.as_str()),
    )
}

fn refusal(code: ErrorCode, message: &str, detail: Option<JsonObject>) -> ErrorBody {
    ErrorBody {
        code,
        message: message.to_owned(),
        detail,
    }
}

fn detail(member: &str, content: impl Into<Value>) -> Option<JsonObject> {
    let mut detail = JsonObject::new();
    detail.insert(member.to_owned(), content.into());
    Some(detail)
}

#[cfg(test)]
mod ext_tests;
#[cfg(test)]
mod m2a_tests;
#[cfg(test)]
mod m2b_tests;
#[cfg(test)]
mod tests;
