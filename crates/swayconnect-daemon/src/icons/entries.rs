//! Desktop entry index and the window-to-entry matching order of `docs/V2_RESEARCH.md` 3.
//! Shared with the apps topic (M2b).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::keyfile;

/// Larger files are skipped: real entries are a few KiB.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES: usize = 8192;
/// `applications/` subdirectories give ids with `-`; real trees are one level deep.
const MAX_DIR_DEPTH: usize = 4;
const ENTRY_SUFFIX: &str = ".desktop";

/// An entry that names an icon. Entries without `Icon=` are not indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// The desktop file id, without `.desktop`.
    pub id: String,
    /// The `Icon=` value: an icon name or an absolute path.
    pub icon: String,
    pub startup_wm_class: Option<String>,
}

/// What a window tells about its app. Every field is client-controlled text.
#[derive(Debug, Default)]
pub struct Hints<'a> {
    /// `app_id`, then the X11 class, as present.
    pub names: Vec<&'a str>,
    pub sandbox_app_id: Option<&'a str>,
    pub x11_instance: Option<&'a str>,
    /// From the cgroup of the window's process.
    pub flatpak_id: Option<String>,
    /// The owner's `[icons.aliases]` value for one of the names.
    pub alias: Option<&'a str>,
}

/// Entries by id, from every `applications/` directory under the search dirs.
#[derive(Debug, Default)]
pub struct EntryIndex {
    entries: BTreeMap<String, DesktopEntry>,
}

impl EntryIndex {
    /// Reads `<dir>/applications` for each search dir. An earlier dir wins for the same id,
    /// also when its entry is `Hidden` or has no icon. Unreadable files are skipped.
    pub fn scan(search_dirs: &[PathBuf]) -> EntryIndex {
        let mut seen = HashSet::new();
        let mut entries = BTreeMap::new();
        for dir in search_dirs {
            let mut files = Vec::new();
            collect_files(&dir.join("applications"), "", 0, &mut files);
            for (id, path) in files {
                if seen.len() >= MAX_ENTRIES || !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(entry) = read_entry(id, &path) {
                    entries.insert(entry.id.clone(), entry);
                }
            }
        }
        EntryIndex { entries }
    }

    /// The entry for a window, in the order of `docs/V2_RESEARCH.md` 3: sandbox app id,
    /// exact id, case-insensitive id, `StartupWMClass`, unique reverse-DNS suffix, Xwayland
    /// instance, flatpak hint, owner alias.
    pub fn find(&self, hints: &Hints<'_>) -> Option<&DesktopEntry> {
        let names = &hints.names;
        hints
            .sandbox_app_id
            .and_then(|id| self.entries.get(id))
            .or_else(|| names.iter().find_map(|name| self.entries.get(*name)))
            .or_else(|| names.iter().find_map(|name| self.case_insensitive(name)))
            .or_else(|| names.iter().find_map(|name| self.by_wm_class(name)))
            .or_else(|| names.iter().find_map(|name| self.by_unique_suffix(name)))
            .or_else(|| {
                let instance = hints.x11_instance?;
                self.entries
                    .get(instance)
                    .or_else(|| self.case_insensitive(instance))
            })
            .or_else(|| {
                hints
                    .flatpak_id
                    .as_deref()
                    .and_then(|id| self.entries.get(id))
            })
            .or_else(|| hints.alias.and_then(|id| self.entries.get(id)))
    }

    fn case_insensitive(&self, name: &str) -> Option<&DesktopEntry> {
        self.entries
            .values()
            .find(|entry| entry.id.eq_ignore_ascii_case(name))
    }

    fn by_wm_class(&self, name: &str) -> Option<&DesktopEntry> {
        self.entries.values().find(|entry| {
            entry
                .startup_wm_class
                .as_deref()
                .is_some_and(|class| class.eq_ignore_ascii_case(name))
        })
    }

    /// `org.example.Notes` for `notes`, only when exactly one entry ends that way.
    fn by_unique_suffix(&self, name: &str) -> Option<&DesktopEntry> {
        let suffix = format!(".{}", name.to_ascii_lowercase());
        let mut matching = self
            .entries
            .values()
            .filter(|entry| entry.id.to_ascii_lowercase().ends_with(&suffix));
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    }
}

/// The flatpak app id from `/proc/<pid>/cgroup` text: flatpak runs each app in a scope
/// called `app-flatpak-<id>-<number>.scope`.
pub fn flatpak_id_from_cgroup(cgroup: &str) -> Option<String> {
    cgroup
        .lines()
        .flat_map(|line| line.split('/'))
        .find_map(|unit| unit.strip_prefix("app-flatpak-")?.strip_suffix(".scope"))
        .and_then(|unit| unit.rsplit_once('-'))
        .filter(|(id, number)| {
            !id.is_empty() && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
        })
        .map(|(id, _)| id.to_owned())
}

/// `(id, path)` of every `.desktop` file under `dir`, sorted, so a scan is deterministic.
/// Symlinked directories are not followed (a loop would never end); symlinked files are.
fn collect_files(dir: &Path, prefix: &str, depth: usize, files: &mut Vec<(String, PathBuf)>) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<_> = listing.filter_map(Result::ok).collect();
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let Ok(name) = child.file_name().into_string() else {
            continue;
        };
        let is_real_dir = child.file_type().is_ok_and(|kind| kind.is_dir());
        if is_real_dir && depth < MAX_DIR_DEPTH {
            collect_files(&child.path(), &format!("{prefix}{name}-"), depth + 1, files);
        } else if let Some(stem) = name.strip_suffix(ENTRY_SUFFIX) {
            files.push((format!("{prefix}{stem}"), child.path()));
        }
    }
}

fn read_entry(id: String, path: &Path) -> Option<DesktopEntry> {
    let file = super::open_regular(path).ok()?;
    let mut text = String::new();
    file.take(MAX_ENTRY_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_ENTRY_BYTES {
        return None;
    }
    let groups = keyfile::parse(&text);
    let entry = keyfile::group(&groups, "Desktop Entry")?;
    if entry.get("Hidden") == Some("true") {
        return None;
    }
    let icon = entry.get("Icon").filter(|icon| !icon.is_empty())?;
    Some(DesktopEntry {
        id,
        icon: icon.to_owned(),
        startup_wm_class: entry.get("StartupWMClass").map(str::to_owned),
    })
}

#[cfg(test)]
#[path = "entries_tests.rs"]
mod tests;
