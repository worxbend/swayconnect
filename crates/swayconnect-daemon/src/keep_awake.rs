//! Keep awake (`docs/PROTOCOL.md` 17.8, `docs/DESIGN.md` 11.6): at most one
//! `systemd-inhibit --what=idle --mode=block ... sleep <n>` child, so the logind inhibitor
//! ends by itself at the expiry even when the daemon is gone.
//!
//! One task owns the child, the expiry, the holder and the continuous cap. Requests reach it
//! over a channel; its view goes out on a `watch` for topic `session`. Nothing here belongs to
//! a connection, so a client disconnect ends nothing (rule 6).

use std::sync::Arc;
use std::time::Duration;

use swayconnect_protocol::ops::KeepAwake as KeepAwakeArgs;
use swayconnect_protocol::{
    Desktop, DeviceId, ErrorCode, Holder, InvalidMember, KeepAwakeResult, KeepAwakeState,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, sleep_until};

use self::budget::Budget;
use crate::notify::{Notice, Notifier};
use crate::proc::{BoxFuture, ChildCommand, Exit, ProcError, Runner, Running};
use crate::session::stopped;
use crate::sway_handle::{ActionError, Published};

mod budget;
mod config;

pub use self::config::KeepAwakeConfig;

const INHIBIT_ARGS: [&str; 4] = [
    "--what=idle",
    "--mode=block",
    "--who=swayconnect",
    "--why=Kept awake from a paired device",
];
const REQUEST_QUEUE_LEN: usize = 16;
/// How long a new inhibitor is watched before it counts as holding
/// ([`KeepAwakeDeps::start_check`]). It runs for at least a minute, so an end within this
/// time means logind, polkit or the bus refused it.
pub const START_CHECK: Duration = Duration::from_millis(500);

/// The fixed sway command `output * power on` (`docs/DESIGN.md` 11.3 rule 2). The daemon
/// wiring implements it with the sway handle.
pub trait PowerOn: Send + Sync + 'static {
    fn power_on(&self) -> BoxFuture<'_, Result<(), ActionError>>;
}

/// Rule 2 of 17.8: an active output is powered off.
pub fn needs_power_on(desktop: &Desktop) -> bool {
    desktop
        .outputs
        .iter()
        .flatten()
        .any(|output| output.active && !output.power)
}

/// What keep-awake uses outside itself.
pub struct KeepAwakeDeps {
    pub runner: Arc<dyn Runner>,
    pub notifier: Notifier,
    pub desktop: watch::Receiver<Arc<Published>>,
    pub power: Arc<dyn PowerOn>,
    /// [`START_CHECK`] in the daemon. Unit tests on a paused clock pass zero, so the check
    /// does not move their clock; it then sees only a child that ended before the spawn
    /// returned.
    pub start_check: Duration,
}

/// The device that sent the last `true`, and when the inhibitor ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub holder: DeviceId,
    pub expires_at: Instant,
}

/// Keep-awake as topic `session` needs it. Changes only on events, so a `watch` of it
/// publishes nothing while time passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAwakeView {
    pub hold: Option<Hold>,
    /// Set while `limit_reached` applies.
    pub cooldown_until: Option<Instant>,
    pub default_duration_s: u64,
    pub max_duration_s: u64,
    pub max_continuous_s: u64,
}

impl KeepAwakeView {
    /// Member `keep_awake` of topic `session` for `device`, with times as of `now`.
    pub fn state_for(&self, device: &DeviceId, now: Instant) -> KeepAwakeState {
        let holder = self.hold.as_ref().map(|hold| {
            if hold.holder == *device {
                Holder::ThisDevice
            } else {
                Holder::OtherDevice
            }
        });
        KeepAwakeState {
            active: self.hold.is_some(),
            remaining_ms: self
                .hold
                .as_ref()
                .map(|hold| millis_until(hold.expires_at, now)),
            holder,
            default_duration_s: self.default_duration_s,
            max_duration_s: self.max_duration_s,
            max_continuous_s: self.max_continuous_s,
            cooldown_remaining_ms: self.cooldown_until.map(|until| millis_until(until, now)),
        }
    }
}

fn millis_until(deadline: Instant, now: Instant) -> u64 {
    millis(deadline.saturating_duration_since(now))
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum KeepAwakeError {
    #[error("duration_s is above max_duration_s")]
    BadDuration(InvalidMember),
    #[error("the continuous keep-awake cap applies")]
    LimitReached { retry_after: Duration },
    #[error("cannot start the inhibitor: {0}")]
    Spawn(#[source] ProcError),
    #[error("the inhibitor ended at once: {0:?}")]
    Ended(Result<Exit, ProcError>),
    #[error("keep-awake is closed until the daemon restarts")]
    ReadOnly,
    #[error("keep-awake has stopped with the daemon")]
    Stopped,
}

impl KeepAwakeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            KeepAwakeError::BadDuration(_) => ErrorCode::BadRequest,
            KeepAwakeError::LimitReached { .. } => ErrorCode::LimitReached,
            KeepAwakeError::Spawn(_) => ErrorCode::SpawnFailed,
            KeepAwakeError::Ended(_) | KeepAwakeError::Stopped => ErrorCode::BackendUnavailable,
            KeepAwakeError::ReadOnly => ErrorCode::ReadOnly,
        }
    }

    /// `detail.retry_after_ms` of `limit_reached`.
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            KeepAwakeError::LimitReached { retry_after } => Some(millis(*retry_after)),
            _ => None,
        }
    }
}

enum Request {
    Set {
        device: DeviceId,
        args: KeepAwakeArgs,
        reply: oneshot::Sender<Result<KeepAwakeResult, KeepAwakeError>>,
    },
    /// `None`: whoever holds it (daemon stop). `Some`: only that holder (revoke).
    Release {
        holder: Option<DeviceId>,
        reply: oneshot::Sender<()>,
    },
    /// The panic switch: ends keep-awake and refuses every later `true`.
    Close { reply: oneshot::Sender<()> },
}

/// The handle every session shares. Cheap to clone.
#[derive(Clone)]
pub struct KeepAwake {
    requests: mpsc::Sender<Request>,
    view: watch::Receiver<KeepAwakeView>,
}

impl KeepAwake {
    /// Starts the task. It ends, killing the child, when `shutdown` turns `true` or closes,
    /// or when every handle is gone. Must be called inside a tokio runtime.
    pub fn spawn(
        config: &KeepAwakeConfig,
        deps: KeepAwakeDeps,
        shutdown: watch::Receiver<bool>,
    ) -> KeepAwake {
        let (view_sender, view) = watch::channel(KeepAwakeView {
            hold: None,
            cooldown_until: None,
            default_duration_s: config.default_duration_s,
            max_duration_s: config.max_duration_s,
            max_continuous_s: config.max_continuous_s,
        });
        let (requests, queued) = mpsc::channel(REQUEST_QUEUE_LEN);
        let keeper = Keeper {
            budget: Budget::new(
                Duration::from_secs(config.max_continuous_s),
                Duration::from_secs(config.cooldown_s),
            ),
            config: config.clone(),
            deps,
            child: None,
            view: view_sender,
            closed: false,
        };
        tokio::spawn(keeper.run(queued, shutdown));
        KeepAwake { requests, view }
    }

    pub fn view(&self) -> watch::Receiver<KeepAwakeView> {
        self.view.clone()
    }

    /// Op `keep_awake.set` for `device`, after the scope, read-only and rate checks.
    pub async fn set(
        &self,
        device: &DeviceId,
        args: KeepAwakeArgs,
    ) -> Result<KeepAwakeResult, KeepAwakeError> {
        let (reply, outcome) = oneshot::channel();
        let request = Request::Set {
            device: device.clone(),
            args,
            reply,
        };
        self.requests
            .send(request)
            .await
            .map_err(|_| KeepAwakeError::Stopped)?;
        outcome.await.unwrap_or(Err(KeepAwakeError::Stopped))
    }

    /// Ends keep-awake whoever holds it: daemon stop (rule 6).
    pub async fn release(&self) {
        self.send_release(None).await;
    }

    /// The panic switch (SIGUSR1, rule 6): ends keep-awake whoever holds it, and refuses
    /// with `read_only` every `true` still queued or sent later, until the daemon restarts.
    pub async fn close(&self) {
        let (reply, done) = oneshot::channel();
        // A closed queue or a dropped reply: the task has ended and killed its child.
        if self.requests.send(Request::Close { reply }).await.is_ok() {
            let _ = done.await;
        }
    }

    /// Ends keep-awake when `device` holds it: revoke of the holder (rule 6).
    pub async fn release_holder(&self, device: &DeviceId) {
        self.send_release(Some(device.clone())).await;
    }

    async fn send_release(&self, holder: Option<DeviceId>) {
        let (reply, done) = oneshot::channel();
        // A closed queue or a dropped reply: the task has ended and killed its child.
        if self
            .requests
            .send(Request::Release { holder, reply })
            .await
            .is_ok()
        {
            let _ = done.await;
        }
    }
}

/// The task behind [`KeepAwake`].
struct Keeper {
    config: KeepAwakeConfig,
    deps: KeepAwakeDeps,
    budget: Budget,
    child: Option<Box<dyn Running>>,
    view: watch::Sender<KeepAwakeView>,
    /// Set by [`Request::Close`]; never cleared.
    closed: bool,
}

impl Keeper {
    async fn run(
        mut self,
        mut requests: mpsc::Receiver<Request>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        loop {
            let expires_at = self.view.borrow().hold.as_ref().map(|hold| hold.expires_at);
            let cooldown_until = self.view.borrow().cooldown_until;
            // Biased: an expiry or a child exit that is due is handled before the next
            // request, so a request never sees a hold that has already run out.
            tokio::select! {
                biased;
                () = stopped(&mut shutdown) => break,
                exit = child_exit(&mut self.child) => self.child_ended(exit),
                () = deadline(expires_at) => self.end().await,
                () = deadline(cooldown_until) => {
                    self.publish(None);
                }
                request = requests.recv() => match request {
                    Some(request) => self.handle(request).await,
                    None => break,
                },
            }
        }
        self.end().await;
    }

    async fn handle(&mut self, request: Request) {
        match request {
            Request::Set {
                device,
                args,
                reply,
            } => {
                let outcome = self.set(device, args).await;
                // The caller gave up; the change stands either way.
                let _ = reply.send(outcome);
            }
            Request::Release { holder, reply } => {
                let held_by = self
                    .view
                    .borrow()
                    .hold
                    .as_ref()
                    .map(|hold| hold.holder.clone());
                if holder.is_none() || holder == held_by {
                    self.end().await;
                }
                let _ = reply.send(());
            }
            Request::Close { reply } => {
                self.closed = true;
                self.end().await;
                let _ = reply.send(());
            }
        }
    }

    async fn set(
        &mut self,
        device: DeviceId,
        args: KeepAwakeArgs,
    ) -> Result<KeepAwakeResult, KeepAwakeError> {
        let args = args
            .check_max_duration(self.config.max_duration_s)
            .map_err(KeepAwakeError::BadDuration)?;
        match args {
            KeepAwakeArgs::On { duration_s } => {
                let duration_s = duration_s.unwrap_or(self.config.default_duration_s);
                self.turn_on(device, duration_s).await
            }
            KeepAwakeArgs::Off => {
                self.end().await;
                Ok(KeepAwakeResult { remaining_ms: None })
            }
        }
    }

    /// Starts or replaces the inhibitor: the new child first, then the old one is killed, so
    /// a replace leaves no gap without an inhibitor. The old one stays when the new one
    /// ends within [`KeepAwakeDeps::start_check`]. The hold counts from the spawn, when the
    /// inhibitor's own sleep started; `remaining_ms` is the time granted.
    async fn turn_on(
        &mut self,
        device: DeviceId,
        duration_s: u64,
    ) -> Result<KeepAwakeResult, KeepAwakeError> {
        if self.closed {
            return Err(KeepAwakeError::ReadOnly);
        }
        let now = Instant::now();
        let seconds = duration_s.min(self.budget.seconds_left(now));
        if seconds == 0 {
            let retry_after = self.budget.retry_after(now).unwrap_or_default();
            return Err(KeepAwakeError::LimitReached { retry_after });
        }
        let mut started = self
            .deps
            .runner
            .spawn(self.inhibit_command(seconds))
            .map_err(KeepAwakeError::Spawn)?;
        if let Ok(ended) = tokio::time::timeout(self.deps.start_check, started.exited()).await {
            tracing::warn!(?ended, "keep-awake inhibitor ended at once");
            return Err(KeepAwakeError::Ended(ended));
        }
        if let Some(old) = self.child.replace(started) {
            stop_child(old).await;
        }
        self.budget.start(now);
        let hold = Hold {
            holder: device.clone(),
            expires_at: now + Duration::from_secs(seconds),
        };
        let previous = self.publish(Some(hold));
        power_on_if_needed(&self.deps).await;
        if previous.is_none_or(|previous| previous.holder != device) {
            self.show_notice(device);
        }
        Ok(KeepAwakeResult {
            remaining_ms: Some(seconds * 1000),
        })
    }

    fn inhibit_command(&self, seconds: u64) -> ChildCommand {
        INHIBIT_ARGS
            .iter()
            .fold(
                ChildCommand::new(&self.config.inhibit),
                |command, argument| command.arg(argument),
            )
            .arg(self.config.sleep.as_path())
            .arg(seconds.to_string())
    }

    /// Rule 7 for a start, and for a take-over by another device. Runs on its own, so a slow
    /// notification server delays nothing.
    fn show_notice(&self, device: DeviceId) {
        let notifier = self.deps.notifier.clone();
        tokio::spawn(async move {
            if let Err(error) = notifier.show(Notice::KeepAwakeOn, &device).await {
                tracing::warn!(%error, "keep-awake notice not shown");
            }
        });
    }

    /// The inhibitor ended by itself: at the expiry, or early when it failed.
    fn child_ended(&mut self, exit: Result<Exit, ProcError>) {
        self.child = None;
        let early = self
            .view
            .borrow()
            .hold
            .as_ref()
            .is_some_and(|hold| Instant::now() < hold.expires_at);
        if early {
            tracing::warn!(?exit, "keep-awake inhibitor ended before its expiry");
        }
        self.budget.stop(Instant::now());
        self.publish(None);
    }

    /// Off: kills the child and waits for its end.
    async fn end(&mut self) {
        if let Some(child) = self.child.take() {
            stop_child(child).await;
        }
        self.budget.stop(Instant::now());
        self.publish(None);
    }

    /// Sets the hold, recomputes the cooldown, publishes on a change. Returns the old hold.
    fn publish(&mut self, hold: Option<Hold>) -> Option<Hold> {
        let now = Instant::now();
        let cooldown_until = match hold {
            Some(_) => None,
            None => self.budget.retry_after(now).map(|wait| now + wait),
        };
        let mut previous = None;
        self.view.send_if_modified(|view| {
            previous = std::mem::replace(&mut view.hold, hold);
            let changed = previous != view.hold || view.cooldown_until != cooldown_until;
            view.cooldown_until = cooldown_until;
            changed
        });
        previous
    }
}

/// Rule 2 of 17.8, after the inhibitor is taken. A failure leaves keep-awake on.
async fn power_on_if_needed(deps: &KeepAwakeDeps) {
    let off = needs_power_on(&deps.desktop.borrow().desktop);
    if off && let Err(error) = deps.power.power_on().await {
        tracing::warn!(?error, "keep-awake: outputs not powered on");
    }
}

/// Resolves when the child ends; never without one. Cancel-safe.
async fn child_exit(child: &mut Option<Box<dyn Running>>) -> Result<Exit, ProcError> {
    match child {
        Some(child) => child.exited().await,
        None => std::future::pending().await,
    }
}

/// Resolves at `at`; never for `None`.
async fn deadline(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn stop_child(mut child: Box<dyn Running>) {
    if let Err(error) = child.kill() {
        tracing::warn!(%error, "keep-awake: cannot kill the inhibitor");
        return;
    }
    if let Err(error) = child.exited().await {
        tracing::warn!(%error, "keep-awake: the inhibitor's end is unknown");
    }
}

#[cfg(test)]
mod tests;
