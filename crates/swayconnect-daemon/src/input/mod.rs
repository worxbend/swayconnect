//! Remote keyboard (`docs/PROTOCOL.md` 17.13, `docs/DESIGN.md` 11.7): text cut into parts
//! of at most 16 characters, the checks of [`guard`] before every part, delivery through a
//! [`KeyboardSink`], release of every key after each request and on disconnect, and the
//! input session with its desktop notice.
//!
//! Typed text goes only to the keyboard. It is never logged: logs carry lengths and codes.

pub mod config;
pub mod guard;
pub mod keymap;
mod session;
pub mod vkbd;

#[cfg(test)]
pub(crate) mod fake;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use swayconnect_protocol::{
    DeviceId, ErrorBody, ErrorCode, InputRefusal, InputTarget, InputText, InputTextResult, KeyPress,
};
use tokio::sync::{Mutex, watch};

pub use self::config::{InputConfig, SeatName, WaylandConfig, WaylandTarget};
pub use self::guard::{Guard, Stop, SwayFocus};
use self::keymap::KeymapError;
pub use self::keymap::{Delivery, KeyEvent, Keymap};
pub use self::session::SESSION_IDLE;
use self::session::Sessions;
pub use self::vkbd::VirtualKeyboard;
use crate::notify::{Notice, Notifier};
use crate::proc::BoxFuture;

/// Where the parts go. The real one is [`VirtualKeyboard`]; tests use `FakeKeyboard`.
pub trait KeyboardSink: Send + Sync + 'static {
    /// Sends one part and returns once the compositor has processed it.
    fn deliver(&self, delivery: Delivery) -> BoxFuture<'_, Result<(), SinkError>>;
    /// Queues a release of every key and modifier behind any delivery already sent. Does
    /// not wait, so a drop can call it.
    fn release_all(&self);
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("no Wayland socket is known")]
    NoDisplay,
    #[error("the configured seat is not offered by the compositor")]
    NoSeat,
    #[error("Wayland: {0}")]
    Wayland(String),
    #[error("the keymap does not fit a Wayland size")]
    KeymapTooLarge,
    #[error("the compositor did not process the keys in time")]
    TimedOut,
    #[error("the keyboard thread has stopped")]
    Stopped,
}

/// Why an input request ended without delivering everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// A check of 17.13 rule 2 failed; the refusal holds what was delivered before it.
    #[error("input refused with {}", .0.code().as_str())]
    Refused(InputRefusal),
    #[error("sway did not answer the focus read")]
    SwayUnavailable,
    #[error("the virtual keyboard is unavailable")]
    BackendUnavailable,
    /// The panic switch stopped input.
    #[error("input was stopped at the desktop")]
    Aborted,
    #[error("the input could not be mapped to keys")]
    Unmappable,
}

impl InputError {
    pub fn code(self) -> ErrorCode {
        match self {
            InputError::Refused(refusal) => refusal.code(),
            InputError::SwayUnavailable => ErrorCode::SwayUnavailable,
            InputError::BackendUnavailable | InputError::Aborted => ErrorCode::BackendUnavailable,
            InputError::Unmappable => ErrorCode::Internal,
        }
    }

    /// The `res.error` with fixed text.
    pub fn error_body(self) -> ErrorBody {
        let message = match self {
            InputError::Refused(InputRefusal::Locked { .. }) => "the screen is locked",
            InputError::Refused(InputRefusal::LauncherClosed { .. }) => {
                "no launcher of this device is running"
            }
            InputError::Refused(InputRefusal::KeyboardGrabbed { .. }) => {
                "another surface takes the keyboard"
            }
            InputError::Refused(InputRefusal::FocusChanged { .. }) => {
                "the focused window is not the target"
            }
            InputError::SwayUnavailable => "sway did not answer",
            InputError::BackendUnavailable => "the virtual keyboard is unavailable",
            InputError::Aborted => "input was stopped at the desktop",
            InputError::Unmappable => "the input could not be mapped to keys",
        };
        match self {
            InputError::Refused(refusal) => refusal.error_body(message),
            _ => ErrorBody {
                code: self.code(),
                message: message.to_owned(),
                detail: None,
            },
        }
    }

    fn stopped(stop: Stop, delivered_chars: u64) -> InputError {
        let refusal = match stop {
            Stop::Locked => InputRefusal::Locked { delivered_chars },
            Stop::LauncherClosed => InputRefusal::LauncherClosed { delivered_chars },
            Stop::KeyboardGrabbed => InputRefusal::KeyboardGrabbed { delivered_chars },
            Stop::FocusChanged(focused_window_id) => InputRefusal::FocusChanged {
                delivered_chars,
                focused_window_id,
            },
            Stop::SwayUnavailable => return InputError::SwayUnavailable,
        };
        InputError::Refused(refusal)
    }
}

/// One part: its keys and the characters it counts (a key press counts none).
struct Part {
    chars: u64,
    delivery: Result<Delivery, KeymapError>,
}

struct Shared {
    sink: Arc<dyn KeyboardSink>,
    guard: Guard,
    notifier: Notifier,
    /// One request at a time over all connections, so parts and keymaps never interleave.
    turn: Mutex<()>,
    /// Bumped by the panic switch; a request started under an older value stops.
    generation: AtomicU64,
    sessions: Sessions,
}

/// The daemon's remote keyboard. Cheap to clone.
#[derive(Clone)]
pub struct InputService {
    shared: Arc<Shared>,
}

impl InputService {
    pub fn new(sink: Arc<dyn KeyboardSink>, guard: Guard, notifier: Notifier) -> InputService {
        InputService {
            shared: Arc::new(Shared {
                sink,
                guard,
                notifier,
                turn: Mutex::new(()),
                generation: AtomicU64::new(0),
                sessions: Sessions::default(),
            }),
        }
    }

    /// The input of one connection of `device`. Drop it when the connection ends.
    pub fn connection(&self, device: DeviceId) -> InputConnection {
        InputConnection {
            id: self.shared.sessions.next_id(),
            device,
            shared: Arc::clone(&self.shared),
        }
    }

    /// The device of the newest armed input session, `None` while none is armed. Member
    /// `input` of topic `session`.
    pub fn view(&self) -> watch::Receiver<Option<DeviceId>> {
        self.shared.sessions.view()
    }

    /// Panic switch (`docs/DESIGN.md` 11.8): stops every running request before its next
    /// part, ends every session and releases every key.
    pub fn abort(&self) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.sessions.end_all();
        self.shared.sink.release_all();
    }
}

/// Input of one connection. Dropping it ends its session and releases every key.
pub struct InputConnection {
    id: u64,
    device: DeviceId,
    shared: Arc<Shared>,
}

impl InputConnection {
    /// `input.text`: the parts of 17.13 rule 1, each checked and delivered in turn.
    pub async fn type_text(
        &self,
        target: InputTarget,
        text: &InputText,
    ) -> Result<InputTextResult, InputError> {
        tracing::debug!(bytes = text.as_str().len(), "input.text");
        let parts = text.parts().map(|part| Part {
            chars: part.chars().count() as u64,
            delivery: keymap::text_part(part),
        });
        let delivered_chars = self.run(target, parts).await?;
        Ok(InputTextResult { delivered_chars })
    }

    /// `input.key`: one part.
    pub async fn press_key(&self, target: InputTarget, press: &KeyPress) -> Result<(), InputError> {
        tracing::debug!(
            key = press.key().as_str(),
            repeat = press.repeat(),
            "input.key"
        );
        let part = Part {
            chars: 0,
            delivery: keymap::key_press(press),
        };
        self.run(target, std::iter::once(part)).await.map(drop)
    }

    async fn run(
        &self,
        target: InputTarget,
        parts: impl Iterator<Item = Part>,
    ) -> Result<u64, InputError> {
        let generation = self.shared.generation.load(Ordering::SeqCst);
        let _turn = self.shared.turn.lock().await;
        // Also on cancellation: a dropped request still releases every key.
        let _release = ReleaseOnDrop(self.shared.sink.as_ref());
        let outcome = self.deliver_parts(target, parts, generation).await;
        if let Err(error) = outcome {
            tracing::debug!(code = error.code().as_str(), "input stopped");
        }
        outcome
    }

    async fn deliver_parts(
        &self,
        target: InputTarget,
        parts: impl Iterator<Item = Part>,
        generation: u64,
    ) -> Result<u64, InputError> {
        let mut delivered = 0;
        for part in parts {
            self.shared
                .guard
                .check(&self.device, target)
                .await
                .map_err(|stop| InputError::stopped(stop, delivered))?;
            // After the awaited check, so a panic during it still stops this part.
            if self.shared.generation.load(Ordering::SeqCst) != generation {
                return Err(InputError::Aborted);
            }
            let delivery = part.delivery.map_err(|error| {
                tracing::error!(%error, "input part has no keys");
                InputError::Unmappable
            })?;
            self.arm();
            self.shared.sink.deliver(delivery).await.map_err(|error| {
                tracing::warn!(%error, "virtual keyboard delivery failed");
                InputError::BackendUnavailable
            })?;
            delivered += part.chars;
        }
        Ok(delivered)
    }

    /// Arms or refreshes this connection's session; the first arming shows the notice.
    fn arm(&self) {
        if self.shared.sessions.touch(self.id, &self.device) {
            tracing::info!("remote keyboard session started");
            self.shared.sessions.spawn_expiry(self.id);
            let notifier = self.shared.notifier.clone();
            let device = self.device.clone();
            tokio::spawn(async move {
                if let Err(error) = notifier.show(Notice::RemoteKeyboardActive, &device).await {
                    tracing::warn!(%error, "remote keyboard notice not shown");
                }
            });
        }
    }
}

impl Drop for InputConnection {
    fn drop(&mut self) {
        self.shared.sessions.end(self.id);
        self.shared.sink.release_all();
    }
}

struct ReleaseOnDrop<'a>(&'a dyn KeyboardSink);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release_all();
    }
}

#[cfg(test)]
#[path = "log_tests.rs"]
mod log_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "vector_tests.rs"]
mod vector_tests;
