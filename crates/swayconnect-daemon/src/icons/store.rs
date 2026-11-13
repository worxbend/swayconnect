//! Produced PNGs by hash, resolution results by key, and the disk cache of rasterised
//! icons in `$XDG_CACHE_HOME/swayconnect/icons/` (`docs/DESIGN.md` 11.4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use swayconnect_protocol::{Icon, IconGetResult, IconHash};

use super::{IconKey, png};
use crate::paths::{self, PathError};

/// How long a failed key stays failed before it is tried again.
pub const NEGATIVE_TTL: Duration = Duration::from_secs(30);
/// Keys remembered at most. Windows choose their key fields, so a full map is cleared:
/// the next refresh queues the live windows' keys again (their PNGs stay by hash).
pub const MAX_KEYS: usize = 1024;
/// Part of every disk cache file name. Bump it when `raster::render` output changes.
const RENDER_VERSION: &str = "r1";

/// What the store knows about a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Ready(IconHash),
    /// Failed less than [`NEGATIVE_TTL`] ago.
    Missing,
    /// Never tried, or the failure expired.
    Unknown,
}

#[derive(Debug, Clone, Copy)]
enum Resolution {
    Produced(IconHash),
    Missing { until: Instant },
}

#[derive(Debug, Default)]
pub struct Store {
    by_key: HashMap<IconKey, Resolution>,
    pngs: HashMap<IconHash, Arc<[u8]>>,
}

impl Store {
    pub fn lookup(&self, key: &IconKey, now: Instant) -> Lookup {
        match self.by_key.get(key) {
            Some(Resolution::Produced(hash)) => Lookup::Ready(*hash),
            Some(Resolution::Missing { until }) if now < *until => Lookup::Missing,
            _ => Lookup::Unknown,
        }
    }

    /// Keeps a servable PNG (checked by the caller) and answers its hash.
    pub fn record_produced(&mut self, key: IconKey, png: Vec<u8>) -> IconHash {
        let hash = IconHash::of(&png);
        self.pngs.entry(hash).or_insert_with(|| png.into());
        self.make_room(&key);
        self.by_key.insert(key, Resolution::Produced(hash));
        hash
    }

    /// Marks `key` failed for [`NEGATIVE_TTL`] and drops expired failures, so keys that
    /// windows invent do not pile up.
    pub fn record_missing(&mut self, key: IconKey, now: Instant) {
        self.by_key.retain(|_, resolution| match resolution {
            Resolution::Produced(_) => true,
            Resolution::Missing { until } => now < *until,
        });
        let until = now + NEGATIVE_TTL;
        self.make_room(&key);
        self.by_key.insert(key, Resolution::Missing { until });
    }

    /// Clears the key map when a new key would pass [`MAX_KEYS`].
    fn make_room(&mut self, key: &IconKey) {
        if self.by_key.len() >= MAX_KEYS && !self.by_key.contains_key(key) {
            self.by_key.clear();
        }
    }

    /// The `icon.get` result: the PNGs this daemon produced, in request order.
    pub fn icons(&self, hashes: &[IconHash]) -> IconGetResult {
        let mut result = IconGetResult {
            icons: Vec::new(),
            missing: Vec::new(),
        };
        for hash in hashes {
            match self.pngs.get(hash) {
                Some(png) => result.icons.push(Icon {
                    hash: *hash,
                    png: png.to_vec(),
                }),
                None => result.missing.push(*hash),
            }
        }
        result
    }
}

/// Rasterised PNGs by the hash of their source bytes, so a restart does not run the child
/// again. Files are private (0600 in a 0700 directory) and checked again when read.
#[derive(Debug)]
pub struct DiskCache {
    dir: PathBuf,
    uid: u32,
}

impl DiskCache {
    /// Creates `<cache_dir>/icons` when missing. `cache_dir` is the daemon's own cache
    /// directory (`$XDG_CACHE_HOME/swayconnect`).
    pub fn open(cache_dir: &Path, uid: u32) -> Result<DiskCache, PathError> {
        paths::ensure_private_dir(cache_dir, uid)?;
        let dir = cache_dir.join("icons");
        paths::ensure_private_dir(&dir, uid)?;
        Ok(DiskCache { dir, uid })
    }

    /// The cached PNG for `source`. A file that is not private, unreadable or not servable
    /// counts as absent.
    pub fn get(&self, source: &[u8]) -> Option<Vec<u8>> {
        match paths::read_private(&self.file_for(source), self.uid) {
            Ok(Some(png)) if png::is_servable(&png) => Some(png),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(%error, "icon cache entry ignored");
                None
            }
        }
    }

    pub fn put(&self, source: &[u8], png: &[u8]) -> Result<(), PathError> {
        paths::write_atomic(&self.file_for(source), png)
    }

    fn file_for(&self, source: &[u8]) -> PathBuf {
        // Base64url: no `/`, so the name stays in the directory.
        let digest = IconHash::of(source);
        self.dir.join(format!("{RENDER_VERSION}-{digest}.png"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(app_id: &str) -> IconKey {
        IconKey {
            app_id: Some(app_id.to_owned()),
            ..IconKey::default()
        }
    }

    #[test]
    fn a_failure_is_remembered_for_30_seconds() {
        let mut store = Store::default();
        let start = Instant::now();
        assert_eq!(store.lookup(&key("a"), start), Lookup::Unknown);
        store.record_missing(key("a"), start);
        let almost = start + NEGATIVE_TTL - Duration::from_millis(1);
        assert_eq!(store.lookup(&key("a"), almost), Lookup::Missing);
        assert_eq!(
            store.lookup(&key("a"), start + NEGATIVE_TTL),
            Lookup::Unknown
        );

        store.record_missing(key("b"), start + NEGATIVE_TTL);
        assert!(
            !store.by_key.contains_key(&key("a")),
            "expired failure kept"
        );
    }

    #[test]
    fn keys_that_windows_invent_stay_bounded() {
        let mut store = Store::default();
        let png = png::header(16, 16);
        for index in 0..10_000 {
            store.record_produced(key(&format!("App{index}")), png.clone());
        }
        assert!(store.by_key.len() <= MAX_KEYS);
        assert_eq!(store.pngs.len(), 1, "one PNG, kept by hash");
        let last = key("App9999");
        assert!(matches!(
            store.lookup(&last, Instant::now()),
            Lookup::Ready(_)
        ));
    }

    #[test]
    fn icon_get_answers_produced_pngs_in_request_order() {
        let mut store = Store::default();
        let png = png::header(16, 16);
        let hash = store.record_produced(key("a"), png.clone());
        assert_eq!(hash, IconHash::of(&png));
        assert_eq!(store.lookup(&key("a"), Instant::now()), Lookup::Ready(hash));

        let unknown = IconHash::of(b"unknown");
        let result = store.icons(&[unknown, hash]);
        assert_eq!(result.missing, [unknown]);
        assert_eq!(result.icons, [Icon { hash, png }]);
    }

    #[test]
    fn the_disk_cache_is_private_and_checked_on_read() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let uid = rustix::process::getuid().as_raw();
        let cache = DiskCache::open(&root.path().join("swayconnect"), uid).expect("open");
        let png = png::header(32, 32);
        assert_eq!(cache.get(b"source"), None);
        cache.put(b"source", &png).expect("put");
        assert_eq!(cache.get(b"source"), Some(png));

        cache.put(b"other", &png::header(512, 512)).expect("put");
        assert_eq!(cache.get(b"other"), None, "an unservable file was served");
    }
}
