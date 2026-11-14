//! Starts the M2b services (`docs/DESIGN.md` 11.6, 11.7): audio, media, apps with the
//! launcher, and the remote keyboard. Slice D-WIRE.2b.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use swayconnect_protocol::IconHash;
use swayconnect_sway::SwayTarget;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::apps::index::IndexDirs;
use crate::apps::{AppIcons, AppsService, Launcher, PublishedApps};
use crate::audio::{AudioDeps, AudioService};
use crate::config::Config;
use crate::icons::IconService;
use crate::input::{Guard, InputService, KeyboardSink, SwayFocus, VirtualKeyboard, WaylandTarget};
use crate::lockstate::ProcScanner;
use crate::media::mpris::MprisBackend;
use crate::media::{MediaBackend, MediaService};
use crate::notify::Notifier;
use crate::proc::Runner;
use crate::session::stopped;

/// A subscription to `apps` reads the desktop entries again when the last read is older.
const APPS_RESCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Where `input.*` sends its keys.
pub enum KeyboardHost {
    /// The compositor's virtual keyboard. It connects at the first delivery.
    Wayland(WaylandTarget),
    /// A ready sink (tests).
    Sink(Arc<dyn KeyboardSink>),
}

impl KeyboardHost {
    /// No Wayland socket can be named, so every delivery fails with `backend_unavailable`.
    pub fn none() -> KeyboardHost {
        KeyboardHost::Wayland(WaylandTarget::Discover {
            runtime_dir: None,
            display: None,
        })
    }
}

/// Where topic `media` reads its players.
pub enum MediaHost {
    /// MPRIS on the session bus at this address. `None`: no bus, so no players.
    Bus(Option<String>),
    /// A ready backend (tests: `media::fake::FakeMedia`).
    Backend(Arc<dyn MediaBackend>),
}

/// What the M2b services take from the host besides the config.
pub struct ServiceHost {
    pub runner: Arc<dyn Runner>,
    /// Where the input guard reads processes: `/proc` outside tests.
    pub proc_root: PathBuf,
    /// The input guard and `launcher.open` read focus over their own connections.
    pub sway: SwayTarget,
    pub keyboard: KeyboardHost,
    pub media: MediaHost,
    pub apps: IndexDirs,
    pub icons: Option<Arc<IconService>>,
}

impl ServiceHost {
    /// No keyboard, no bus, no desktop entries and no icons: nothing outside the runner.
    pub fn offline(runner: Arc<dyn Runner>, proc_root: PathBuf, sway: SwayTarget) -> ServiceHost {
        ServiceHost {
            runner,
            proc_root,
            sway,
            keyboard: KeyboardHost::none(),
            media: MediaHost::Bus(None),
            apps: IndexDirs::default(),
            icons: None,
        }
    }
}

/// The M2b services every session shares. Clones are cheap.
#[derive(Clone)]
pub struct Services {
    pub audio: AudioService,
    pub media: Arc<MediaService>,
    pub apps: Arc<AppsCatalog>,
    pub launcher: Arc<Launcher>,
    pub input: InputService,
    /// Reads the focused window when `launcher.open` runs.
    pub focus: Arc<SwayFocus>,
    /// Shows the connect notice of a device with a tier 2 scope (`docs/DESIGN.md` 11.8).
    pub notifier: Notifier,
}

/// Starts the services; their tasks end when `shutdown` turns `true` or closes. The entry
/// scan runs off the runtime. `Err`: the keyboard thread did not start.
pub async fn start(
    config: &Config,
    host: ServiceHost,
    shutdown: &watch::Receiver<bool>,
) -> io::Result<Services> {
    let ServiceHost {
        runner,
        proc_root,
        sway,
        keyboard,
        media,
        apps,
        icons,
    } = host;
    let notifier = Notifier::new(Arc::clone(&runner), &config.notify);
    let audio_deps = AudioDeps {
        runner: Arc::clone(&runner),
        notifier: notifier.clone(),
    };
    let audio = AudioService::spawn(&config.audio, audio_deps, shutdown.clone());
    let backend: Arc<dyn MediaBackend> = match media {
        MediaHost::Bus(address) => MprisBackend::spawn(address, shutdown.clone()),
        MediaHost::Backend(backend) => backend,
    };
    let media = Arc::new(MediaService::spawn(
        backend,
        icons.clone(),
        shutdown.clone(),
    ));
    let apps = AppsCatalog::start(config, apps, Arc::clone(&runner), icons, shutdown).await;
    let launcher = Arc::new(Launcher::new(config.launcher.argv.clone(), runner));
    let sink: Arc<dyn KeyboardSink> = match keyboard {
        KeyboardHost::Wayland(target) => {
            Arc::new(VirtualKeyboard::spawn(target, config.input.seat.clone())?)
        }
        KeyboardHost::Sink(sink) => sink,
    };
    let uid = rustix::process::getuid().as_raw();
    let guard = Guard::new(
        ProcScanner::new(proc_root, uid, &config.session),
        SwayFocus::new(sway.clone()),
        launcher.view(),
    );
    Ok(Services {
        audio,
        media,
        apps: Arc::new(apps),
        launcher,
        input: InputService::new(sink, guard, notifier.clone()),
        focus: Arc::new(SwayFocus::new(sway)),
        notifier,
    })
}

/// Topic `apps` and the launches, with the rescan cadence of the entry index.
pub struct AppsCatalog {
    service: Arc<AppsService>,
    next_scan: Mutex<Instant>,
}

impl AppsCatalog {
    async fn start(
        config: &Config,
        dirs: IndexDirs,
        runner: Arc<dyn Runner>,
        icons: Option<Arc<IconService>>,
        shutdown: &watch::Receiver<bool>,
    ) -> AppsCatalog {
        let entry_icons: Arc<dyn AppIcons> = match &icons {
            Some(icons) => Arc::clone(icons) as Arc<dyn AppIcons>,
            None => Arc::new(NoIcons),
        };
        let apps_config = config.apps.clone();
        let scan = move || AppsService::new(apps_config, dirs, runner, entry_icons);
        let service = match tokio::task::spawn_blocking(scan).await {
            Ok(service) => Arc::new(service),
            Err(error) => std::panic::resume_unwind(error.into_panic()),
        };
        if let Some(icons) = icons {
            let finished = icons.finished();
            tokio::spawn(refresh_icons(
                Arc::clone(&service),
                finished,
                shutdown.clone(),
            ));
        }
        AppsCatalog {
            service,
            next_scan: Mutex::new(Instant::now() + APPS_RESCAN_INTERVAL),
        }
    }

    pub fn service(&self) -> &AppsService {
        &self.service
    }

    /// Topic `apps`. Starts a rescan in the background when the last one is 30 s old, so a
    /// newly installed app shows on the next subscription after that.
    pub fn subscribe(&self) -> watch::Receiver<Arc<PublishedApps>> {
        if self.is_scan_due() {
            let service = Arc::clone(&self.service);
            tokio::task::spawn_blocking(move || service.rescan());
        }
        self.service.subscribe()
    }

    fn is_scan_due(&self) -> bool {
        let now = Instant::now();
        let mut next_scan = self
            .next_scan
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let is_due = now >= *next_scan;
        if is_due {
            *next_scan = now + APPS_RESCAN_INTERVAL;
        }
        is_due
    }
}

/// A daemon without icon lookup: no entry has an icon.
struct NoIcons;

impl AppIcons for NoIcons {
    fn entry_icon(&self, _entry_id: &str) -> Option<IconHash> {
        None
    }
}

/// Publishes `apps` again whenever the icon service finished a job, so new icons appear.
async fn refresh_icons(
    apps: Arc<AppsService>,
    mut finished: watch::Receiver<u64>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            () = stopped(&mut shutdown) => return,
            changed = finished.changed() => match changed {
                Ok(()) => apps.refresh_icons(),
                Err(_) => return,
            },
        }
    }
}
