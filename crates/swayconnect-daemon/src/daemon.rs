//! Starts and stops the daemon's tasks (`docs/DESIGN.md` 4.1, 4.7). Takes explicit values
//! only: `main.rs` reads the environment, tests pass temp directories.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use swayconnect_protocol::fingerprint::encode_base64url;
use swayconnect_sway::SwayTarget;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::admin::{AdminServer, Advertised};
use crate::config::{Config, StartError};
use crate::listener;
use crate::paths::Paths;
use crate::security::Security;
use crate::session::Shared;
use crate::sway_handle::SwayHandle;

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
}

impl DaemonError {
    /// The process exit code of `docs/DESIGN.md` 4.7. A failed bind is 1, so that systemd
    /// tries again when the LAN address comes up.
    pub fn exit_code(&self) -> u8 {
        match self {
            DaemonError::Start(_) => EXIT_CONFIG_ERROR,
            DaemonError::Bind { .. } => EXIT_RUNTIME_FAILURE,
        }
    }
}

pub struct Daemon;

/// A running daemon. Dropping it without [`DaemonHandle::stop`] also stops the tasks, but
/// waits for nothing and can leave `admin.sock` behind.
pub struct DaemonHandle {
    shutdown: watch::Sender<bool>,
    read_only: Arc<AtomicBool>,
    bound: Vec<SocketAddr>,
    fingerprint: [u8; 32],
    tasks: Vec<JoinHandle<()>>,
}

impl Daemon {
    pub async fn start(
        config: Config,
        paths: Paths,
        target: SwayTarget,
    ) -> Result<DaemonHandle, DaemonError> {
        let security = Security::open(&config, &paths)?;
        // Before the listeners: the address of a running daemon is in use, and that must
        // not hide the real reason.
        AdminServer::claim(&paths)?;
        let mut listeners = Vec::new();
        let mut bound = Vec::new();
        for address in config.listen {
            let bind_error = |source| DaemonError::Bind { address, source };
            let listener = TcpListener::bind(address).await.map_err(bind_error)?;
            bound.push(listener.local_addr().map_err(bind_error)?);
            listeners.push(listener);
        }
        // After the listeners: a start that fails on them leaves no dead `admin.sock` behind.
        let admin = AdminServer::bind(&paths)?;

        let (shutdown, stop) = watch::channel(false);
        let read_only = Arc::new(AtomicBool::new(config.read_only));
        let fingerprint = security.fingerprint();
        log_start(&bound, &fingerprint, config.read_only);
        let sway = Arc::new(SwayHandle::spawn(target, stop.clone()));
        let shared = Shared {
            security: security.clone(),
            // The supervisor runs until shutdown; sessions keep the receiver alive.
            desktop: sway.state(),
            sway,
            server_name: Arc::from(config.server_name.as_str()),
            read_only: Arc::clone(&read_only),
            shutdown: stop.clone(),
        };
        let advertised = Advertised {
            server_name: config.server_name,
            bound: bound.clone(),
        };
        let mut tasks = vec![tokio::spawn(admin.run(security, advertised, stop))];
        for listener in listeners {
            tasks.push(tokio::spawn(listener::run(listener, shared.clone())));
        }
        Ok(DaemonHandle {
            shutdown,
            read_only,
            bound,
            fingerprint,
            tasks,
        })
    }
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
    /// sessions see `read_only`.
    pub fn enter_read_only(&self) {
        self.read_only.store(true, Ordering::Relaxed);
        tracing::info!("read-only mode entered; it lasts until the daemon restarts");
    }

    /// Stops accepting, lets the sessions send `bye`, removes `admin.sock`.
    pub async fn stop(self) {
        tracing::info!("stopping");
        self.shutdown.send_replace(true);
        for task in self.tasks {
            if let Err(error) = task.await {
                tracing::error!(%error, "daemon task failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use swayconnect_protocol::limits::DEFAULT_PORT;
    use swayconnect_testkit::{FakeSway, TestDirs};

    use super::*;

    const CRATES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    // Split, so that this file does not hold the texts it searches for.
    const ENVIRONMENT_READS: [&str; 3] = [
        concat!("std::env", "::var"),
        concat!("env::var", "_os"),
        concat!("env::va", "rs"),
    ];
    const COMMAND_TEXTS: [&str; 2] = [
        concat!("[con", "_id="),
        concat!("no-auto", "-back-and-forth"),
    ];

    fn rust_files_below(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("readable directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                rust_files_below(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// Every file under `crates/*/src/` that holds one of `needles`, as a path below `crates/`.
    fn source_files_with(needles: &[&str]) -> Vec<String> {
        let mut sources = Vec::new();
        for entry in fs::read_dir(CRATES_DIR).expect("crates directory") {
            let source_dir = entry.expect("crate directory").path().join("src");
            if source_dir.is_dir() {
                rust_files_below(&source_dir, &mut sources);
            }
        }
        assert!(sources.len() > 20, "the scan found the workspace sources");
        let crates_dir = Path::new(CRATES_DIR)
            .canonicalize()
            .expect("crates directory");
        let mut hits = Vec::new();
        for source in sources {
            let text = fs::read_to_string(&source).expect("readable source file");
            if needles.iter().any(|needle| text.contains(needle)) {
                let source = source.canonicalize().expect("source path");
                let relative = source.strip_prefix(&crates_dir).expect("below crates/");
                hits.push(relative.display().to_string());
            }
        }
        hits.sort();
        hits
    }

    #[test]
    fn only_the_two_main_files_read_the_environment() {
        let allowed = [
            "swayconnect-cli/src/main.rs",
            "swayconnect-daemon/src/main.rs",
        ];
        let mut readers = source_files_with(&ENVIRONMENT_READS);
        readers.retain(|file| !allowed.contains(&file.as_str()));
        assert_eq!(readers, Vec::<String>::new());
    }

    #[test]
    fn only_the_command_builder_spells_sway_command_text() {
        let mut spellers = source_files_with(&COMMAND_TEXTS);
        spellers.retain(|file| file != "swayconnect-sway/src/command.rs");
        assert_eq!(spellers, Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_daemon_binds_an_ephemeral_port_and_removes_the_admin_socket_on_stop() {
        let dirs = TestDirs::new();
        let _sway = FakeSway::start(&dirs.runtime_dir(), &[]);
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml("listen = [\"127.0.0.1:0\"]").expect("config");
        let target = SwayTarget::Explicit(dirs.sway_socket());

        let daemon = Daemon::start(config, paths.clone(), target)
            .await
            .expect("daemon starts");

        let [address] = daemon.bound() else {
            panic!("one listen address, got {:?}", daemon.bound());
        };
        assert!(address.ip().is_loopback());
        assert!(![0, DEFAULT_PORT].contains(&address.port()));
        assert!(paths.admin_socket().exists());
        assert!(paths.identity_cert().exists());

        daemon.stop().await;
        assert!(!paths.admin_socket().exists());
    }

    #[tokio::test]
    async fn an_address_in_use_is_a_runtime_failure_not_a_config_error() {
        let dirs = TestDirs::new();
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        let address = taken.local_addr().expect("address");
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml(&format!("listen = [\"{address}\"]")).expect("config");
        let target = SwayTarget::Explicit(dirs.sway_socket());

        let Err(error) = Daemon::start(config, paths.clone(), target).await else {
            panic!("the bind must fail");
        };
        assert!(matches!(error, DaemonError::Bind { .. }));
        assert_eq!(error.exit_code(), 1);
        assert!(
            !paths.admin_socket().exists(),
            "the failed start left an admin socket that nobody serves"
        );
    }

    #[tokio::test]
    async fn a_second_daemon_is_a_config_error_even_when_its_address_is_in_use() {
        let dirs = TestDirs::new();
        let _sway = FakeSway::start(&dirs.runtime_dir(), &[]);
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml("listen = [\"127.0.0.1:0\"]").expect("config");
        let target = SwayTarget::Explicit(dirs.sway_socket());
        let first = Daemon::start(config, paths.clone(), target.clone())
            .await
            .expect("the first daemon starts");
        let [address] = first.bound() else {
            panic!("one listen address, got {:?}", first.bound());
        };
        let same_address = Config::from_toml(&format!("listen = [\"{address}\"]")).expect("config");

        let Err(error) = Daemon::start(same_address, paths.clone(), target).await else {
            panic!("the second start must fail");
        };

        assert!(matches!(
            error,
            DaemonError::Start(StartError::AlreadyRunning(_))
        ));
        assert_eq!(error.exit_code(), 2);
        assert!(
            paths.admin_socket().exists(),
            "the refused start removed the socket of the running daemon"
        );
        first.stop().await;
    }
}
