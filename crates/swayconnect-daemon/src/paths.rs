//! `Paths` value, file mode and owner checks, atomic write, own uid (`docs/DESIGN.md` 5.5).
//! Written by slice S6a.

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const GROUP_AND_OTHER_BITS: u32 = 0o077;

/// Where the daemon keeps its files. `main.rs` builds it from the XDG variables;
/// tests build it from temp directories.
///
/// An empty `runtime_dir` means `XDG_RUNTIME_DIR` is not set; the admin socket refuses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl Paths {
    pub fn identity_cert(&self) -> PathBuf {
        self.data_dir.join("identity.cert.der")
    }

    pub fn identity_key(&self) -> PathBuf {
        self.data_dir.join("identity.key.der")
    }

    pub fn devices_file(&self) -> PathBuf {
        self.data_dir.join("devices.json")
    }

    pub fn admin_dir(&self) -> PathBuf {
        self.runtime_dir.join("swayconnect")
    }

    pub fn admin_socket(&self) -> PathBuf {
        self.admin_dir().join("admin.sock")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    #[error("is not set or is not an absolute path")]
    NotAbsolute,
    #[error("is a symlink")]
    Symlink,
    #[error("is not a directory or regular file as expected")]
    WrongKind,
    #[error("is owned by another user")]
    WrongOwner,
    #[error("is accessible by group or others")]
    LooseMode,
}

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("{}: {problem}", path.display())]
    Refused { path: PathBuf, problem: Problem },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl PathError {
    pub(crate) fn refused(path: &Path, problem: Problem) -> PathError {
        PathError::Refused {
            path: path.to_owned(),
            problem,
        }
    }

    pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> PathError {
        |source| PathError::Io {
            path: path.to_owned(),
            source,
        }
    }
}

/// The uid this process runs as. Needs a tokio runtime: `std` has no stable peer credentials
/// and the workspace forbids `unsafe` and has no libc binding (`docs/DESIGN.md` 5.5).
pub fn own_uid() -> io::Result<u32> {
    let (one_end, _other_end) = tokio::net::UnixStream::pair()?;
    Ok(one_end.peer_cred()?.uid())
}

pub fn require_absolute(path: &Path) -> Result<(), PathError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(PathError::refused(path, Problem::NotAbsolute))
    }
}

/// Refuses a symlink, another kind of file, another owner and any group or other mode bit.
pub fn check_private(path: &Path, kind: Kind, uid: u32) -> Result<(), PathError> {
    let metadata = fs::symlink_metadata(path).map_err(PathError::io(path))?;
    let file_type = metadata.file_type();
    let is_expected_kind = match kind {
        Kind::Directory => file_type.is_dir(),
        Kind::File => file_type.is_file(),
    };
    let problem = if file_type.is_symlink() {
        Problem::Symlink
    } else if !is_expected_kind {
        Problem::WrongKind
    } else if metadata.uid() != uid {
        Problem::WrongOwner
    } else if metadata.mode() & GROUP_AND_OTHER_BITS != 0 {
        Problem::LooseMode
    } else {
        return Ok(());
    };
    Err(PathError::refused(path, problem))
}

/// Creates the directory with mode 0700 when it is missing. An existing one is checked, never
/// repaired.
pub fn ensure_private_dir(path: &Path, uid: u32) -> Result<(), PathError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => DirBuilder::new()
            .recursive(true)
            .mode(PRIVATE_DIR_MODE)
            .create(path)
            .map_err(PathError::io(path))?,
        Err(error) => return Err(PathError::io(path)(error)),
    }
    check_private(path, Kind::Directory, uid)
}

/// The content of a private file, or `None` when it does not exist.
pub fn read_private(path: &Path, uid: u32) -> Result<Option<Vec<u8>>, PathError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PathError::io(path)(error)),
    }
    check_private(path, Kind::File, uid)?;
    fs::read(path).map(Some).map_err(PathError::io(path))
}

/// Temp file in the same directory (mode 0600), `fsync`, rename, `fsync` of the directory.
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<(), PathError> {
    let temp = temp_path(path);
    let write = || -> io::Result<()> {
        remove_if_present(&temp)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(PRIVATE_FILE_MODE)
            .open(&temp)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        match path.parent() {
            Some(directory) => File::open(directory)?.sync_all(),
            None => Ok(()),
        }
    };
    write().map_err(|error| {
        // The write error is the one to report; a leftover temp file is replaced next time.
        let _ = fs::remove_file(&temp);
        PathError::io(path)(error)
    })
}

/// The daemon is the only writer of its directory, so one fixed temp name per file is enough.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(".tmp");
    path.with_file_name(name)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn problem_of(outcome: Result<(), PathError>) -> Option<Problem> {
        match outcome {
            Ok(()) => None,
            Err(PathError::Refused { problem, .. }) => Some(problem),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    fn file_owner(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").uid()
    }

    #[tokio::test]
    async fn own_uid_equals_the_owner_of_a_new_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let file = root.path().join("probe");
        fs::write(&file, b"x").expect("write probe");
        assert_eq!(own_uid().expect("own uid"), file_owner(&file));
    }

    #[test]
    fn a_directory_with_a_looser_mode_a_symlink_or_another_owner_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let uid = file_owner(root.path());
        let directory = root.path().join("data");
        ensure_private_dir(&directory, uid).expect("create");
        let mode = fs::metadata(&directory).expect("metadata").mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert_eq!(problem_of(ensure_private_dir(&directory, uid)), None);
        assert_eq!(
            problem_of(ensure_private_dir(&directory, uid + 1)),
            Some(Problem::WrongOwner)
        );

        fs::set_permissions(&directory, Permissions::from_mode(0o750)).expect("chmod");
        assert_eq!(
            problem_of(ensure_private_dir(&directory, uid)),
            Some(Problem::LooseMode)
        );

        let link = root.path().join("link");
        fs::set_permissions(&directory, Permissions::from_mode(0o700)).expect("chmod");
        std::os::unix::fs::symlink(&directory, &link).expect("symlink");
        assert_eq!(
            problem_of(ensure_private_dir(&link, uid)),
            Some(Problem::Symlink)
        );
    }

    #[test]
    fn a_file_with_a_looser_mode_a_symlink_or_another_owner_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let uid = file_owner(root.path());
        let file = root.path().join("devices.json");
        write_atomic(&file, b"{}").expect("write");
        assert_eq!(
            read_private(&file, uid).expect("read"),
            Some(b"{}".to_vec())
        );
        assert_eq!(
            problem_of(check_private(&file, Kind::File, uid + 1)),
            Some(Problem::WrongOwner)
        );
        assert_eq!(
            problem_of(check_private(&file, Kind::Directory, uid)),
            Some(Problem::WrongKind)
        );

        let link = root.path().join("link");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");
        assert!(matches!(
            read_private(&link, uid),
            Err(PathError::Refused {
                problem: Problem::Symlink,
                ..
            })
        ));

        fs::set_permissions(&file, Permissions::from_mode(0o640)).expect("chmod");
        assert!(matches!(
            read_private(&file, uid),
            Err(PathError::Refused {
                problem: Problem::LooseMode,
                ..
            })
        ));
        assert_eq!(
            read_private(&root.path().join("missing"), uid).expect("read"),
            None
        );
    }

    #[test]
    fn write_atomic_replaces_the_file_through_a_temp_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let file = root.path().join("devices.json");
        let temp = temp_path(&file);
        assert_eq!(temp.parent(), file.parent());
        // A stale temp file of a crashed write must not block the next one.
        fs::write(&temp, b"stale").expect("write stale temp");

        write_atomic(&file, b"first").expect("first write");
        write_atomic(&file, b"second").expect("second write");

        assert_eq!(fs::read(&file).expect("read"), b"second");
        assert_eq!(fs::metadata(&file).expect("metadata").mode() & 0o777, 0o600);
        assert!(!temp.exists());
        let missing_dir = root.path().join("missing").join("file");
        assert!(matches!(
            write_atomic(&missing_dir, b"x"),
            Err(PathError::Io { .. })
        ));
    }

    #[test]
    fn a_relative_or_empty_path_is_not_absolute() {
        assert_eq!(
            problem_of(require_absolute(Path::new(""))),
            Some(Problem::NotAbsolute)
        );
        assert_eq!(
            problem_of(require_absolute(Path::new("run/user"))),
            Some(Problem::NotAbsolute)
        );
        assert_eq!(problem_of(require_absolute(Path::new("/run/user"))), None);
    }
}
