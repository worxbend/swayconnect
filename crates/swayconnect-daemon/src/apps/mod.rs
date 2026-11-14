//! Topic `apps`, `app.launch` and `launcher.open` (`docs/PROTOCOL.md` 17.11,
//! `docs/DESIGN.md` 11.6, D28). Written by slice D-APPS.
//!
//! The daemon owns every word of a launch argv: the `[apps]` config and the `Exec` tokens
//! of its own desktop entry index. A client names an app only by an `entry_id` that is
//! matched exactly against the published state.

mod config;
pub mod exec;
pub mod index;
pub mod launch;
pub mod topic;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use swayconnect_protocol::limits::MAX_RECENT_APPS;
use swayconnect_protocol::proof::random_bytes;
use swayconnect_protocol::{Apps, EntryId, ErrorCode, IconHash, Scope};
use tokio::sync::watch;

pub use self::config::{AppsConfig, LauncherConfig};
use self::index::{IndexDirs, IndexedApp};
pub use self::launch::{Launcher, OpenError, OpenLauncher, StartError, held_for};
use crate::icons::{IconKey, IconService};
use crate::proc::Runner;

/// Icon hashes for desktop entries. [`IconService`] outside tests.
pub trait AppIcons: Send + Sync + 'static {
    /// The hash when the PNG exists; otherwise `None`, and the lookup may start in the
    /// background.
    fn entry_icon(&self, entry_id: &str) -> Option<IconHash>;
}

impl AppIcons for IconService {
    fn entry_icon(&self, entry_id: &str) -> Option<IconHash> {
        let key = IconKey {
            app_id: Some(entry_id.to_owned()),
            ..IconKey::default()
        };
        self.icon_for(&key, None)
    }
}

/// One publication of topic `apps`. `rev` grows by one per change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedApps {
    pub rev: u64,
    pub state: Apps,
}

/// Why `app.launch` failed.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("the entry is not in the apps state")]
    NotFound,
    #[error("the entry is risky and needs scope launch.risky")]
    NeedsLaunchRisky,
    #[error("the system random source failed")]
    Random,
    #[error("cannot start the app: {0}")]
    Start(#[from] StartError),
}

impl LaunchError {
    pub fn code(&self) -> ErrorCode {
        match self {
            LaunchError::NotFound => ErrorCode::NotFound,
            LaunchError::NeedsLaunchRisky => ErrorCode::Forbidden,
            LaunchError::Random | LaunchError::Start(_) => ErrorCode::SpawnFailed,
        }
    }

    /// `detail.scope` of `forbidden` (17.16 step 8b).
    pub fn missing_scope(&self) -> Option<Scope> {
        matches!(self, LaunchError::NeedsLaunchRisky).then_some(Scope::LaunchRisky)
    }
}

/// An entry that passed the checks of 17.16 step 8 and 8b. The caller focuses the
/// requested workspace (if any) between [`AppsService::prepare_launch`] and
/// [`AppsService::launch`], so a refused entry never switches the workspace.
#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    app: IndexedApp,
}

/// The index, the recent list and the published state of topic `apps`.
pub struct AppsService {
    config: AppsConfig,
    dirs: IndexDirs,
    runner: Arc<dyn Runner>,
    icons: Arc<dyn AppIcons>,
    inner: Mutex<Inner>,
    state: watch::Sender<Arc<PublishedApps>>,
}

struct Inner {
    index: Vec<IndexedApp>,
    /// Most recent first, at most 8, only successful launches.
    recent: Vec<String>,
}

impl AppsService {
    /// Scans the entries and publishes the first state as rev 1. Blocking file I/O: call it
    /// off the async runtime.
    pub fn new(
        config: AppsConfig,
        dirs: IndexDirs,
        runner: Arc<dyn Runner>,
        icons: Arc<dyn AppIcons>,
    ) -> AppsService {
        let inner = Inner {
            index: index::scan(&dirs, &config),
            recent: Vec::new(),
        };
        let first = PublishedApps {
            rev: 1,
            state: build_state(&inner, icons.as_ref()),
        };
        AppsService {
            config,
            dirs,
            runner,
            icons,
            inner: Mutex::new(inner),
            state: watch::channel(Arc::new(first)).0,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<PublishedApps>> {
        self.state.subscribe()
    }

    /// Reads the desktop entries again and publishes on a change. Blocking file I/O.
    pub fn rescan(&self) {
        let index = index::scan(&self.dirs, &self.config);
        let mut inner = self.lock();
        inner.index = index;
        self.publish(&inner);
    }

    /// Builds the state again, so icons resolved since the last build appear. Call it when
    /// [`IconService::finished`] changes.
    pub fn refresh_icons(&self) {
        let inner = self.lock();
        self.publish(&inner);
    }

    /// 17.16 step 8 (`not_found`, exact match against the published state) and step 8b
    /// (`launch.risky` for a risky entry).
    pub fn prepare_launch(
        &self,
        entry_id: &EntryId,
        may_launch_risky: bool,
    ) -> Result<PreparedLaunch, LaunchError> {
        let inner = self.lock();
        let published = self.state.borrow();
        let listed = published
            .state
            .apps
            .iter()
            .find(|app| app.entry_id == entry_id.as_str())
            .ok_or(LaunchError::NotFound)?;
        if listed.risky && !may_launch_risky {
            return Err(LaunchError::NeedsLaunchRisky);
        }
        let app = inner
            .index
            .iter()
            .find(|app| app.entry_id == listed.entry_id)
            .ok_or(LaunchError::NotFound)?;
        Ok(PreparedLaunch { app: app.clone() })
    }

    /// Starts the entry in its own `app-swayconnect-*` scope and puts it first in `recent`.
    /// `Ok` means started, not that a window appeared.
    pub async fn launch(&self, prepared: PreparedLaunch) -> Result<(), LaunchError> {
        let suffix = random_bytes::<4>().map_err(|_| LaunchError::Random)?;
        let suffix: String = suffix.iter().map(|byte| format!("{byte:02x}")).collect();
        let command = launch::launch_command(&self.config, &prepared.app, &suffix);
        launch::start(self.runner.as_ref(), command).await?;
        let mut inner = self.lock();
        let id = prepared.app.entry_id;
        inner.recent.retain(|recent| *recent != id);
        inner.recent.insert(0, id);
        inner.recent.truncate(MAX_RECENT_APPS);
        self.publish(&inner);
        Ok(())
    }

    /// Publishes the state of `inner` when it differs from the published one. The caller
    /// holds the lock, so publications keep the order of the changes.
    fn publish(&self, inner: &Inner) {
        let state = build_state(inner, self.icons.as_ref());
        self.state.send_if_modified(|published| {
            let changed = published.state != state;
            if changed {
                *published = Arc::new(PublishedApps {
                    rev: published.rev + 1,
                    state,
                });
            }
            changed
        });
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The topic state of `inner`; only entries that name an icon ask the icon service.
fn build_state(inner: &Inner, icons: &dyn AppIcons) -> Apps {
    topic::build_state(&inner.index, &inner.recent, |app| {
        app.has_icon
            .then(|| icons.entry_icon(&app.entry_id))
            .flatten()
    })
}

#[cfg(test)]
mod tests;
