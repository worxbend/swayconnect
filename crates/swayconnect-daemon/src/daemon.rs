//! Starts and stops the daemon's tasks (`docs/DESIGN.md` 4.1, 4.7). Takes explicit values
//! only: `main.rs` reads the environment, tests pass temp directories.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use swayconnect_protocol::fingerprint::encode_base64url;
use swayconnect_sway::SwayTarget;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use self::controls::HostParts;
use self::services::{KeyboardHost, MediaHost, ServiceHost, Services};
use crate::admin::{AdminServer, Advertised};
use crate::apps::index::IndexDirs;
use crate::config::{Config, StartError};
use crate::icons::raster::ChildRasteriser;
use crate::icons::{IconService, IconSettings, IconsConfig};
use crate::keep_awake::KeepAwake;
use crate::listener;
use crate::paths::{Paths, own_uid};
use crate::proc::{ChildEnv, RealRunner, Runner};
use crate::security::Security;
use crate::session::Shared;
use crate::sway_handle::SwayHandle;

pub mod controls;
pub mod services;

const EXIT_RUNTIME_FAILURE: u8 = 1;
const EXIT_CONFIG_ERROR: u8 = 2;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error(transparent)]
    Start(#[from] StartError),
    #[error("cannot listen on {address}: {source}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("cannot start the icon service: {0}")]
    Icons(#[source] io::Error),
    #[error("cannot start the remote keyboard: {0}")]
    Keyboard(#[source] io::Error),
}

impl DaemonError {
    /// The process exit code of `docs/DESIGN.md` 4.7. A failed bind is 1, so that systemd
    /// tries again when the LAN address comes up.
    pub fn exit_code(&self) -> u8 {
        match self {
            DaemonError::Start(_) => EXIT_CONFIG_ERROR,
            DaemonError::Bind { .. } | DaemonError::Icons(_) | DaemonError::Keyboard(_) => {
                EXIT_RUNTIME_FAILURE
            }
        }
    }
}

pub struct Daemon;

/// What the icon service reads and runs (`docs/DESIGN.md` 11.5). `main.rs` builds it from
/// the environment; tests point it at temp directories.
#[derive(Debug, Clone)]
pub struct IconSources {
    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`, as `icons::search_dirs` orders them.
    pub search_dirs: Vec<PathBuf>,
    /// `$XDG_CACHE_HOME/swayconnect`; the service keeps its PNGs below it.
    pub cache_dir: PathBuf,
    /// The `swayconnectd` binary, run as `--raster` once per untrusted icon file.
    pub raster_program: PathBuf,
}

/// What the daemon takes from the host besides its config and paths. `main.rs` builds it
/// from the environment; tests point it at temp directories and fake programs.
pub struct Host {
    pub target: SwayTarget,
    /// `None`: no icon lookup. Every window icon is `null` and `icon.get` lists every hash as
    /// `missing`.
    pub icons: Option<IconSources>,
    /// Starts every child program (`docs/DESIGN.md` 11.3 rule 8).
    pub runner: Arc<dyn Runner>,
    /// Where the lock heuristic and the input guard read processes: `/proc` outside tests.
    pub proc_root: PathBuf,
    /// Where `input.*` types (M2b).
    pub keyboard: KeyboardHost,
    /// Where topic `media` finds its players.
    pub media: MediaHost,
    /// Where topic `apps` reads desktop entries.
    pub apps: IndexDirs,
}

impl Host {
    /// No icon lookup, the real `/proc`, children with an empty environment, and nothing of
    /// the desktop session: no keyboard, no session bus, no desktop entries.
    pub fn new(target: SwayTarget) -> Host {
        Host {
            target,
            icons: None,
            runner: Arc::new(RealRunner::new(ChildEnv::default())),
            proc_root: PathBuf::from("/proc"),
            keyboard: KeyboardHost::none(),
            media: MediaHost::Bus(None),
            apps: IndexDirs::default(),
        }
    }
}

/// A running daemon. Dropping it without [`DaemonHandle::stop`] also stops the tasks, but
/// waits for nothing and can leave `admin.sock` behind.
pub struct DaemonHandle {
    shutdown: watch::Sender<bool>,
    read_only: Arc<AtomicBool>,
    keep_awake: KeepAwake,
    services: Services,
    bound: Vec<SocketAddr>,
    fingerprint: [u8; 32],
    tasks: Vec<JoinHandle<()>>,
}

impl Daemon {
    /// A daemon without icon lookup: every window icon is `null` and `icon.get` lists every
    /// hash as `missing`.
    pub async fn start(
        config: Config,
        paths: Paths,
        target: SwayTarget,
    ) -> Result<DaemonHandle, DaemonError> {
        Daemon::start_on(config, paths, Host::new(target)).await
    }

    /// A daemon whose icon service reads `icons`.
    pub async fn start_with_icons(
        config: Config,
        paths: Paths,
        target: SwayTarget,
        icons: IconSources,
    ) -> Result<DaemonHandle, DaemonError> {
        let host = Host {
            icons: Some(icons),
            ..Host::new(target)
        };
        Daemon::start_on(config, paths, host).await
    }

    /// A daemon that takes everything outside its config and paths from `host`.
    pub async fn start_on(
        config: Config,
        paths: Paths,
        host: Host,
    ) -> Result<DaemonHandle, DaemonError> {
        let Host {
            target,
            icons: icon_sources,
            runner,
            proc_root,
            keyboard,
            media,
            apps,
        } = host;
        let security = Security::open(&config, &paths)?;
        // Before the listeners: the address of a running daemon is in use, and that must
        // not hide the real reason.
        AdminServer::claim(&paths)?;
        let mut listeners = Vec::new();
        let mut bound = Vec::new();
        for &address in &config.listen {
            let bind_error = |source| DaemonError::Bind { address, source };
            let listener = TcpListener::bind(address).await.map_err(bind_error)?;
            bound.push(listener.local_addr().map_err(bind_error)?);
            listeners.push(listener);
        }
        let icons = match icon_sources {
            Some(sources) => Some(Arc::new(start_icons(sources, config.icons.clone())?)),
            None => None,
        };
        // A failed start drops the sender, which also stops the tasks started so far.
        let (shutdown, stop) = watch::channel(false);
        let service_host = ServiceHost {
            runner: Arc::clone(&runner),
            proc_root: proc_root.clone(),
            sway: target.clone(),
            keyboard,
            media,
            apps,
            icons: icons.clone(),
        };
        // Before the services: their watchers log at once (a missing `pactl`), and that must
        // not bury a socket path that cannot be bound.
        let admin = AdminServer::bind(&paths)?;
        let services = match services::start(&config, service_host, &stop).await {
            Ok(services) => services,
            Err(error) => {
                // A failed start leaves no `admin.sock` that nobody serves.
                drop(admin);
                if let Err(remove_error) = std::fs::remove_file(paths.admin_socket()) {
                    tracing::warn!(error = %remove_error, "admin socket not removed");
                }
                return Err(DaemonError::Keyboard(error));
            }
        };

        let read_only = Arc::new(AtomicBool::new(config.read_only));
        let fingerprint = security.fingerprint();
        log_start(&bound, &fingerprint, config.read_only);
        let sway = Arc::new(SwayHandle::spawn_with_icons(
            target,
            stop.clone(),
            icons.clone(),
        ));
        let host_parts = HostParts { runner, proc_root };
        let controls = controls::start(&config, host_parts, &sway, admin.audit(), &stop);
        let keep_awake = controls.keep_awake.clone();
        let shared = Shared {
            security: security.clone(),
            // The supervisor runs until shutdown; sessions keep the receiver alive.
            desktop: sway.state(),
            sway,
            server_name: Arc::from(config.server_name.as_str()),
            read_only: Arc::clone(&read_only),
            shutdown: stop.clone(),
            icons,
            controls,
            services: services.clone(),
        };
        let advertised = Advertised {
            server_name: config.server_name,
            bound: bound.clone(),
        };
        let mut tasks = vec![tokio::spawn(controls::release_on_revoke(
            keep_awake.clone(),
            security.clone(),
            stop.clone(),
        ))];
        tasks.push(tokio::spawn(admin.run(security, advertised, stop)));
        for listener in listeners {
            tasks.push(tokio::spawn(listener::run(listener, shared.clone())));
        }
        Ok(DaemonHandle {
            shutdown,
            read_only,
            keep_awake,
            services,
            bound,
            fingerprint,
            tasks,
        })
    }
}

fn start_icons(sources: IconSources, config: IconsConfig) -> Result<IconService, DaemonError> {
    let settings = IconSettings {
        search_dirs: sources.search_dirs,
        cache_dir: Some(sources.cache_dir),
        uid: own_uid().map_err(DaemonError::Icons)?,
        proc_root: PathBuf::from("/proc"),
        config,
    };
    let rasteriser = ChildRasteriser::new(sources.raster_program);
    IconService::start(settings, rasteriser).map_err(DaemonError::Icons)
}

fn log_start(bound: &[SocketAddr], fingerprint: &[u8; 32], read_only: bool) {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        fingerprint = %encode_base64url(fingerprint),
        read_only,
        "swayconnectd started"
    );
    for address in bound {
        tracing::info!(%address, "listening");
        if !address.ip().is_loopback() {
            tracing::info!(
                port = address.port(),
                "a host firewall must allow this TCP port from the allow_from networks"
            );
        }
    }
}

impl DaemonHandle {
    /// The listen addresses with their real ports.
    pub fn bound(&self) -> &[SocketAddr] {
        &self.bound
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// The panic switch: until the daemon restarts, every action is refused and new
    /// sessions see `read_only`. Keep-awake ends too (`docs/PROTOCOL.md` 17.8 rule 6), input
    /// stops and the launcher is killed (`docs/DESIGN.md` 11.8), in a task, so the switch
    /// never waits. Must be called inside a tokio runtime.
    pub fn enter_read_only(&self) {
        self.read_only.store(true, Ordering::SeqCst);
        tracing::info!("read-only mode entered; it lasts until the daemon restarts");
        // At once: a running request stops before its next part.
        self.services.input.abort();
        let keep_awake = self.keep_awake.clone();
        let launcher = Arc::clone(&self.services.launcher);
        // Both refuse a later start themselves, so a request that passed the read-only
        // check before the switch starts nothing after it. Side by side: a slow launcher
        // kill does not delay the end of keep-awake.
        tokio::spawn(async move {
            tokio::join!(launcher.close(), keep_awake.close());
        });
    }

    /// Ends keep-awake, stops accepting, lets the sessions send `bye`, removes `admin.sock`.
    pub async fn stop(self) {
        tracing::info!("stopping");
        // The keep-awake task also ends its inhibitor on shutdown; this waits for the kill.
        self.keep_awake.release().await;
        self.shutdown.send_replace(true);
        for task in self.tasks {
            if let Err(error) = task.await {
                tracing::error!(%error, "daemon task failed");
            }
        }
    }
}

#[cfg(test)]
mod tests;
