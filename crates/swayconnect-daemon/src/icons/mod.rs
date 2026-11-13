//! Icon service (`docs/DESIGN.md` 11.5, D24): window → desktop entry → icon file → PNG of at
//! most 128 px, served by content hash only. Written by slice D-ICON.
//!
//! [`IconService::icon_for`] never waits: an unknown key is queued for one worker thread,
//! which resolves one key at a time, off the async runtime. Untrusted files are rasterised
//! in a child process ([`raster`]).

pub mod entries;
mod keyfile;
mod png;
pub mod raster;
pub mod store;
pub mod theme;

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use swayconnect_protocol::{IconGetResult, IconHash};
use swayconnect_sway::Node;
use tokio::sync::watch;

use self::entries::{EntryIndex, Hints};
use self::raster::{MAX_SOURCE_BYTES, MAX_SOURCE_SIDE_PX, RasterError, Rasteriser};
use self::store::{DiskCache, Lookup, Store};

/// The desktop entry index is read again at most this often, when a key misses.
const INDEX_REBUILD_INTERVAL: Duration = Duration::from_secs(30);
/// Longer window strings are not looked up; the protocol caps `app_id` at 128 scalars.
const MAX_KEY_FIELD_BYTES: usize = 512;
const MAX_CGROUP_BYTES: u64 = 16 * 1024;
/// Jobs waiting for the worker; a key that finds the queue full is asked for again later.
const MAX_QUEUED_JOBS: usize = 64;
/// `XDG_DATA_DIRS` when unset or empty (XDG Base Directory Specification).
const DEFAULT_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];

/// What identifies a window's app for the icon cache. Every field is client-controlled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IconKey {
    pub app_id: Option<String>,
    pub x11_class: Option<String>,
    pub x11_instance: Option<String>,
    pub sandbox_app_id: Option<String>,
}

impl IconKey {
    /// Every app hint of a window node, raw: the Wayland `app_id`, the Xwayland class and
    /// instance, and the sandbox app id (`V2_RESEARCH.md` 3 matching order).
    pub fn of_window(node: &Node) -> IconKey {
        let properties = node.window_properties.as_ref();
        IconKey {
            app_id: node.app_id.clone(),
            x11_class: properties.and_then(|props| props.class.clone()),
            x11_instance: properties.and_then(|props| props.instance.clone()),
            sandbox_app_id: node.sandbox_app_id.clone(),
        }
    }

    fn is_lookup_worthy(&self) -> bool {
        let fields = [
            &self.app_id,
            &self.x11_class,
            &self.x11_instance,
            &self.sandbox_app_id,
        ];
        let present: Vec<&String> = fields.into_iter().flatten().collect();
        !present.is_empty()
            && present
                .iter()
                .all(|field| field.len() <= MAX_KEY_FIELD_BYTES)
    }
}

/// Config section `[icons]` (`docs/DESIGN.md` 11.9). `config.rs` embeds it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconsConfig {
    /// `app_id` or X11 class → desktop entry id, for windows nothing else matches.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// Icon theme searched before `hicolor`. `None`: `hicolor` only.
    #[serde(default)]
    pub theme: Option<String>,
}

/// Everything the service reads. `main.rs` builds it; tests point it at temp dirs.
#[derive(Debug, Clone)]
pub struct IconSettings {
    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`; see [`search_dirs`].
    pub search_dirs: Vec<PathBuf>,
    /// The daemon's cache directory (`$XDG_CACHE_HOME/swayconnect`). `None`: no disk cache.
    pub cache_dir: Option<PathBuf>,
    pub uid: u32,
    /// `/proc` outside tests; read for the flatpak hint of a window's process.
    pub proc_root: PathBuf,
    pub config: IconsConfig,
}

/// The data dirs to search, highest priority first. `xdg_data_dirs` is the raw variable;
/// relative entries are dropped, as the XDG rules say.
pub fn search_dirs(data_home: &Path, xdg_data_dirs: Option<&OsStr>) -> Vec<PathBuf> {
    let listed: Vec<PathBuf> = xdg_data_dirs
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|dir| dir.is_absolute())
        .collect();
    let rest = if listed.is_empty() {
        DEFAULT_DATA_DIRS.iter().map(PathBuf::from).collect()
    } else {
        listed
    };
    std::iter::once(data_home.to_path_buf())
        .chain(rest)
        .collect()
}

/// Why a key got no icon. Logged at debug level only; it ends in the negative cache.
#[derive(Debug, thiserror::Error)]
enum IconError {
    #[error("no desktop entry matches")]
    NoEntry,
    #[error("icon `{0}` not found or not allowed")]
    NoFile(String),
    #[error("icon file: {0}")]
    Source(#[from] io::Error),
    #[error("icon file is larger than {MAX_SOURCE_BYTES} bytes")]
    SourceTooLarge,
    #[error("PNG header is invalid or wider than {MAX_SOURCE_SIDE_PX} px")]
    PngHeader,
    #[error(transparent)]
    Raster(#[from] RasterError),
}

/// The running service. Cheap to share behind an `Arc`; dropping it stops the worker
/// after its current job.
#[derive(Debug)]
pub struct IconService {
    shared: Arc<Shared>,
    jobs: mpsc::SyncSender<Job>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    finished: watch::Sender<u64>,
}

#[derive(Debug, Default)]
struct State {
    store: Store,
    pending: HashSet<IconKey>,
}

#[derive(Debug)]
struct Job {
    key: IconKey,
    pid: Option<u32>,
}

impl IconService {
    /// Starts the worker thread. The disk cache is optional: when it cannot be opened the
    /// service runs without it and says so in the log.
    pub fn start(settings: IconSettings, rasteriser: impl Rasteriser) -> io::Result<IconService> {
        let disk = settings.cache_dir.as_deref().and_then(|dir| {
            DiskCache::open(dir, settings.uid)
                .inspect_err(|error| tracing::warn!(%error, "icon disk cache disabled"))
                .ok()
        });
        let (finished, _) = watch::channel(0);
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            finished,
        });
        let (jobs, queue) = mpsc::sync_channel(MAX_QUEUED_JOBS);
        let resolver = Resolver {
            settings,
            rasteriser,
            disk,
            index: EntryIndex::default(),
            built_at: None,
        };
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("icons".to_owned())
            .spawn(move || resolver.run(&queue, &worker_shared))?;
        Ok(IconService { shared, jobs })
    }

    /// The hash for a window's icon when its PNG exists. Otherwise queues the key (once)
    /// and answers `None`; [`finished`](Self::finished) changes when the job ends.
    pub fn icon_for(&self, key: &IconKey, pid: Option<u32>) -> Option<IconHash> {
        if !key.is_lookup_worthy() {
            return None;
        }
        let mut state = self.shared.lock();
        match state.store.lookup(key, Instant::now()) {
            Lookup::Ready(hash) => return Some(hash),
            Lookup::Missing => return None,
            Lookup::Unknown => {}
        }
        if state.pending.insert(key.clone()) {
            let job = Job {
                key: key.clone(),
                pid,
            };
            match self.jobs.try_send(job) {
                Ok(()) => {}
                // A later refresh asks again: the key is not left pending.
                Err(mpsc::TrySendError::Full(_)) => {
                    state.pending.remove(key);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    state.pending.remove(key);
                    tracing::warn!("icon worker stopped; icons are not resolved");
                }
            }
        }
        None
    }

    /// What the service knows about `key`, without queueing it.
    pub fn lookup(&self, key: &IconKey) -> Lookup {
        self.shared.lock().store.lookup(key, Instant::now())
    }

    /// The `icon.get` result. Never waits for the worker.
    pub fn icons(&self, hashes: &[IconHash]) -> IconGetResult {
        self.shared.lock().store.icons(hashes)
    }

    /// Counts finished jobs, with or without an icon. A publisher re-reads `icon_for` for
    /// its windows when it changes.
    pub fn finished(&self) -> watch::Receiver<u64> {
        self.shared.finished.subscribe()
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn finish(&self, key: IconKey, outcome: Result<Vec<u8>, IconError>) {
        {
            let mut state = self.lock();
            state.pending.remove(&key);
            match outcome {
                Ok(png) => {
                    state.store.record_produced(key, png);
                }
                Err(error) => {
                    tracing::debug!(app_id = ?key.app_id, %error, "no icon");
                    state.store.record_missing(key, Instant::now());
                }
            }
        }
        self.finished.send_modify(|count| *count += 1);
    }
}

struct Resolver<R> {
    settings: IconSettings,
    rasteriser: R,
    disk: Option<DiskCache>,
    index: EntryIndex,
    built_at: Option<Instant>,
}

impl<R: Rasteriser> Resolver<R> {
    fn run(mut self, queue: &mpsc::Receiver<Job>, shared: &Shared) {
        while let Ok(job) = queue.recv() {
            let outcome = self.produce(&job);
            shared.finish(job.key, outcome);
        }
    }

    fn produce(&mut self, job: &Job) -> Result<Vec<u8>, IconError> {
        let icon = self.icon_value(job).ok_or(IconError::NoEntry)?;
        let theme = self
            .settings
            .config
            .theme
            .as_deref()
            .unwrap_or(theme::FALLBACK_THEME);
        let file = theme::resolve(&icon, theme, &self.settings.search_dirs)
            .ok_or(IconError::NoFile(icon))?;
        let source = read_source(&file)?;
        self.png_from(&source)
    }

    /// The `Icon=` value of the matching entry. A miss reads the entries again when the
    /// index is older than [`INDEX_REBUILD_INTERVAL`]: the app may be new.
    fn icon_value(&mut self, job: &Job) -> Option<String> {
        let hints = hints(&self.settings, job);
        let icon_in = |index: &EntryIndex| index.find(&hints).map(|entry| entry.icon.clone());
        let may_rescan = match self.built_at {
            None => {
                self.index = EntryIndex::scan(&self.settings.search_dirs);
                self.built_at = Some(Instant::now());
                false
            }
            Some(built_at) => built_at.elapsed() >= INDEX_REBUILD_INTERVAL,
        };
        if let Some(icon) = icon_in(&self.index) {
            return Some(icon);
        }
        if !may_rescan {
            return None;
        }
        self.index = EntryIndex::scan(&self.settings.search_dirs);
        self.built_at = Some(Instant::now());
        icon_in(&self.index)
    }

    /// A PNG within the limits is served as it is; anything else goes through the disk
    /// cache or the rasteriser. A PNG header over 1024 px is refused here already.
    fn png_from(&self, source: &[u8]) -> Result<Vec<u8>, IconError> {
        if png::has_signature(source) {
            let (width, height) = png::declared_size(source).ok_or(IconError::PngHeader)?;
            if width > MAX_SOURCE_SIDE_PX || height > MAX_SOURCE_SIDE_PX {
                return Err(IconError::PngHeader);
            }
            if png::is_servable(source) {
                return Ok(source.to_vec());
            }
        }
        if let Some(cached) = self.disk.as_ref().and_then(|disk| disk.get(source)) {
            return Ok(cached);
        }
        let output = self.rasteriser.rasterise(source)?;
        if !png::is_servable(&output) {
            return Err(RasterError::OutputNotServable.into());
        }
        if let Some(Err(error)) = self.disk.as_ref().map(|disk| disk.put(source, &output)) {
            tracing::warn!(%error, "icon not written to the disk cache");
        }
        Ok(output)
    }
}

fn hints<'a>(settings: &'a IconSettings, job: &'a Job) -> Hints<'a> {
    let key = &job.key;
    let names: Vec<&str> = [&key.app_id, &key.x11_class]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let alias = names
        .iter()
        .find_map(|name| settings.config.aliases.get(*name))
        .map(String::as_str);
    Hints {
        names,
        sandbox_app_id: key.sandbox_app_id.as_deref(),
        x11_instance: key.x11_instance.as_deref(),
        flatpak_id: job.pid.and_then(|pid| flatpak_id(&settings.proc_root, pid)),
        alias,
    }
}

fn flatpak_id(proc_root: &Path, pid: u32) -> Option<String> {
    let file = proc_root.join(pid.to_string()).join("cgroup");
    let mut text = String::new();
    fs::File::open(file)
        .ok()?
        .take(MAX_CGROUP_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    entries::flatpak_id_from_cgroup(&text)
}

/// Opens `path` for reading only when it is a regular file. The open does not block, so
/// a FIFO named like an icon or an index cannot stall the one worker thread; the type is
/// checked on the opened handle, so a swap after a path check does not matter.
pub(crate) fn open_regular(path: &Path) -> io::Result<fs::File> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY;
    let opened = fs::File::from(rustix::fs::open(path, flags, Mode::empty())?);
    if !opened.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    // A regular file ignores O_NONBLOCK, so reads stay plain blocking reads.
    Ok(opened)
}

/// At most [`MAX_SOURCE_BYTES`] of a regular file.
fn read_source(file: &Path) -> Result<Vec<u8>, IconError> {
    let opened = open_regular(file)?;
    let mut source = Vec::new();
    opened
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut source)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(IconError::SourceTooLarge);
    }
    Ok(source)
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
