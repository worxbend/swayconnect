//! The path guard of `docs/DESIGN.md` 9.2 rule 3: a fake sway can only live inside the
//! directory the test harness names, and never at a path the harness forbids.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// Returns the symlink-free form of `socket`, or panics when the fake sway must not bind it.
///
/// Refused: a path outside `dir` (after symlinks are resolved), a path in `forbidden`,
/// and a path where something already exists. The last rule means the fake never
/// unlinks or replaces a file it did not create.
pub(crate) fn checked_socket_path(socket: &Path, dir: &Path, forbidden: &[PathBuf]) -> PathBuf {
    let real_dir = dir.canonicalize().unwrap_or_else(|error| {
        panic!(
            "fake sway directory {} is not usable: {error}",
            dir.display()
        )
    });
    let real_socket = resolve_parent(socket).unwrap_or_else(|error| {
        panic!(
            "fake sway socket path {} is not usable: {error}",
            socket.display()
        )
    });

    assert!(
        real_socket.starts_with(&real_dir),
        "fake sway socket path {} is outside {}",
        real_socket.display(),
        real_dir.display()
    );
    for forbidden_path in forbidden {
        // A forbidden path that cannot be resolved is still compared as written.
        let real_forbidden = resolve_parent(forbidden_path).ok();
        assert!(
            socket != forbidden_path && real_forbidden.as_ref() != Some(&real_socket),
            "fake sway socket path {} is forbidden",
            real_socket.display()
        );
    }
    match real_socket.symlink_metadata() {
        Err(error) if error.kind() == ErrorKind::NotFound => real_socket,
        Err(error) => panic!(
            "fake sway socket path {} is not usable: {error}",
            real_socket.display()
        ),
        Ok(_) => panic!(
            "fake sway socket path {} already exists",
            real_socket.display()
        ),
    }
}

/// Resolves every symlink in the parent directory and keeps the file name as written.
/// The file itself need not exist.
fn resolve_parent(path: &Path) -> io::Result<PathBuf> {
    let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "no parent directory or file name",
        ));
    };
    Ok(parent.canonicalize()?.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .tempdir_in("/tmp")
            .expect("temp dir")
    }

    #[test]
    fn a_path_inside_the_directory_is_accepted() {
        let dir = scratch_dir();
        let socket = dir.path().join("sway.sock");

        let checked = checked_socket_path(&socket, dir.path(), &[]);

        assert_eq!(
            checked,
            dir.path()
                .canonicalize()
                .expect("canonical")
                .join("sway.sock")
        );
    }

    #[test]
    #[should_panic(expected = "is outside")]
    fn a_dot_dot_path_that_leaves_the_directory_is_refused() {
        let dir = scratch_dir();
        let inner = dir.path().join("inner");
        std::fs::create_dir(&inner).expect("inner dir");

        checked_socket_path(&inner.join("../sway.sock"), &inner, &[]);
    }

    #[test]
    #[should_panic(expected = "is forbidden")]
    fn a_forbidden_path_reached_through_a_symlink_is_refused() {
        let dir = scratch_dir();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("real dir");
        std::os::unix::fs::symlink(&real, dir.path().join("link")).expect("symlink");

        checked_socket_path(
            &dir.path().join("link/sway.sock"),
            dir.path(),
            &[real.join("sway.sock")],
        );
    }

    #[test]
    #[should_panic(expected = "not usable")]
    fn a_relative_file_name_is_refused() {
        let dir = scratch_dir();

        checked_socket_path(Path::new("sway.sock"), dir.path(), &[]);
    }
}
