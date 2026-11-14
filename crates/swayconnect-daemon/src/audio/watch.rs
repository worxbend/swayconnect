//! Keeps topic `audio` fresh (`docs/DESIGN.md` 11.6): a long-lived `pactl subscribe` child,
//! a fresh snapshot 100 ms after the first device event, and a respawn with backoff when the
//! child ends or cannot start.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch::{Receiver, Sender};
use tokio::time::{Instant, sleep, sleep_until};

use super::pactl::{self, Direction};
use super::snapshot::{Lists, Registry, Snapshot};
use super::{PACTL_TIMEOUT, Shared};
use crate::proc::{LineChild, Running};
use crate::session::stopped;

/// Events come in bursts (a plug changes card, sink and source); one snapshot follows them.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(100);
pub(super) const FIRST_BACKOFF: Duration = Duration::from_secs(1);
pub(super) const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A child that ran this long was healthy; the next respawn starts at [`FIRST_BACKOFF`].
pub(super) const HEALTHY_RUN: Duration = Duration::from_secs(60);

/// Whether the task goes on.
enum Flow {
    Continue,
    Stop,
}

pub(super) struct Watcher {
    shared: Arc<Shared>,
    registry: Registry,
    snapshot: Sender<Arc<Snapshot>>,
}

impl Watcher {
    pub(super) fn new(shared: Arc<Shared>, snapshot: Sender<Arc<Snapshot>>) -> Watcher {
        Watcher {
            shared,
            registry: Registry::default(),
            snapshot,
        }
    }

    pub(super) async fn run(mut self, mut shutdown: Receiver<bool>) {
        let mut backoff = FIRST_BACKOFF;
        loop {
            let started = Instant::now();
            let command = self.shared.pactl(pactl::SUBSCRIBE_ARGS);
            let flow = match self.shared.runner.spawn_lines(command) {
                Ok(child) => self.follow(child, &mut shutdown).await,
                Err(error) => {
                    tracing::warn!(%error, "audio: cannot start pactl subscribe");
                    self.refresh_unless_stopped(&mut shutdown).await
                }
            };
            if matches!(flow, Flow::Stop) {
                return;
            }
            if started.elapsed() >= HEALTHY_RUN {
                backoff = FIRST_BACKOFF;
            }
            tokio::select! {
                () = stopped(&mut shutdown) => return,
                () = sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }

    /// Reads events until the child's stdout ends. The snapshot taken after the start
    /// covers whatever happened while no child ran.
    async fn follow(&mut self, started: LineChild, shutdown: &mut Receiver<bool>) -> Flow {
        let LineChild {
            mut child,
            mut lines,
        } = started;
        let mut flow = self.refresh_unless_stopped(shutdown).await;
        let mut due = None;
        while matches!(flow, Flow::Continue) {
            tokio::select! {
                biased;
                () = stopped(shutdown) => flow = Flow::Stop,
                () = deadline(due) => {
                    due = None;
                    flow = self.refresh_unless_stopped(shutdown).await;
                }
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        if due.is_none() && pactl::is_device_event(&line) {
                            due = Some(Instant::now() + DEBOUNCE);
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "audio: cannot read pactl subscribe");
                        break;
                    }
                },
            }
        }
        stop_child(child.as_mut()).await;
        flow
    }

    /// A snapshot that a stop can cut short; a cut run kills its pactl child.
    async fn refresh_unless_stopped(&mut self, shutdown: &mut Receiver<bool>) -> Flow {
        tokio::select! {
            () = stopped(shutdown) => Flow::Stop,
            () = self.refresh() => Flow::Continue,
        }
    }

    async fn refresh(&mut self) {
        let lists = self.read_lists().await;
        let fresh = self.registry.build(lists, self.shared.max_volume_pct);
        self.snapshot.send_if_modified(|current| {
            let changed = **current != fresh;
            if changed {
                *current = Arc::new(fresh);
            }
            changed
        });
    }

    /// The four reads of a snapshot. `None` at the first failure: then the state is
    /// "not reachable".
    async fn read_lists(&self) -> Option<Lists> {
        let sinks = self.read(&pactl::list_args(Direction::Sink)).await?;
        let sources = self.read(&pactl::list_args(Direction::Source)).await?;
        let default_sink = self.read(&pactl::default_args(Direction::Sink)).await?;
        let default_source = self.read(&pactl::default_args(Direction::Source)).await?;
        let parsed = pactl::parse_list(Direction::Sink, &sinks)
            .and_then(|sinks| Ok((sinks, pactl::parse_list(Direction::Source, &sources)?)));
        match parsed {
            Ok((sinks, sources)) => Some(Lists {
                sinks,
                sources,
                default_sink: pactl::parse_default(&default_sink),
                default_source: pactl::parse_default(&default_source),
            }),
            Err(error) => {
                tracing::warn!(%error, "audio: unreadable pactl list");
                None
            }
        }
    }

    /// The stdout of one successful pactl run.
    async fn read(&self, args: &[&str]) -> Option<Vec<u8>> {
        let command = self.shared.pactl(args.iter().copied());
        match self.shared.runner.run(command, PACTL_TIMEOUT).await {
            Ok(finished) if finished.exit.success() => Some(finished.stdout.bytes),
            Ok(finished) => {
                tracing::debug!(exit = ?finished.exit, "audio: pactl read failed");
                None
            }
            Err(error) => {
                tracing::warn!(%error, "audio: pactl read failed");
                None
            }
        }
    }
}

/// Resolves at `at`; never for `None`.
async fn deadline(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Kills the child (nothing for one that ended) and waits for its end.
async fn stop_child(child: &mut dyn Running) {
    if let Err(error) = child.kill() {
        tracing::warn!(%error, "audio: cannot kill pactl subscribe");
        return;
    }
    if let Err(error) = child.exited().await {
        tracing::warn!(%error, "audio: the end of pactl subscribe is unknown");
    }
}

#[cfg(test)]
mod tests;
