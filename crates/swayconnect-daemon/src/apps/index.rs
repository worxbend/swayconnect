//! The launchable desktop entries (`docs/DESIGN.md` 11.6, `docs/V2_RESEARCH.md` 3): read
//! from every `applications/` directory of the data dirs, filtered, with the argv each one
//! runs. Blocking file I/O: call it off the async runtime.

use std::collections::HashSet;
use std::fs;
use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use swayconnect_protocol::EntryId;
use swayconnect_protocol::limits::{MAX_KEYWORD_SCALARS, MAX_KEYWORDS, MAX_KEYWORDS_BYTES};
use swayconnect_protocol::text::replace_unsafe;

use super::config::AppsConfig;
use super::exec::{self, ExecLine};
use crate::icons::entries::collect_files;
use crate::icons::keyfile::{self, Group};
use crate::icons::open_regular;

/// Larger files are skipped: real entries are a few KiB.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;
/// Entry files read per scan, across every data dir.
const MAX_ENTRY_FILES: usize = 8192;
/// `name` and `generic_name` of `PROTOCOL.md` 17.11, in Unicode scalar values.
const MAX_APP_TEXT_SCALARS: usize = 128;
/// The desktop name `OnlyShowIn` and `NotShowIn` are compared with.
const DESKTOP_NAME: &str = "sway";
const URL_SCHEME_MIME_PREFIX: &str = "x-scheme-handler/";

/// One launchable entry. Text is already cleaned and cut for the topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedApp {
    pub entry_id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub keywords: Vec<String>,
    /// The entry names an icon, so asking the icon service for it can succeed.
    pub has_icon: bool,
    pub terminal: bool,
    pub risky: bool,
    /// The `Exec` tokens, field codes removed. Never sent to a client.
    pub argv: Vec<String>,
}

/// Where entries and `TryExec` programs are looked up. `main.rs` builds it.
#[derive(Debug, Clone, Default)]
pub struct IndexDirs {
    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS` (`icons::search_dirs`).
    pub data_dirs: Vec<PathBuf>,
    /// The session's `PATH`, for a `TryExec` without a slash.
    pub exec_path: Vec<PathBuf>,
}

/// Every launchable entry, sorted by name (case-insensitive), then id. An earlier data dir
/// wins for the same id, also when its entry is hidden or invalid.
pub fn scan(dirs: &IndexDirs, config: &AppsConfig) -> Vec<IndexedApp> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in &dirs.data_dirs {
        let mut files = Vec::new();
        collect_files(&dir.join("applications"), "", 0, &mut files);
        for (id, path) in files {
            if seen.len() >= MAX_ENTRY_FILES || !seen.insert(id.clone()) {
                continue;
            }
            let text = read_text(&path);
            if let Some(app) = text.and_then(|text| entry_app(id, &text, dirs, config)) {
                apps.push(app);
            }
        }
    }
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.entry_id.clone()));
    apps
}

fn read_text(path: &Path) -> Option<String> {
    let file = open_regular(path).ok()?;
    let mut text = String::new();
    file.take(MAX_ENTRY_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_ENTRY_BYTES).then_some(text)
}

/// The app for one entry file, or `None` when a filter of `DESIGN.md` 11.6 hides it.
fn entry_app(id: String, text: &str, dirs: &IndexDirs, config: &AppsConfig) -> Option<IndexedApp> {
    EntryId::parse(&id)?;
    let groups = keyfile::parse(text);
    let entry = keyfile::group(&groups, "Desktop Entry")?;
    if !is_shown(entry) || config.deny.contains(&id) {
        return None;
    }
    let name = entry
        .get("Name")
        .map(clean_text)
        .filter(|n| !n.is_empty())?;
    let exec = exec::parse_exec(entry.get("Exec")?)
        .inspect_err(|error| tracing::debug!(entry_id = %id, %error, "entry skipped"))
        .ok()?;
    if is_url_only_handler(entry, &exec) || !try_exec_found(entry, &dirs.exec_path) {
        return None;
    }
    let terminal = is_true(entry, "Terminal");
    Some(IndexedApp {
        risky: terminal || is_terminal_emulator(entry) || config.risky.contains(&id),
        entry_id: id,
        name,
        generic_name: entry
            .get("GenericName")
            .map(clean_text)
            .filter(|g| !g.is_empty()),
        keywords: keywords(entry.get("Keywords").unwrap_or_default()),
        has_icon: entry.get("Icon").is_some_and(|icon| !icon.is_empty()),
        terminal,
        argv: exec.argv,
    })
}

fn is_shown(entry: &Group) -> bool {
    let lists_desktop = |key| {
        entry.get(key).map(|value| {
            exec::split_list(value)
                .iter()
                .any(|name| name == DESKTOP_NAME)
        })
    };
    entry.get("Type") == Some("Application")
        && !is_true(entry, "Hidden")
        && !is_true(entry, "NoDisplay")
        && lists_desktop("OnlyShowIn") != Some(false)
        && lists_desktop("NotShowIn") != Some(true)
}

/// A terminal emulator such as foot or Alacritty: `Terminal=false`, but it runs whatever is
/// typed into it, so `DESIGN.md` 11.6 treats it as a terminal.
fn is_terminal_emulator(entry: &Group) -> bool {
    entry.get("Categories").is_some_and(|value| {
        exec::split_list(value)
            .iter()
            .any(|category| category == "TerminalEmulator")
    })
}

fn is_true(entry: &Group, key: &str) -> bool {
    entry.get(key) == Some("true")
}

/// A scheme handler such as a `geo:` helper: it takes only URLs and handles only
/// `x-scheme-handler/*` types, so starting it without a URL does nothing useful.
fn is_url_only_handler(entry: &Group, exec: &ExecLine) -> bool {
    let mime_types = exec::split_list(entry.get("MimeType").unwrap_or_default());
    exec.takes_urls
        && !mime_types.is_empty()
        && mime_types
            .iter()
            .all(|mime| mime.starts_with(URL_SCHEME_MIME_PREFIX))
}

/// `TryExec` absent, or naming an executable file (absolute, or found in `exec_path`).
fn try_exec_found(entry: &Group, exec_path: &[PathBuf]) -> bool {
    let Some(program) = entry.get("TryExec").map(exec::unescape_value) else {
        return true;
    };
    let program = Path::new(&program);
    if program.is_absolute() {
        return is_executable(program);
    }
    program.components().count() == 1
        && exec_path
            .iter()
            .filter(|dir| dir.is_absolute())
            .any(|dir| is_executable(&dir.join(program)))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Replaces first, then cuts (`PROTOCOL.md` 9.3 rule 3, 17.1 rule 7).
fn clean_text(raw: &str) -> String {
    cut_clean(&exec::unescape_value(raw), MAX_APP_TEXT_SCALARS)
}

fn cut_clean(text: &str, max_scalars: usize) -> String {
    replace_unsafe(text).chars().take(max_scalars).collect()
}

/// At most 16 keywords of at most 64 scalars each, together at most 512 bytes (17.11).
fn keywords(raw: &str) -> Vec<String> {
    let mut kept = Vec::new();
    let mut bytes = 0;
    for keyword in exec::split_list(raw) {
        let keyword = cut_clean(&keyword, MAX_KEYWORD_SCALARS);
        if kept.len() == MAX_KEYWORDS || bytes + keyword.len() > MAX_KEYWORDS_BYTES {
            break;
        }
        bytes += keyword.len();
        kept.push(keyword);
    }
    kept
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
