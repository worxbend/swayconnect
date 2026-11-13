//! The profile file: where it lives and who may read it (`docs/DESIGN.md` 7.2).

use std::ffi::OsString;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use swayconnect_protocol::JsonObject;

use crate::client::{Profile, ProfileError};

const PROFILE_FILE: &str = "default.json";
const TEMPORARY_FILE: &str = "default.json.tmp";
const DIRECTORY_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
const GROUP_AND_OTHERS: u32 = 0o077;
const PERMISSION_BITS: u32 = 0o7777;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SWAYCONNECT_CLIENT_DIR must be an absolute path")]
    RelativeClientDirectory,
    #[error("neither XDG_DATA_HOME nor HOME is an absolute path")]
    NoHome,
    #[error("no profile at {}: run `swayconnect pair` first", .0.display())]
    NoProfile(PathBuf),
    #[error("{} is not a regular file", .0.display())]
    NotAFile(PathBuf),
    #[error(
        "{} has mode {mode:o}: the device key must be readable by its owner only (chmod 600)",
        path.display()
    )]
    LooseMode { path: PathBuf, mode: u32 },
    #[error("{} is not a JSON object: {source}", path.display())]
    NotJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{}: {source}", path.display())]
    Invalid { path: PathBuf, source: ProfileError },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The values `main.rs` reads from the process environment.
#[derive(Debug, Default)]
pub struct DirectoryInputs {
    pub client_dir: Option<OsString>,
    pub xdg_data_home: Option<OsString>,
    pub home: Option<OsString>,
}

/// The override when set; else `$XDG_DATA_HOME/swayconnect/client`. A relative
/// `XDG_DATA_HOME` is ignored, as the XDG base directory specification asks.
pub fn client_directory(inputs: &DirectoryInputs) -> Result<PathBuf, StoreError> {
    if let Some(client_dir) = &inputs.client_dir {
        let client_dir = PathBuf::from(client_dir);
        if !client_dir.is_absolute() {
            return Err(StoreError::RelativeClientDirectory);
        }
        return Ok(client_dir);
    }
    let data_home = match absolute(&inputs.xdg_data_home) {
        Some(data_home) => data_home,
        None => absolute(&inputs.home)
            .ok_or(StoreError::NoHome)?
            .join(".local/share"),
    };
    Ok(data_home.join("swayconnect/client"))
}

fn absolute(value: &Option<OsString>) -> Option<PathBuf> {
    value
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Holds the one profile of a client directory.
#[derive(Debug)]
pub struct ProfileStore {
    directory: PathBuf,
}

impl ProfileStore {
    pub fn new(directory: PathBuf) -> ProfileStore {
        ProfileStore { directory }
    }

    pub fn load(&self) -> Result<Profile, StoreError> {
        let path = self.directory.join(PROFILE_FILE);
        let mut file = File::open(&path).map_err(|source| match source.kind() {
            ErrorKind::NotFound => StoreError::NoProfile(path.clone()),
            _other_kind => io_error(&path, source),
        })?;
        // The checks use the open handle, so the file that was checked is the file read.
        let metadata = file.metadata().map_err(|source| io_error(&path, source))?;
        if !metadata.is_file() {
            return Err(StoreError::NotAFile(path));
        }
        let mode = metadata.mode() & PERMISSION_BITS;
        if mode & GROUP_AND_OTHERS != 0 {
            return Err(StoreError::LooseMode { path, mode });
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|source| io_error(&path, source))?;
        let object: JsonObject = match serde_json::from_str(&text) {
            Ok(object) => object,
            Err(source) => return Err(StoreError::NotJson { path, source }),
        };
        Profile::from_json(&object).map_err(|source| StoreError::Invalid { path, source })
    }

    /// Replaces an older profile. The new file is complete on disk before it gets the name.
    pub fn save(&self, profile: &Profile) -> Result<(), StoreError> {
        DirBuilder::new()
            .recursive(true)
            .mode(DIRECTORY_MODE)
            .create(&self.directory)
            .map_err(|source| io_error(&self.directory, source))?;
        let temporary = self.directory.join(TEMPORARY_FILE);
        write_private_file(&temporary, &profile_text(profile))
            .map_err(|source| io_error(&temporary, source))?;
        let path = self.directory.join(PROFILE_FILE);
        std::fs::rename(&temporary, &path).map_err(|source| io_error(&path, source))
    }
}

fn profile_text(profile: &Profile) -> String {
    let mut text = serde_json::Value::Object(profile.to_json()).to_string();
    text.push('\n');
    text
}

fn write_private_file(path: &Path, text: &str) -> std::io::Result<()> {
    // `mode` only applies to a file that `open` creates, so a leftover must go first.
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

fn io_error(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt as _;

    use swayconnect_protocol::DeviceId;

    use super::*;

    fn profile(server_name: &str) -> Profile {
        Profile {
            host: "desk.example".to_owned(),
            port: 4000,
            fp: [1; 32],
            device_id: DeviceId::from_bytes([2; 16]),
            key: [3; 32],
            server_name: server_name.to_owned(),
        }
    }

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path).expect("metadata").mode() & PERMISSION_BITS
    }

    fn inputs(
        client_dir: Option<&str>,
        xdg_data_home: Option<&str>,
        home: Option<&str>,
    ) -> DirectoryInputs {
        DirectoryInputs {
            client_dir: client_dir.map(OsString::from),
            xdg_data_home: xdg_data_home.map(OsString::from),
            home: home.map(OsString::from),
        }
    }

    #[test]
    fn a_saved_profile_is_private_and_loads_again() {
        let root = tempfile::tempdir().expect("temp dir");
        let directory = root.path().join("data/client");
        let store = ProfileStore::new(directory.clone());

        store.save(&profile("desk")).expect("save");

        assert_eq!(mode_of(&directory.join(PROFILE_FILE)), 0o600);
        assert_eq!(mode_of(&directory), 0o700);
        assert_eq!(store.load().expect("load"), profile("desk"));
    }

    #[test]
    fn saving_replaces_an_older_profile_and_a_leftover_temporary_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = ProfileStore::new(root.path().to_owned());
        store.save(&profile("old")).expect("first save");
        let leftover = root.path().join(TEMPORARY_FILE);
        std::fs::write(&leftover, "junk").expect("leftover");
        std::fs::set_permissions(&leftover, Permissions::from_mode(0o644)).expect("chmod");

        store.save(&profile("new")).expect("second save");

        assert_eq!(store.load().expect("load"), profile("new"));
        assert_eq!(mode_of(&root.path().join(PROFILE_FILE)), 0o600);
        assert!(!leftover.exists());
    }

    #[test]
    fn a_profile_readable_by_group_or_others_is_refused() {
        for loose_mode in [0o640, 0o604, 0o660, 0o644, 0o610, 0o601] {
            let root = tempfile::tempdir().expect("temp dir");
            let store = ProfileStore::new(root.path().to_owned());
            store.save(&profile("desk")).expect("save");
            let path = root.path().join(PROFILE_FILE);
            std::fs::set_permissions(&path, Permissions::from_mode(loose_mode)).expect("chmod");

            let refused = store.load().expect_err("loose mode");

            assert!(
                matches!(refused, StoreError::LooseMode { mode, .. } if mode == loose_mode),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_stricter_mode_is_accepted() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = ProfileStore::new(root.path().to_owned());
        store.save(&profile("desk")).expect("save");
        let path = root.path().join(PROFILE_FILE);
        std::fs::set_permissions(&path, Permissions::from_mode(0o400)).expect("chmod");

        assert_eq!(store.load().expect("load"), profile("desk"));
    }

    #[test]
    fn a_missing_profile_names_the_pair_command() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = ProfileStore::new(root.path().join("nothing-here"));

        let missing = store.load().expect_err("no file");

        assert!(matches!(missing, StoreError::NoProfile(_)), "{missing}");
        assert!(missing.to_string().contains("swayconnect pair"));
    }

    #[test]
    fn a_damaged_profile_is_refused() {
        let cases = [
            ("not json", "is not a JSON object"),
            ("[1]", "is not a JSON object"),
            ("{\"host\":\"desk\"}", "`port`"),
        ];
        for (content, expected) in cases {
            let root = tempfile::tempdir().expect("temp dir");
            let path = root.path().join(PROFILE_FILE);
            std::fs::write(&path, content).expect("write");
            std::fs::set_permissions(&path, Permissions::from_mode(0o600)).expect("chmod");

            let refused = ProfileStore::new(root.path().to_owned())
                .load()
                .expect_err(content);

            assert!(refused.to_string().contains(expected), "{refused}");
        }
    }

    #[test]
    fn a_directory_in_place_of_the_profile_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(root.path().join(PROFILE_FILE)).expect("mkdir");

        let refused = ProfileStore::new(root.path().to_owned())
            .load()
            .expect_err("directory");

        assert!(matches!(refused, StoreError::NotAFile(_)), "{refused}");
    }

    #[test]
    fn the_override_directory_wins_and_must_be_absolute() {
        let overridden = inputs(Some("/tmp/devices/a"), Some("/data"), Some("/home/u"));
        assert_eq!(
            client_directory(&overridden).expect("absolute override"),
            PathBuf::from("/tmp/devices/a")
        );

        for relative in ["devices/a", "", "./a"] {
            let refused = client_directory(&inputs(Some(relative), Some("/data"), Some("/home/u")));
            assert!(matches!(refused, Err(StoreError::RelativeClientDirectory)));
        }
    }

    #[test]
    fn the_default_directory_follows_xdg_data_home_then_home() {
        let cases = [
            (Some("/data"), Some("/home/u"), "/data/swayconnect/client"),
            (
                None,
                Some("/home/u"),
                "/home/u/.local/share/swayconnect/client",
            ),
            (
                Some("relative"),
                Some("/home/u"),
                "/home/u/.local/share/swayconnect/client",
            ),
        ];
        for (xdg_data_home, home, expected) in cases {
            let directory = client_directory(&inputs(None, xdg_data_home, home)).expect(expected);
            assert_eq!(directory, PathBuf::from(expected));
        }

        let nowhere = client_directory(&inputs(None, None, None));
        assert!(matches!(nowhere, Err(StoreError::NoHome)));
    }
}
