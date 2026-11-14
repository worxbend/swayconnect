//! Topic `session` and op `session.lock` (`docs/PROTOCOL.md` 17.6, 17.8; `docs/DESIGN.md`
//! 11.6).
//!
//! The facts behind the topic are the same for every device; [`SessionFacts::state_for`]
//! builds the per-device state, because `holder` names the receiver. While at least one
//! client subscribes, a task re-runs the lock scan every 2 s and publishes on a change.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use swayconnect_protocol::{DeviceId, ErrorCode, LockResult, Session};
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};

use crate::keep_awake::KeepAwakeView;
use crate::lockstate::{ProcScanner, SessionConfig};
use crate::proc::{ChildCommand, ProcError, Runner, Running};
use crate::session::stopped;

/// How often lock state is re-checked while `session` has subscribers (17.8).
pub const LOCK_POLL: Duration = Duration::from_secs(2);

/// What topic `session` shows, before it is built for one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    pub keep_awake: KeepAwakeView,
    pub windows_inhibiting: u64,
    /// `None` when unknown: the scan failed, or nobody subscribes, so nothing re-checks it.
    pub locked: Option<bool>,
}

impl SessionFacts {
    /// The state object for `device`, with times as of `now`. M2a offers neither input nor
    /// the launcher, so both members stay absent.
    pub fn state_for(&self, device: &DeviceId, now: Instant) -> Session {
        Session {
            keep_awake: self.keep_awake.state_for(device, now),
            windows_inhibiting: self.windows_inhibiting,
            locked: self.locked,
            input: None,
            launcher: None,
        }
    }
}

/// One publication of the topic. `rev` grows by one per change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSession {
    pub rev: u64,
    pub facts: SessionFacts,
}

/// The sources the topic task reads.
pub struct SessionSources {
    pub keep_awake: watch::Receiver<KeepAwakeView>,
    pub windows_inhibiting: watch::Receiver<u32>,
    pub scanner: Arc<ProcScanner>,
}

pub struct SessionTopic {
    state: watch::Receiver<Arc<PublishedSession>>,
    subscribers: Arc<watch::Sender<usize>>,
}

/// While one lives, lock state is re-checked every 2 s. A session holds one while it
/// subscribes to `session`.
#[must_use = "the lock poll stops when this is dropped"]
pub struct SessionInterest {
    subscribers: Arc<watch::Sender<usize>>,
}

impl Drop for SessionInterest {
    fn drop(&mut self) {
        self.subscribers.send_modify(|count| *count -= 1);
    }
}

impl SessionTopic {
    /// Starts the topic task. It ends when `shutdown` turns `true` or closes, or when a
    /// source is gone. Must be called inside a tokio runtime.
    pub fn spawn(sources: SessionSources, shutdown: watch::Receiver<bool>) -> SessionTopic {
        let facts = SessionFacts {
            keep_awake: sources.keep_awake.borrow().clone(),
            windows_inhibiting: u64::from(*sources.windows_inhibiting.borrow()),
            locked: None,
        };
        let (publisher, state) = watch::channel(Arc::new(PublishedSession { rev: 1, facts }));
        let (subscribers, subscriber_count) = watch::channel(0);
        tokio::spawn(run(sources, subscriber_count, publisher, shutdown));
        SessionTopic {
            state,
            subscribers: Arc::new(subscribers),
        }
    }

    /// The newest publication.
    pub fn state(&self) -> watch::Receiver<Arc<PublishedSession>> {
        self.state.clone()
    }

    /// Counts one more `session` subscriber until the result is dropped.
    pub fn watch_session(&self) -> SessionInterest {
        self.subscribers.send_modify(|count| *count += 1);
        SessionInterest {
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

async fn run(
    mut sources: SessionSources,
    mut subscribers: watch::Receiver<usize>,
    publisher: watch::Sender<Arc<PublishedSession>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut locked = None;
    let mut next_poll = None;
    loop {
        tokio::select! {
            () = stopped(&mut shutdown) => return,
            changed = sources.keep_awake.changed() => if changed.is_err() { return },
            changed = sources.windows_inhibiting.changed() => if changed.is_err() { return },
            changed = subscribers.changed() => {
                if changed.is_err() {
                    return;
                }
                let polling = *subscribers.borrow_and_update() > 0;
                // A first subscriber gets a fresh scan at once; without one, nothing
                // re-checks the old answer, so it is unknown.
                match (polling, next_poll) {
                    (true, None) => next_poll = Some(Instant::now()),
                    (false, _) => (next_poll, locked) = (None, None),
                    (true, Some(_)) => {}
                }
            }
            () = poll_due(next_poll) => {
                locked = scan_locked(&sources.scanner).await;
                next_poll = Some(Instant::now() + LOCK_POLL);
            }
        }
        let facts = SessionFacts {
            keep_awake: sources.keep_awake.borrow_and_update().clone(),
            windows_inhibiting: u64::from(*sources.windows_inhibiting.borrow_and_update()),
            locked,
        };
        publish(&publisher, facts);
    }
}

fn publish(publisher: &watch::Sender<Arc<PublishedSession>>, facts: SessionFacts) {
    publisher.send_if_modified(|published| {
        let changed = published.facts != facts;
        if changed {
            *published = Arc::new(PublishedSession {
                rev: published.rev + 1,
                facts,
            });
        }
        changed
    });
}

async fn poll_due(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// One scan off the runtime. `None` when it fails: unknown, which every caller treats as
/// the safe direction (11.3 rule 10; 17.6 `session.lock` rule 3).
pub async fn scan_locked(scanner: &Arc<ProcScanner>) -> Option<bool> {
    let scanner = Arc::clone(scanner);
    match tokio::task::spawn_blocking(move || scanner.scan()).await {
        Ok(Ok(facts)) => Some(facts.locked),
        Ok(Err(error)) => {
            tracing::debug!(%error, "lock state unknown");
            None
        }
        Err(error) => {
            tracing::warn!(%error, "lock scan task failed");
            None
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cannot start the lock command: {0}")]
pub struct LockError(#[source] pub ProcError);

impl LockError {
    pub fn code(&self) -> ErrorCode {
        ErrorCode::SpawnFailed
    }
}

/// Op `session.lock`: runs `[session] lock`, at most one child at a time.
pub struct Locker {
    runner: Arc<dyn Runner>,
    command: ChildCommand,
    scanner: Arc<ProcScanner>,
    child: Mutex<Option<Box<dyn Running>>>,
}

impl Locker {
    /// `None` without `[session] lock`: then the op is not offered (17.6 rule 1).
    pub fn new(
        runner: Arc<dyn Runner>,
        config: &SessionConfig,
        scanner: Arc<ProcScanner>,
    ) -> Option<Locker> {
        let command = ChildCommand::from(config.lock.as_ref()?);
        Some(Locker {
            runner,
            command,
            scanner,
            child: Mutex::new(None),
        })
    }

    /// Starts nothing while the screen is locked or the last lock child runs (rule 2).
    /// Unknown lock state with no child running starts it (rule 3).
    pub async fn lock(&self) -> Result<LockResult, LockError> {
        let locked = scan_locked(&self.scanner).await;
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        if locked == Some(true) || still_running(child.as_mut()) {
            return Ok(LockResult {
                already_locked: true,
            });
        }
        *child = Some(self.runner.spawn(self.command.clone()).map_err(LockError)?);
        Ok(LockResult {
            already_locked: false,
        })
    }
}

/// A child whose end cannot be read counts as ended: locking again is the safe direction.
fn still_running(child: Option<&mut Box<dyn Running>>) -> bool {
    child.is_some_and(|child| matches!(child.try_exit(), Ok(None)))
}

#[cfg(test)]
mod tests;
