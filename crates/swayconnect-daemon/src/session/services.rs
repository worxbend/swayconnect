//! Answers of the M2b operations (`docs/PROTOCOL.md` 17.9 to 17.13): the service call after
//! step 6, the step 7 and 8 checks of an `input.*` target window, and the audit line of each
//! tier 2 request (`docs/DESIGN.md` 11.8). Slice D-WIRE.2b.
//!
//! Typed text goes only to the keyboard: the audit line holds its byte length.

use std::sync::atomic::Ordering;

use swayconnect_protocol::ops::Operation;
use swayconnect_protocol::{
    AudioChange, EntryId, ErrorBody, InputTarget, InputText, JsonObject, KeyPress, MediaCommand,
    Res, Scope,
};
use tokio::io::{AsyncRead, AsyncWrite};

use super::Session;
use super::answers::answer;
use crate::actions;
use crate::audio::Capture;
use crate::audio::pactl::Direction;
use crate::audit::Record;
use crate::dispatch;
use crate::input::InputError;
use crate::sway_handle::Action;

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// `audio.*`: the `max_volume_pct` limit (step 6), then the audio service (step 8, 8a,
    /// the write). A write to a source other than muting it is tier 2 and audited.
    pub(super) async fn audio(&self, id: u64, device_id: u64, change: AudioChange) -> Res {
        let audio = &self.services.audio;
        let Ok(change) = change.check_max_volume(audio.max_volume_pct()) else {
            return Res::failure(id, dispatch::volume_refusal());
        };
        let is_source = audio
            .snapshot()
            .borrow()
            .target(device_id)
            .is_some_and(|target| target.direction == Direction::Source);
        let capture = if self.scopes.contains(&Scope::AudioCapture) {
            Capture::Granted
        } else {
            Capture::NotGranted
        };
        let outcome = audio
            .apply(&self.device_id, device_id, change, capture)
            .await
            .map(|()| JsonObject::new())
            .map_err(|error| {
                tracing::debug!(%error, "audio request refused");
                dispatch::audio_refusal(&error)
            });
        if is_source && change.needs_capture_on_source() {
            let record = Record {
                targets: vec![device_id],
                ..self.record(change.operation(), &outcome)
            };
            self.controls.audit.record_or_warn(&record);
        }
        answer(id, outcome)
    }

    /// `media.*`. Tier 1, so no audit line.
    pub(super) async fn media(&self, id: u64, player_id: u64, command: MediaCommand) -> Res {
        let outcome = self
            .services
            .media
            .command(player_id, command)
            .await
            .map(|()| JsonObject::new())
            .map_err(|error| {
                tracing::debug!(%error, "media request refused");
                dispatch::media_refusal(&error)
            });
        answer(id, outcome)
    }

    /// `app.launch`, with its audit line.
    pub(super) async fn launch(&self, id: u64, entry_id: &EntryId, number: Option<u16>) -> Res {
        let outcome = self
            .start_app(entry_id, number)
            .await
            .map(|()| JsonObject::new());
        let record = self.record(Operation::AppLaunch, &outcome);
        self.controls.audit.record_or_warn(&record);
        answer(id, outcome)
    }

    /// Step 8 and 8b, then the workspace switch, then the start: a refused entry never
    /// switches the workspace, and a failed switch starts nothing.
    async fn start_app(&self, entry_id: &EntryId, number: Option<u16>) -> Result<(), ErrorBody> {
        let apps = self.services.apps.service();
        let may_launch_risky = self.scopes.contains(&Scope::LaunchRisky);
        let prepared = apps
            .prepare_launch(entry_id, may_launch_risky)
            .map_err(|error| dispatch::launch_refusal(&error))?;
        if let Some(number) = number {
            let action = Action::FocusWorkspaceNumber(number);
            self.sway
                .act(action)
                .await
                .map_err(|error| dispatch::action_refusal(action, error))?;
        }
        // The switch above awaited sway; the panic switch may have fired since decide.
        if self.read_only.load(Ordering::SeqCst) {
            return Err(dispatch::read_only_refusal());
        }
        apps.launch(prepared).await.map_err(|error| {
            tracing::warn!(%error, "app.launch failed");
            dispatch::launch_refusal(&error)
        })
    }

    /// `launcher.open`, with its audit line. The window focused now is the one input to
    /// the launcher must find focused (17.13 rule 2).
    pub(super) async fn open_launcher(&self, id: u64) -> Res {
        let outcome = match self.services.focus.focused_window().await {
            Ok(focused) => self
                .services
                .launcher
                .open(&self.device_id, focused)
                .map(|result| result.encode())
                .map_err(|error| {
                    tracing::warn!(%error, "launcher.open failed");
                    dispatch::launcher_refusal(&error)
                }),
            Err(error) => {
                tracing::debug!(%error, "no focus read for launcher.open");
                Err(dispatch::sway_unavailable())
            }
        };
        let record = self.record(Operation::LauncherOpen, &outcome);
        self.controls.audit.record_or_warn(&record);
        answer(id, outcome)
    }

    /// `input.text`, with its audit line.
    pub(super) async fn type_text(&self, id: u64, target: InputTarget, text: &InputText) -> Res {
        let outcome = match self.check_input_target(target) {
            Ok(()) => self
                .input
                .type_text(target, text)
                .await
                .map(|result| result.encode())
                .map_err(InputError::error_body),
            Err(refusal) => Err(refusal),
        };
        let record = Record {
            targets: window_of(target),
            text_len: Some(text.as_str().len()),
            ..self.record(Operation::InputText, &outcome)
        };
        self.controls.audit.record_or_warn(&record);
        answer(id, outcome)
    }

    /// `input.key`, with its audit line.
    pub(super) async fn press_key(&self, id: u64, target: InputTarget, press: &KeyPress) -> Res {
        let outcome = match self.check_input_target(target) {
            Ok(()) => self
                .input
                .press_key(target, press)
                .await
                .map(|()| JsonObject::new())
                .map_err(InputError::error_body),
            Err(refusal) => Err(refusal),
        };
        let record = Record {
            targets: window_of(target),
            ..self.record(Operation::InputKey, &outcome)
        };
        self.controls.audit.record_or_warn(&record);
        answer(id, outcome)
    }

    /// Steps 7 and 8 for a target window, against the `desktop` state; the input service
    /// does 8c. The launcher target has no id to look up.
    fn check_input_target(&self, target: InputTarget) -> Result<(), ErrorBody> {
        let InputTarget::Window(window_id) = target else {
            return Ok(());
        };
        let published = self.desktop.borrow();
        actions::check_input_window(window_id, &published.desktop)
            .map_err(|error| dispatch::action_refusal(Action::FocusWindow(window_id), error))
    }
}

fn window_of(target: InputTarget) -> Vec<u64> {
    match target {
        InputTarget::Window(window_id) => vec![window_id],
        InputTarget::Launcher => Vec::new(),
    }
}
