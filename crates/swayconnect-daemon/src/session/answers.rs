//! Answers that reach past the dispatch table: topic states and their pushes, sway actions,
//! keep awake and lock (`docs/PROTOCOL.md` 8, 17.6 to 17.8), with the audit line of each tier
//! 2 request (`docs/DESIGN.md` 11.8). Slice D-WIRE.2a.

use swayconnect_protocol::ops::{EVENT_STATE_CHANGED, KeepAwake, Operation, TopicState};
use swayconnect_protocol::{ErrorBody, Evt, JsonObject, Message, Res};
use tokio::io::{AsyncRead, AsyncWrite};

use super::feeds::Viewer;
use super::{Flow, Session};
use crate::audit::{Op, Outcome, Record};
use crate::dispatch::{self, Topic};
use crate::sway_handle::Action;

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Follows the topics that are not followed yet and returns each state, in request
    /// order.
    pub(super) fn subscribe(&mut self, topics: &[Topic]) -> Vec<TopicState> {
        let viewer = Viewer {
            device: &self.device_id,
            scopes: &self.scopes,
        };
        topics
            .iter()
            .map(|topic| self.feeds.subscribe(*topic, &viewer))
            .collect()
    }

    /// Sends the newest state of `topic` as `state.changed`.
    pub(super) async fn push(&mut self, topic: Topic) -> Flow {
        let viewer = Viewer {
            device: &self.device_id,
            scopes: &self.scopes,
        };
        let Some(state) = self.feeds.state(topic, &viewer) else {
            return Flow::Continue;
        };
        let event = Evt {
            event: EVENT_STATE_CHANGED.to_owned(),
            data: state.encode(),
        };
        self.send(Message::Evt(event)).await
    }

    /// Hands `action` to the sway half. `window.close` gets an audit line.
    pub(super) async fn act(&self, id: u64, action: Action) -> Res {
        let outcome = self
            .sway
            .act(action)
            .await
            .map_err(|error| dispatch::action_refusal(action, error));
        if let Action::Close(window_id) = action {
            let record = Record {
                targets: vec![window_id],
                ..self.record(Operation::WindowClose, &outcome)
            };
            self.controls.audit.record_or_warn(&record);
        }
        answer(id, outcome.map(|()| JsonObject::new()))
    }

    /// `keep_awake.set`, with its audit line.
    pub(super) async fn keep_awake(&self, id: u64, args: KeepAwake) -> Res {
        let keep_awake = &self.controls.keep_awake;
        let duration_s = match args {
            KeepAwake::On { duration_s } => {
                Some(duration_s.unwrap_or_else(|| keep_awake.view().borrow().default_duration_s))
            }
            KeepAwake::Off => None,
        };
        let outcome = keep_awake
            .set(&self.device_id, args)
            .await
            .map(|result| result.encode())
            .map_err(|error| dispatch::keep_awake_refusal(&error));
        let record = Record {
            duration_s,
            ..self.record(Operation::KeepAwakeSet, &outcome)
        };
        self.controls.audit.record_or_warn(&record);
        answer(id, outcome)
    }

    /// `session.lock`. Tier 1, so no audit line.
    pub(super) async fn lock(&self, id: u64) -> Res {
        // Dispatch offers the op only with a lock command; this is the same check, not a
        // second source of truth.
        let Some(locker) = &self.controls.locker else {
            return Res::failure(id, dispatch::unknown_op());
        };
        let outcome = locker
            .lock()
            .await
            .map(|result| result.encode())
            .map_err(|error| {
                tracing::warn!(%error, "session.lock failed");
                dispatch::lock_refusal(&error)
            });
        answer(id, outcome)
    }

    /// An audit line for `operation` from this device, with the request's outcome.
    pub(super) fn record<T>(&self, operation: Operation, outcome: &Result<T, ErrorBody>) -> Record {
        let outcome = match outcome {
            Ok(_) => Outcome::Ok,
            Err(error) => Outcome::Error(error.code),
        };
        Record::new(
            self.device_id.clone(),
            Some(self.peer),
            Op::Request(operation),
            outcome,
        )
    }
}

pub(super) fn answer(id: u64, outcome: Result<JsonObject, ErrorBody>) -> Res {
    match outcome {
        Ok(result) => Res::success(id, result),
        Err(error) => Res::failure(id, error),
    }
}
