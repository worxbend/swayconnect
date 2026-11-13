//! Where the sway IPC socket is (`docs/DESIGN.md` 4.3).

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How the daemon finds sway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwayTarget {
    /// This path and nothing else (config key `sway.socket`).
    Explicit(PathBuf),
    /// `swaysock` if connectable, else the newest live `sway-ipc.<uid>.*.sock`.
    Discover(EnvSnapshot),
}

/// The environment values discovery needs. Only a `main.rs` builds it from the real
/// environment; nothing in this crate reads the process environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSnapshot {
    pub swaysock: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub uid: u32,
}

/// Resolves the socket path. Called again on every connect attempt.
/// Pure except for file system reads.
///
/// `None` means "no sway found now". A runtime directory that cannot be read counts as
/// empty: the caller retries anyway and has no use for the reason.
pub fn resolve(target: &SwayTarget) -> Option<PathBuf> {
    match target {
        SwayTarget::Explicit(path) => Some(path.clone()),
        SwayTarget::Discover(env) => connectable_swaysock(env).or_else(|| newest_live_socket(env)),
    }
}

fn connectable_swaysock(env: &EnvSnapshot) -> Option<PathBuf> {
    let path = env.swaysock.as_ref()?;
    UnixStream::connect(path).is_ok().then(|| path.clone())
}

/// A directory entry named `sway-ipc.<uid>.<pid>.sock`.
#[derive(Debug)]
struct SocketFile {
    path: PathBuf,
    owner: u32,
    pid: u32,
    modified: SystemTime,
}

fn newest_live_socket(env: &EnvSnapshot) -> Option<PathBuf> {
    fs::read_dir(env.xdg_runtime_dir.as_ref()?)
        .ok()?
        .filter_map(|entry| socket_file(&entry.ok()?, env.uid))
        .filter(|file| is_live_and_owned(file, env.uid))
        .max_by_key(|file| (file.modified, file.pid))
        .map(|file| file.path)
}

fn socket_file(entry: &fs::DirEntry, uid: u32) -> Option<SocketFile> {
    let pid = pid_in_name(entry.file_name().to_str()?, uid)?;
    // Not followed through a symlink: the owner checked is the owner of the entry itself.
    let metadata = entry.metadata().ok()?;
    Some(SocketFile {
        path: entry.path(),
        owner: metadata.uid(),
        pid,
        modified: metadata.modified().ok()?,
    })
}

fn pid_in_name(file_name: &str, uid: u32) -> Option<u32> {
    let pid = file_name
        .strip_prefix(&format!("sway-ipc.{uid}."))?
        .strip_suffix(".sock")?;
    // `parse` alone would also take a leading `+`.
    pid.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| pid.parse().ok())?
}

fn is_live_and_owned(file: &SocketFile, uid: u32) -> bool {
    file.owner == uid && Path::new("/proc").join(file.pid.to_string()).exists()
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;

    /// Above the kernel's largest pid (2^22), so `/proc/<pid>` never exists.
    const DEAD_PID: u32 = u32::MAX;
    /// Pid 1 is alive on every Linux system.
    const INIT_PID: u32 = 1;

    struct RuntimeDir {
        dir: TempDir,
        uid: u32,
    }

    impl RuntimeDir {
        fn new() -> RuntimeDir {
            let dir = tempfile::tempdir().expect("temp directory");
            let uid = fs::metadata(dir.path())
                .expect("temp directory metadata")
                .uid();
            RuntimeDir { dir, uid }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        /// Creates `sway-ipc.<uid>.<pid>.sock`, modified `age_secs` seconds ago.
        fn add_socket_file(&self, uid: u32, pid: u32, age_secs: u64) -> PathBuf {
            let path = self.path(&format!("sway-ipc.{uid}.{pid}.sock"));
            let file = File::create(&path).expect("create the socket file");
            file.set_modified(SystemTime::now() - Duration::from_secs(age_secs))
                .expect("set the modification time");
            path
        }

        fn discover(&self, swaysock: Option<PathBuf>) -> SwayTarget {
            SwayTarget::Discover(EnvSnapshot {
                swaysock,
                xdg_runtime_dir: Some(self.dir.path().to_owned()),
                uid: self.uid,
            })
        }
    }

    #[test]
    fn explicit_wins_and_never_looks_elsewhere() {
        let runtime = RuntimeDir::new();
        runtime.add_socket_file(runtime.uid, std::process::id(), 0);
        let explicit = runtime.path("not-there.sock");

        assert_eq!(
            resolve(&SwayTarget::Explicit(explicit.clone())),
            Some(explicit)
        );
    }

    #[test]
    fn a_connectable_swaysock_wins_over_the_glob() {
        let runtime = RuntimeDir::new();
        runtime.add_socket_file(runtime.uid, std::process::id(), 0);
        let swaysock = runtime.path("custom.sock");
        let _listener = UnixListener::bind(&swaysock).expect("bind in the temp directory");

        assert_eq!(
            resolve(&runtime.discover(Some(swaysock.clone()))),
            Some(swaysock)
        );
    }

    #[test]
    fn a_stale_swaysock_falls_through_to_the_glob() {
        let runtime = RuntimeDir::new();
        let live = runtime.add_socket_file(runtime.uid, std::process::id(), 0);
        let stale = runtime.path("stale.sock");
        // The file stays after the listener is gone, as after a sway crash.
        drop(UnixListener::bind(&stale).expect("bind in the temp directory"));
        assert!(stale.exists());

        assert_eq!(resolve(&runtime.discover(Some(stale))), Some(live.clone()));
        assert_eq!(
            resolve(&runtime.discover(Some(runtime.path("missing.sock")))),
            Some(live)
        );
    }

    #[test]
    fn a_file_with_a_dead_pid_is_skipped() {
        let runtime = RuntimeDir::new();
        runtime.add_socket_file(runtime.uid, DEAD_PID, 0);
        assert_eq!(resolve(&runtime.discover(None)), None);

        let live = runtime.add_socket_file(runtime.uid, std::process::id(), 60);
        assert_eq!(resolve(&runtime.discover(None)), Some(live));
    }

    #[test]
    fn the_newest_live_file_is_taken() {
        let runtime = RuntimeDir::new();
        runtime.add_socket_file(runtime.uid, std::process::id(), 60);
        let newest = runtime.add_socket_file(runtime.uid, INIT_PID, 10);

        assert_eq!(resolve(&runtime.discover(None)), Some(newest));
    }

    #[test]
    fn files_with_other_names_are_ignored() {
        let runtime = RuntimeDir::new();
        let pid = std::process::id();
        runtime.add_socket_file(runtime.uid + 1, pid, 0);
        for name in [
            format!("sway-ipc.{}.{pid}.sock.bak", runtime.uid),
            format!("sway-ipc.{}.+{pid}.sock", runtime.uid),
            format!("sway-ipc.{}..sock", runtime.uid),
            format!("i3-ipc.{}.{pid}.sock", runtime.uid),
        ] {
            File::create(runtime.path(&name)).expect("create a file");
        }

        assert_eq!(resolve(&runtime.discover(None)), None);
    }

    #[test]
    fn no_runtime_directory_gives_nothing() {
        let runtime = RuntimeDir::new();
        let missing_dir = SwayTarget::Discover(EnvSnapshot {
            swaysock: None,
            xdg_runtime_dir: Some(runtime.path("missing")),
            uid: runtime.uid,
        });
        let unset_dir = SwayTarget::Discover(EnvSnapshot {
            swaysock: None,
            xdg_runtime_dir: None,
            uid: runtime.uid,
        });

        assert_eq!(resolve(&missing_dir), None);
        assert_eq!(resolve(&unset_dir), None);
    }

    fn socket_file_of(owner: u32, pid: u32) -> SocketFile {
        SocketFile {
            path: PathBuf::from("sway-ipc.sock"),
            owner,
            pid,
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn the_filter_keeps_only_live_files_of_the_own_uid() {
        let uid = 1000;
        assert!(is_live_and_owned(&socket_file_of(uid, INIT_PID), uid));
        assert!(
            !is_live_and_owned(&socket_file_of(uid + 1, INIT_PID), uid),
            "another owner"
        );
        assert!(
            !is_live_and_owned(&socket_file_of(0, INIT_PID), uid),
            "owned by root"
        );
        assert!(
            !is_live_and_owned(&socket_file_of(uid, DEAD_PID), uid),
            "dead pid"
        );
    }
}
