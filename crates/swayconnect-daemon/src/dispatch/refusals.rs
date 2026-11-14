//! The `res.error` of a request that passed steps 2 to 6 and was refused later: by the sway
//! half, the keep-awake task, the lock command or an M2b service. Fixed words only: an error
//! of a service can name a program path, an OS error or a player's bus name, which stay in
//! the journal.

use swayconnect_protocol::limits::MAX_INT;
use swayconnect_protocol::ops::WorkspaceTarget;
use swayconnect_protocol::{ErrorBody, ErrorCode};

use super::{detail, refusal};
use crate::apps::{LaunchError, OpenError};
use crate::audio::AudioError;
use crate::keep_awake::KeepAwakeError;
use crate::media::MediaError;
use crate::session_topic::LockError;
use crate::sway_handle::{Action, ActionError};

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

/// The `res.error` for a `keep_awake.set` that the keep-awake task refused. Fixed words:
/// a start error names a program path and an OS error, which stay in the journal.
pub fn keep_awake_refusal(error: &KeepAwakeError) -> ErrorBody {
    let message = match error {
        KeepAwakeError::BadDuration(_) => "invalid arguments",
        KeepAwakeError::LimitReached { .. } => "the continuous keep-awake cap applies",
        KeepAwakeError::Spawn(_) => "cannot start the idle inhibitor",
        KeepAwakeError::Ended(_) => "the idle inhibitor was refused",
        KeepAwakeError::Stopped => "keep awake has stopped",
        KeepAwakeError::ReadOnly => "the daemon is in read-only mode",
    };
    let retry_after = error
        .retry_after_ms()
        .and_then(|millis| detail("retry_after_ms", millis.min(MAX_INT)));
    refusal(error.code(), message, retry_after)
}

/// The `res.error` for a `session.lock` whose lock command did not start.
pub fn lock_refusal(error: &LockError) -> ErrorBody {
    refusal(error.code(), "cannot start the lock command", None)
}

/// The `message` of `not_found`, in the words of transcript T3 (spec 15.5).
fn missing_target(action: Action) -> String {
    match action {
        Action::FocusWorkspaceNumber(number) => format!("no workspace with number {number}"),
        Action::FocusWorkspaceId(id) => format!("no workspace with id {id}"),
        Action::FocusWindow(window_id)
        | Action::Fullscreen { window_id, .. }
        | Action::MoveWindow {
            window_id,
            to: WorkspaceTarget::Number(_),
        }
        | Action::Floating { window_id, .. }
        | Action::Layout { window_id, .. }
        | Action::MoveToScratchpad(window_id)
        | Action::Close(window_id) => format!("no window with id {window_id}"),
        Action::MoveWindow {
            window_id,
            to: WorkspaceTarget::Id(workspace_id),
        } => format!("no window with id {window_id} or no workspace with id {workspace_id}"),
        Action::ScratchpadShow(window_id) => {
            format!("no scratchpad window with id {window_id}")
        }
        // Has no target, so `actions.rs` never answers it with `not_found`.
        Action::OutputsPowerOn => "no output".to_owned(),
    }
}

/// `bad_request` for a `volume_pct` above the audio service's `max_volume_pct` (step 6).
pub fn volume_refusal() -> ErrorBody {
    refusal(
        ErrorCode::BadRequest,
        "volume_pct above max_volume_pct",
        None,
    )
}

/// The `res.error` for an `audio.*` op the audio service refused (step 8, 8a, the write).
pub fn audio_refusal(error: &AudioError) -> ErrorBody {
    let message = match error {
        AudioError::NotFound => "no device with this id in the audio state",
        AudioError::CaptureRequired => "the device lacks the scope",
        AudioError::Unavailable(_) => "the sound server is unavailable",
        AudioError::Timeout => "the sound server did not answer in time",
    };
    let scope = error
        .scope()
        .and_then(|scope| detail("scope", scope.as_str()));
    refusal(error.code(), message, scope)
}

/// The `res.error` for a `media.*` op the media service refused.
pub fn media_refusal(error: &MediaError) -> ErrorBody {
    let message = match error {
        MediaError::NotFound => "no player with this id in the media state",
        MediaError::UnsupportedTarget => "the player does not allow this command",
        MediaError::Backend(_) if error.code() == ErrorCode::Timeout => {
            "the player did not answer in time"
        }
        MediaError::Backend(_) => "the media service is unavailable",
    };
    refusal(error.code(), message, None)
}

/// The `res.error` for an `app.launch` the apps service refused (step 8, 8b, the start).
pub fn launch_refusal(error: &LaunchError) -> ErrorBody {
    let message = match error {
        LaunchError::NotFound => "no app with this entry_id in the apps state",
        LaunchError::NeedsLaunchRisky => "the device lacks the scope",
        LaunchError::Random | LaunchError::Start(_) => "cannot start the app",
    };
    let scope = error
        .missing_scope()
        .and_then(|scope| detail("scope", scope.as_str()));
    refusal(error.code(), message, scope)
}

/// The `res.error` for a `launcher.open` that started nothing.
pub fn launcher_refusal(error: &OpenError) -> ErrorBody {
    match error {
        OpenError::Closed => read_only_refusal(),
        OpenError::Spawn(_) => refusal(ErrorCode::SpawnFailed, "cannot start the launcher", None),
    }
}

/// `read_only`: at decide time, or for a request that passed decide before the panic
/// switch and reached its start after it.
pub fn read_only_refusal() -> ErrorBody {
    refusal(ErrorCode::ReadOnly, "the daemon is in read-only mode", None)
}

/// `sway_unavailable` for a `launcher.open` whose focus read got no answer.
pub fn sway_unavailable() -> ErrorBody {
    refusal(ErrorCode::SwayUnavailable, "sway is offline", None)
}
