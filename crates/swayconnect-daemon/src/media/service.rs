//! [`MediaService`]: one task keeps the `media` facts current; the ops look players up in
//! the newest publication.

use std::sync::Arc;
use std::time::Duration;

use swayconnect_protocol::{IconHash, MediaCommand};
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

use super::facts::{MediaFacts, TrackedPlayer, Tracker};
use super::{BackendError, MediaBackend, MediaError, PlayerReport};
use crate::icons::{IconKey, IconService};
use crate::session::stopped;

/// Players send bursts of property changes; one re-read follows a burst.
pub const DEBOUNCE: Duration = Duration::from_millis(200);
/// The longest a full re-read of the players may take.
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest a player may take to answer a command (`timeout` after that).
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

/// One publication of the topic. `rev` grows by one per change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedMedia {
    pub rev: u64,
    pub facts: MediaFacts,
}

pub struct MediaService {
    backend: Arc<dyn MediaBackend>,
    state: watch::Receiver<Arc<PublishedMedia>>,
}

impl MediaService {
    /// Starts the topic task: it reads the players at once and again after each change the
    /// backend reports. It ends when `shutdown` turns `true` or closes. `icons` fills
    /// `Player.icon` from the desktop entry. Must be called inside a tokio runtime.
    pub fn spawn(
        backend: Arc<dyn MediaBackend>,
        icons: Option<Arc<IconService>>,
        shutdown: watch::Receiver<bool>,
    ) -> MediaService {
        let first = PublishedMedia {
            rev: 1,
            facts: MediaFacts::default(),
        };
        let (publisher, state) = watch::channel(Arc::new(first));
        tokio::spawn(run(Arc::clone(&backend), icons, publisher, shutdown));
        MediaService { backend, state }
    }

    /// The newest publication.
    pub fn state(&self) -> watch::Receiver<Arc<PublishedMedia>> {
        self.state.clone()
    }

    /// Ops `media.play_pause`, `media.next`, `media.previous` after the generic checks:
    /// `not_found` for an id the newest state does not hold, then `unsupported_target` when
    /// the player does not allow the command, then the backend call.
    pub async fn command(&self, player_id: u64, command: MediaCommand) -> Result<(), MediaError> {
        let bus_name = {
            let published = self.state.borrow();
            let fact = published
                .facts
                .find(player_id)
                .ok_or(MediaError::NotFound)?;
            if !fact.player.allows(command) {
                return Err(MediaError::UnsupportedTarget);
            }
            fact.bus_name.clone()
        };
        match timeout(COMMAND_TIMEOUT, self.backend.send(&bus_name, command)).await {
            Ok(result) => result.map_err(MediaError::from),
            Err(_) => Err(MediaError::Backend(BackendError::Timeout)),
        }
    }
}

async fn run(
    backend: Arc<dyn MediaBackend>,
    icons: Option<Arc<IconService>>,
    publisher: watch::Sender<Arc<PublishedMedia>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut changes = backend.changes();
    let mut icons_done = icons.as_ref().map(|icons| icons.finished());
    let mut tracker = Tracker::default();
    changes.mark_unchanged();
    let mut players = refresh(backend.as_ref(), &mut tracker).await;
    publish(
        &publisher,
        MediaFacts::build(&players, |report| icon_of(icons.as_deref(), report)),
    );
    loop {
        tokio::select! {
            () = stopped(&mut shutdown) => return,
            changed = changes.changed() => {
                if changed.is_err() {
                    return;
                }
                tokio::select! {
                    () = stopped(&mut shutdown) => return,
                    () = sleep(DEBOUNCE) => {}
                }
                changes.mark_unchanged();
                players = refresh(backend.as_ref(), &mut tracker).await;
            }
            // An icon job ended: a player's icon may be ready now.
            changed = icons_finished(icons_done.as_mut()) => {
                if changed.is_err() {
                    icons_done = None;
                }
            }
        }
        publish(
            &publisher,
            MediaFacts::build(&players, |report| icon_of(icons.as_deref(), report)),
        );
    }
}

/// A failed read publishes no players, since none can be reached, but the tracker keeps
/// every id and mark: one missed read must not give the players new identities.
async fn refresh(backend: &dyn MediaBackend, tracker: &mut Tracker) -> Vec<TrackedPlayer> {
    match timeout(REFRESH_TIMEOUT, backend.players()).await {
        Ok(Ok(players)) => tracker.observe(players.reports, &players.unread),
        Ok(Err(error)) => {
            tracing::debug!(%error, "media players not read");
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("media players not read: the bus did not answer in time");
            Vec::new()
        }
    }
}

fn icon_of(icons: Option<&IconService>, report: &PlayerReport) -> Option<IconHash> {
    let key = IconKey {
        app_id: report.desktop_entry.clone(),
        ..IconKey::default()
    };
    icons?.icon_for(&key, None)
}

async fn icons_finished(
    done: Option<&mut watch::Receiver<u64>>,
) -> Result<(), watch::error::RecvError> {
    match done {
        Some(done) => done.changed().await,
        None => std::future::pending().await,
    }
}

fn publish(publisher: &watch::Sender<Arc<PublishedMedia>>, facts: MediaFacts) {
    publisher.send_if_modified(|published| {
        let changed = published.facts != facts;
        if changed {
            *published = Arc::new(PublishedMedia {
                rev: published.rev + 1,
                facts,
            });
        }
        changed
    });
}

#[cfg(test)]
mod tests;
