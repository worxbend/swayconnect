//! Icon Theme Specification lookup: the configured theme, its `Inherits` chain, `hicolor`,
//! then `pixmaps`. Also the rule for absolute `Icon=` paths (`docs/DESIGN.md` 11.5).

use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use swayconnect_protocol::limits::MAX_ICON_SIDE_PX;

use super::keyfile;

pub const FALLBACK_THEME: &str = "hicolor";
const EXTENSIONS: [&str; 2] = ["png", "svg"];
/// Bounds the `Inherits` walk; it also stops at a theme seen before.
const MAX_THEMES: usize = 8;
const MAX_INDEX_BYTES: u64 = 256 * 1024;
const MAX_NAME_BYTES: usize = 255;

/// The file for an `Icon=` value. An absolute value must pass [`allowed_absolute`]; any
/// other value must be a plain icon name.
pub fn resolve(icon: &str, theme: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    if icon.starts_with('/') {
        return allowed_absolute(Path::new(icon), search_dirs);
    }
    // Some entries name the file; the specification wants the bare name.
    let name = EXTENSIONS
        .iter()
        .find_map(|extension| icon.strip_suffix(&format!(".{extension}")[..]))
        .unwrap_or(icon);
    if !is_plain_name(name) {
        return None;
    }
    let mut visited = Vec::new();
    find_in_chain(name, theme, search_dirs, &mut visited)
        .or_else(|| find_in_chain(name, FALLBACK_THEME, search_dirs, &mut visited))
        .or_else(|| find_in_pixmaps(name, search_dirs))
}

/// The canonical path when it lies under `<dir>/icons` or `<dir>/pixmaps` of a search dir
/// (both canonicalised too) and has a png or svg extension. Search dirs include the
/// flatpak exports when the session lists them.
pub fn allowed_absolute(path: &Path, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    let canonical = fs::canonicalize(path).ok()?;
    let extension = canonical.extension()?.to_str()?.to_ascii_lowercase();
    if !EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let is_inside_a_root = search_dirs
        .iter()
        .flat_map(|dir| [dir.join("icons"), dir.join("pixmaps")])
        .filter_map(|root| fs::canonicalize(root).ok())
        .any(|root| canonical.starts_with(root));
    is_inside_a_root.then_some(canonical)
}

/// A theme or icon name that cannot leave its directory.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name.starts_with('.')
        && !name.contains(['/', '\0'])
}

fn find_in_chain(
    name: &str,
    theme: &str,
    search_dirs: &[PathBuf],
    visited: &mut Vec<String>,
) -> Option<PathBuf> {
    if visited.len() >= MAX_THEMES || !is_plain_name(theme) || visited.iter().any(|t| t == theme) {
        return None;
    }
    visited.push(theme.to_owned());
    let index = ThemeIndex::read(theme, search_dirs)?;
    index.lookup(name, search_dirs).or_else(|| {
        index
            .parents
            .iter()
            .find_map(|parent| find_in_chain(name, parent, search_dirs, visited))
    })
}

fn find_in_pixmaps(name: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    search_dirs.iter().find_map(|dir| {
        EXTENSIONS
            .iter()
            .map(|extension| dir.join("pixmaps").join(format!("{name}.{extension}")))
            .find(|file| is_file(file))
    })
}

fn is_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeKind {
    Fixed,
    Scalable,
    Threshold,
}

/// One directory group of `index.theme`.
#[derive(Debug)]
struct SubDir {
    path: String,
    kind: SizeKind,
    size: u32,
    scale: u32,
    min_size: u32,
    max_size: u32,
    threshold: u32,
}

impl SubDir {
    fn from_group(path: &str, group: &keyfile::Group) -> Option<SubDir> {
        let is_relative = Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
        if path.is_empty() || !is_relative {
            return None;
        }
        let number = |key: &str| group.get(key).and_then(|text| text.parse::<u32>().ok());
        let size = number("Size")?;
        let kind = match group.get("Type") {
            Some("Fixed") => SizeKind::Fixed,
            Some("Scalable") => SizeKind::Scalable,
            _ => SizeKind::Threshold,
        };
        Some(SubDir {
            path: path.to_owned(),
            kind,
            size,
            scale: number("Scale").unwrap_or(1),
            min_size: number("MinSize").unwrap_or(size),
            max_size: number("MaxSize").unwrap_or(size),
            threshold: number("Threshold").unwrap_or(2),
        })
    }

    /// `DirectoryMatchesSize` of the specification, for the wanted size at scale 1.
    fn matches(&self, wanted: u32) -> bool {
        self.scale == 1
            && match self.kind {
                SizeKind::Fixed => self.size == wanted,
                SizeKind::Scalable => (self.min_size..=self.max_size).contains(&wanted),
                SizeKind::Threshold => (self.size.saturating_sub(self.threshold)
                    ..=self.size + self.threshold)
                    .contains(&wanted),
            }
    }

    /// `DirectorySizeDistance` of the specification, at scale 1.
    fn distance(&self, wanted: u32) -> u32 {
        let (low, high) = match self.kind {
            SizeKind::Fixed => (self.size, self.size),
            SizeKind::Scalable => (self.min_size, self.max_size),
            SizeKind::Threshold => (
                self.size.saturating_sub(self.threshold),
                self.size + self.threshold,
            ),
        };
        let (low, high) = (
            low.saturating_mul(self.scale),
            high.saturating_mul(self.scale),
        );
        if wanted < low {
            low - wanted
        } else {
            wanted.saturating_sub(high)
        }
    }
}

#[derive(Debug)]
struct ThemeIndex {
    name: String,
    subdirs: Vec<SubDir>,
    parents: Vec<String>,
}

impl ThemeIndex {
    /// The `index.theme` of the first search dir that has one.
    fn read(theme: &str, search_dirs: &[PathBuf]) -> Option<ThemeIndex> {
        let text = search_dirs
            .iter()
            .map(|dir| dir.join("icons").join(theme).join("index.theme"))
            .find_map(|file| read_capped(&file))?;
        let groups = keyfile::parse(&text);
        let header = keyfile::group(&groups, "Icon Theme")?;
        let list = |key: &str| {
            header
                .get(key)
                .into_iter()
                .flat_map(|value| value.split(','))
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let subdirs = list("Directories")
            .into_iter()
            .chain(list("ScaledDirectories"))
            .filter_map(|path| SubDir::from_group(&path, keyfile::group(&groups, &path)?))
            .collect();
        Some(ThemeIndex {
            name: theme.to_owned(),
            subdirs,
            parents: list("Inherits"),
        })
    }

    /// `LookupIcon` of the specification for 128 px: an exact size match first, else the
    /// closest directory.
    fn lookup(&self, name: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
        let wanted = MAX_ICON_SIDE_PX;
        let candidates = |subdir: &SubDir| {
            let relative = Path::new(&self.name).join(&subdir.path);
            search_dirs
                .iter()
                .flat_map(move |dir| {
                    let directory = dir.join("icons").join(&relative);
                    EXTENSIONS.map(|extension| directory.join(format!("{name}.{extension}")))
                })
                .find(|file| is_file(file))
        };
        self.subdirs
            .iter()
            .filter(|subdir| subdir.matches(wanted))
            .find_map(candidates)
            .or_else(|| {
                self.subdirs
                    .iter()
                    .filter_map(|subdir| Some((subdir.distance(wanted), candidates(subdir)?)))
                    .min_by_key(|(distance, _)| *distance)
                    .map(|(_, file)| file)
            })
    }
}

fn read_capped(path: &Path) -> Option<String> {
    let file = super::open_regular(path).ok()?;
    let mut text = String::new();
    file.take(MAX_INDEX_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_INDEX_BYTES).then_some(text)
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
