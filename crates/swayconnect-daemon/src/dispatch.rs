//! Operation table and request checks, in the order of `docs/PROTOCOL.md` 10.2.
//!
//! Steps 2 to 6 are decided here, with the rate classes of 17.14 and the additions of 17.16.
//! An action that passes them leaves as [`Decision::Act`]; steps 7 to 9 belong to the sway
//! half (`actions.rs`, `supervisor.rs`).

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;
use swayconnect_protocol::limits::MAX_INT;
use swayconnect_protocol::ops::{Args, Operation, TOPIC_DESKTOP, WorkspaceTarget};
use swayconnect_protocol::{DeviceId, ErrorBody, ErrorCode, IconHash, JsonObject, Req, Scope};

use crate::security::Security;
use crate::sway_handle::{Action, ActionError};
use crate::throttle::RateClass;

/// In the order of `welcome.ops`: the version 1 operations first, `icon.get` after them.
const SERVED_OPERATIONS: [Operation; 6] = [
    Operation::Ping,
    Operation::StateSubscribe,
    Operation::WorkspaceFocus,
    Operation::WindowFocus,
    Operation::WindowFullscreen,
    Operation::IconGet,
];

/// Every scope this version knows (`welcome.known_scopes`).
const KNOWN_SCOPES: [Scope; 3] = [Scope::StateRead, Scope::StateTitles, Scope::Control];

/// What the session does with one request.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// The device was revoked since it authenticated: `bye` with `revoked`.
    Revoked,
    Refuse(ErrorBody),
    Pong,
    SubscribeDesktop,
    /// Answer `icon.get` from the icon store, in request order.
    Icons(Vec<IconHash>),
    /// Hand the action to the sway half; answer with its outcome.
    Act(Action),
}

/// Who sends the request, and what the daemon allows right now.
pub struct Caller<'a> {
    pub security: &'a Security,
    pub device: &'a DeviceId,
    pub scopes: &'a BTreeSet<Scope>,
    pub is_read_only: bool,
}

/// `welcome.ops` for a device with `scopes`. A read-only daemon offers no action.
pub fn offered_ops(scopes: &BTreeSet<Scope>, is_read_only: bool) -> Vec<String> {
    SERVED_OPERATIONS
        .into_iter()
        .filter(|operation| is_granted(*operation, scopes))
        .filter(|operation| !(is_read_only && operation.is_action()))
        .map(|operation| operation.as_str().to_owned())
        .collect()
}

/// `welcome.known_ops`: the operations a device with every scope gets when the daemon is not
/// read-only (spec 17.3).
pub fn known_ops() -> Vec<String> {
    SERVED_OPERATIONS
        .into_iter()
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

/// The rate class of spec 17.14 whose bucket `operation` takes a token from (step 5).
pub const fn rate_class(operation: Operation) -> Option<RateClass> {
    match operation {
        Operation::Ping | Operation::StateSubscribe => None,
        Operation::IconGet => Some(RateClass::Icons),
        Operation::WorkspaceFocus | Operation::WindowFocus | Operation::WindowFullscreen => {
            Some(RateClass::Actions)
        }
    }
}

/// `welcome.topics` for a device with `scopes`.
pub fn offered_topics(scopes: &BTreeSet<Scope>) -> Vec<String> {
    if scopes.contains(&Scope::StateRead) {
        vec![TOPIC_DESKTOP.to_owned()]
    } else {
        Vec::new()
    }
}

pub fn decide(caller: &Caller<'_>, request: &Req) -> Decision {
    let device = caller.device;
    if !caller.security.is_active(device) {
        return Decision::Revoked;
    }
    let take_token = |class| caller.security.take_token(device, class);
    let decision = check(caller.scopes, caller.is_read_only, take_token, request)
        .unwrap_or_else(Decision::Refuse);
    // `take_token` logs an empty action bucket itself.
    if let Decision::Act(action) = &decision {
        tracing::info!(device = device.as_str(), ?action, "action accepted");
    }
    decision
}

/// The `res.error` for `action`, which the sway half refused (steps 7 to 9 of spec 10.2).
pub fn action_refusal(action: Action, error: ActionError) -> ErrorBody {
    match error {
        ActionError::NotFound => refusal(ErrorCode::NotFound, &missing_target(action), None),
        ActionError::UnsupportedTarget => refusal(
            ErrorCode::UnsupportedTarget,
            "the workspace cannot be addressed by its number",
            None,
        ),
        ActionError::SwayUnavailable => {
            refusal(ErrorCode::SwayUnavailable, "sway is offline", None)
        }
        ActionError::SwayError(sway_text) => refusal(ErrorCode::SwayError, &sway_text, None),
        ActionError::Timeout => refusal(ErrorCode::Timeout, "sway did not answer in time", None),
    }
}

/// The `message` of `not_found`, in the words of transcript T3 (spec 15.5).
fn missing_target(action: Action) -> String {
    match action {
        Action::FocusWorkspaceNumber(number) => format!("no workspace with number {number}"),
        Action::FocusWorkspaceId(id) => format!("no workspace with id {id}"),
        Action::FocusWindow(window_id) | Action::Fullscreen { window_id, .. } => {
            format!("no window with id {window_id}")
        }
    }
}

fn is_granted(operation: Operation, scopes: &BTreeSet<Scope>) -> bool {
    operation
        .required_scope()
        .is_none_or(|scope| scopes.contains(&scope))
}

/// Steps 2 to 6 of spec 10.2. `take_token` is called for an operation with a rate class
/// that reaches step 5, and only then.
fn check(
    scopes: &BTreeSet<Scope>,
    is_read_only: bool,
    take_token: impl FnOnce(RateClass) -> Result<(), Duration>,
    request: &Req,
) -> Result<Decision, ErrorBody> {
    let operation = Operation::from_wire(&request.op)
        .filter(|operation| SERVED_OPERATIONS.contains(operation))
        .ok_or_else(|| refusal(ErrorCode::UnknownOp, "unknown operation", None))?;
    if let Some(scope) = operation
        .required_scope()
        .filter(|scope| !scopes.contains(scope))
    {
        let detail = detail("scope", scope.as_str());
        return Err(refusal(
            ErrorCode::Forbidden,
            "the device lacks the scope",
            detail,
        ));
    }
    if is_read_only && operation.is_action() {
        return Err(refusal(
            ErrorCode::ReadOnly,
            "the daemon is in read-only mode",
            None,
        ));
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
        Args::StateSubscribe { topics } => {
            match topics.iter().find(|topic| *topic != TOPIC_DESKTOP) {
                Some(topic) => Err(refusal(
                    ErrorCode::UnknownTopic,
                    "unknown topic",
                    detail("topic", topic.as_str()),
                )),
                None => Ok(Decision::SubscribeDesktop),
            }
        }
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
    };
    refusal(
        ErrorCode::RateLimited,
        message,
        detail("retry_after_ms", retry_after_ms),
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
mod tests;
