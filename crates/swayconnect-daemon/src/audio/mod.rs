//! Audio (`docs/PROTOCOL.md` 17.9, `docs/DESIGN.md` 11.6): sinks and sources through
//! `pactl`. A task keeps topic `audio` fresh from a `pactl subscribe` child (`watch.rs`);
//! writes run in the caller's task against the latest snapshot.
//!
//! Rule 8 of 11.3: pactl is the config path, every name comes from pactl's own output and
//! goes after `--`, and no client string reaches an argv: a client names a device only by
//! the id this module issued.

use std::sync::Arc;
use std::time::Duration;

use swayconnect_protocol::{AudioChange, DeviceId, ErrorCode, Scope};
use tokio::sync::watch::Receiver;

use self::pactl::Direction;
use self::watch::Watcher;
use crate::notify::{Notice, Notifier};
use crate::proc::{ChildCommand, ProcError, ProgramPath, Runner};

mod config;
pub mod pactl;
pub mod snapshot;
mod watch;

pub use self::config::{AudioConfig, MaxVolumePct};
pub use self::snapshot::Snapshot;

/// The longest any pactl call may take. pactl answers at once unless the sound server hangs.
pub const PACTL_TIMEOUT: Duration = Duration::from_secs(3);

/// What audio uses outside itself.
pub struct AudioDeps {
    pub runner: Arc<dyn Runner>,
    pub notifier: Notifier,
}

/// Whether the device holds scope `audio.capture` (17.16 step 8a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    Granted,
    NotGranted,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no device with this id in the current audio state")]
    NotFound,
    #[error("this change of a source needs scope audio.capture")]
    CaptureRequired,
    #[error("the sound server refused or could not be reached: {0}")]
    Unavailable(String),
    #[error("pactl did not answer in time")]
    Timeout,
}

impl AudioError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AudioError::NotFound => ErrorCode::NotFound,
            AudioError::CaptureRequired => ErrorCode::Forbidden,
            AudioError::Unavailable(_) => ErrorCode::BackendUnavailable,
            AudioError::Timeout => ErrorCode::Timeout,
        }
    }

    /// `detail.scope` of a `forbidden`.
    pub fn scope(&self) -> Option<Scope> {
        match self {
            AudioError::CaptureRequired => Some(Scope::AudioCapture),
            _ => None,
        }
    }
}

/// The parts the watcher and the writers share.
struct Shared {
    runner: Arc<dyn Runner>,
    notifier: Notifier,
    pactl: ProgramPath,
    max_volume_pct: u16,
}

impl Shared {
    fn pactl<'a>(&self, args: impl IntoIterator<Item = &'a str>) -> ChildCommand {
        args.into_iter()
            .fold(ChildCommand::new(&self.pactl), ChildCommand::arg)
    }
}

/// The handle every session shares. Cheap to clone.
#[derive(Clone)]
pub struct AudioService {
    shared: Arc<Shared>,
    snapshot: Receiver<Arc<Snapshot>>,
}

impl AudioService {
    /// Starts the watch task. It ends, killing its `pactl subscribe` child, when `shutdown`
    /// turns `true` or closes. Must be called inside a tokio runtime.
    pub fn spawn(config: &AudioConfig, deps: AudioDeps, shutdown: Receiver<bool>) -> AudioService {
        let max_volume_pct = config.max_volume_pct.get();
        let shared = Arc::new(Shared {
            runner: deps.runner,
            notifier: deps.notifier,
            pactl: config.pactl.clone(),
            max_volume_pct,
        });
        let (sender, snapshot) =
            tokio::sync::watch::channel(Arc::new(Snapshot::unavailable(max_volume_pct)));
        tokio::spawn(Watcher::new(Arc::clone(&shared), sender).run(shutdown));
        AudioService { shared, snapshot }
    }

    /// The server limit of step 6: [`AudioChange::check_max_volume`] takes it.
    pub fn max_volume_pct(&self) -> u16 {
        self.shared.max_volume_pct
    }

    /// Topic `audio`: [`Snapshot::audio`] is the state. Changes only when the state does.
    pub fn snapshot(&self) -> Receiver<Arc<Snapshot>> {
        self.snapshot.clone()
    }

    /// Ops `audio.*` for `device`, after steps 2 to 7 (scope `audio`, read-only, rate class,
    /// args and `max_volume_pct`). Does step 8 (the id), 8a (the capture rule), the write and
    /// the notice of a remote unmute.
    pub async fn apply(
        &self,
        device: &DeviceId,
        device_id: u64,
        change: AudioChange,
        capture: Capture,
    ) -> Result<(), AudioError> {
        let target = self
            .snapshot
            .borrow()
            .target(device_id)
            .cloned()
            .ok_or(AudioError::NotFound)?;
        let is_source = target.direction == Direction::Source;
        if is_source && change.needs_capture_on_source() && capture == Capture::NotGranted {
            return Err(AudioError::CaptureRequired);
        }
        let args = pactl::write_args(
            target.direction,
            &target.name,
            change,
            self.shared.max_volume_pct,
        );
        let finished = self
            .shared
            .runner
            .run(
                self.shared.pactl(args.iter().map(String::as_str)),
                PACTL_TIMEOUT,
            )
            .await
            .map_err(|error| match error {
                ProcError::TimedOut(_) => AudioError::Timeout,
                error => AudioError::Unavailable(error.to_string()),
            })?;
        if !finished.exit.success() {
            return Err(AudioError::Unavailable(format!(
                "pactl ended with {:?}",
                finished.exit
            )));
        }
        if is_source && change == AudioChange::Mute(false) {
            self.show_unmute_notice(device.clone());
        }
        Ok(())
    }

    /// Rule 11: a fixed text and the short id. Runs on its own, so a slow notification
    /// server does not delay the reply.
    fn show_unmute_notice(&self, device: DeviceId) {
        let notifier = self.shared.notifier.clone();
        tokio::spawn(async move {
            if let Err(error) = notifier.show(Notice::MicrophoneUnmuted, &device).await {
                tracing::warn!(%error, "microphone notice not shown");
            }
        });
    }
}

#[cfg(test)]
mod tests;
