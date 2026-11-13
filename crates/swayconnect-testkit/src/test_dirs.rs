//! One temp root per test with everything a daemon or a child process needs.

use std::ffi::OsStr;
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::SWAY_SOCKET_NAME;

/// A temp root with config, data and runtime directories. Removed on drop.
///
/// The testkit does not depend on the daemon, so a test builds the daemon's `Paths`
/// from [`config_file`](Self::config_file), [`data_dir`](Self::data_dir) and
/// [`runtime_dir`](Self::runtime_dir), and `SwayTarget::Explicit` from
/// [`sway_socket`](Self::sway_socket).
#[derive(Debug)]
pub struct TestDirs {
    root: TempDir,
}

impl TestDirs {
    /// Creates the directories. Panics when the file system refuses.
    #[expect(
        clippy::new_without_default,
        reason = "creating directories is not a default value"
    )]
    pub fn new() -> Self {
        // Fixed parent: the process environment is not read, and the socket path stays
        // well under the 108 bytes a Unix socket address can hold.
        let root = tempfile::Builder::new()
            .prefix("swayconnect-")
            .tempdir_in("/tmp")
            .expect("create temp root under /tmp");
        let dirs = Self { root };
        fs::create_dir_all(dirs.config_home().join("swayconnect")).expect("create config dir");
        fs::create_dir(dirs.data_home()).expect("create data home");
        fs::create_dir(dirs.runtime_dir()).expect("create runtime dir");
        // The daemon refuses a runtime directory with a looser mode (docs/DESIGN.md 5.4).
        fs::set_permissions(dirs.runtime_dir(), Permissions::from_mode(0o700))
            .expect("set runtime dir mode");
        dirs
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Value for `XDG_CONFIG_HOME`.
    pub fn config_home(&self) -> PathBuf {
        self.root().join("config")
    }

    /// Value for `XDG_DATA_HOME`.
    pub fn data_home(&self) -> PathBuf {
        self.root().join("data")
    }

    /// Value for `XDG_RUNTIME_DIR`, mode 0700. Also the directory of the fake sway socket.
    pub fn runtime_dir(&self) -> PathBuf {
        self.root().join("runtime")
    }

    /// Where the daemon reads its config. The file is not created; its directory is.
    pub fn config_file(&self) -> PathBuf {
        self.config_home().join("swayconnect").join("config.toml")
    }

    /// The daemon's data directory. Not created: the daemon creates it with its own mode.
    pub fn data_dir(&self) -> PathBuf {
        self.data_home().join("swayconnect")
    }

    /// Where `FakeSway::start(&dirs.runtime_dir(), ..)` listens.
    pub fn sway_socket(&self) -> PathBuf {
        self.runtime_dir().join(SWAY_SOCKET_NAME)
    }

    /// A command for a child process with a cleared environment and only the three XDG
    /// directories set (`docs/DESIGN.md` 9.2 rule 4).
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("XDG_CONFIG_HOME", self.config_home())
            .env("XDG_DATA_HOME", self.data_home())
            .env("XDG_RUNTIME_DIR", self.runtime_dir());
        command
    }
}
