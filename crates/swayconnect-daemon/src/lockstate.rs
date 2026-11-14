//! Desktop facts from `/proc` (`docs/DESIGN.md` 11.6, 11.7 item 3): is a screen locker
//! running, and which keyboard-grabbing programs run, among the processes of the own uid.
//! A scan that cannot read `/proc` is an error, which callers treat as unknown and so fail
//! closed (11.3 rule 10). This is a process heuristic: a locker that is not in
//! `lock_processes` is not seen (`docs/DESIGN.md` 5.6).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, de::Error as _};

use crate::proc::ProgramArgv;

/// The kernel keeps at most 15 bytes of a process name (`comm`).
const MAX_PROCESS_NAME_BYTES: usize = 15;
const DEFAULT_LOCK_PROCESSES: [&str; 1] = ["swaylock"];
const DEFAULT_KEYBOARD_GRABBERS: [&str; 5] = ["fuzzel", "wofi", "bemenu", "tofi", "swaylock"];
/// `read` of a `/proc/<pid>` file after the process was reaped.
const ESRCH: i32 = 3;

/// A process name as `/proc/<pid>/status` shows it: 1 to 15 bytes, no `/`, no backslash
/// (the kernel escapes it there), no control character.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ProcessName(String);

impl ProcessName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProcessName {
    type Error = String;

    fn try_from(name: String) -> Result<ProcessName, String> {
        let fits = (1..=MAX_PROCESS_NAME_BYTES).contains(&name.len());
        let plain = !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control());
        if fits && plain {
            Ok(ProcessName(name))
        } else {
            Err(format!(
                "process name {name:?} must be 1 to {MAX_PROCESS_NAME_BYTES} bytes without '/', '\\' or control characters (a name, not a path)"
            ))
        }
    }
}

/// Config section `[session]` (`docs/DESIGN.md` 11.9). `config.rs` embeds it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    /// The lock program for `session.lock`. `None`: the op is not offered.
    #[serde(default)]
    pub lock: Option<ProgramArgv>,
    /// Never empty: without a name, input would never be refused for a locked screen.
    #[serde(
        default = "default_lock_processes",
        deserialize_with = "non_empty_names"
    )]
    pub lock_processes: Vec<ProcessName>,
    #[serde(default = "default_keyboard_grabbers")]
    pub keyboard_grabbers: Vec<ProcessName>,
}

impl Default for SessionConfig {
    fn default() -> SessionConfig {
        SessionConfig {
            lock: None,
            lock_processes: default_lock_processes(),
            keyboard_grabbers: default_keyboard_grabbers(),
        }
    }
}

fn default_lock_processes() -> Vec<ProcessName> {
    names(&DEFAULT_LOCK_PROCESSES)
}

fn default_keyboard_grabbers() -> Vec<ProcessName> {
    names(&DEFAULT_KEYBOARD_GRABBERS)
}

fn names(defaults: &[&str]) -> Vec<ProcessName> {
    defaults
        .iter()
        .map(|name| ProcessName((*name).to_owned()))
        .collect()
}

fn non_empty_names<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ProcessName>, D::Error> {
    let names = Vec::<ProcessName>::deserialize(deserializer)?;
    if names.is_empty() {
        return Err(D::Error::custom(
            "lock_processes must name at least one program",
        ));
    }
    Ok(names)
}

/// A process of the own uid whose name is in `keyboard_grabbers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grabber {
    pub pid: u32,
    pub name: ProcessName,
}

/// What one scan saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopFacts {
    /// A process of the own uid is in `lock_processes`.
    pub locked: bool,
    pub grabbers: Vec<Grabber>,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot read {}: {source}", path.display())]
pub struct ScanError {
    path: PathBuf,
    #[source]
    source: io::Error,
}

/// Scans `proc_root` (`/proc` outside tests) for processes of `uid`.
#[derive(Debug, Clone)]
pub struct ProcScanner {
    proc_root: PathBuf,
    uid: u32,
    lock_processes: Vec<ProcessName>,
    keyboard_grabbers: Vec<ProcessName>,
}

impl ProcScanner {
    pub fn new(proc_root: PathBuf, uid: u32, config: &SessionConfig) -> ProcScanner {
        ProcScanner {
            proc_root,
            uid,
            lock_processes: config.lock_processes.clone(),
            keyboard_grabbers: config.keyboard_grabbers.clone(),
        }
    }

    /// Blocking file reads; about one small file per process. A process that ends during
    /// the scan is skipped; any other read error ends the scan with an error.
    pub fn scan(&self) -> Result<DesktopFacts, ScanError> {
        let scan_error = |source| ScanError {
            path: self.proc_root.clone(),
            source,
        };
        let mut facts = DesktopFacts::default();
        for entry in fs::read_dir(&self.proc_root).map_err(scan_error)? {
            let entry = entry.map_err(scan_error)?;
            let Some(pid) = entry.file_name().to_str().and_then(parse_pid) else {
                continue;
            };
            if let Some(name) = self.own_process_name(&entry.path())? {
                self.record(&mut facts, pid, &name);
            }
        }
        Ok(facts)
    }

    /// `None` for a process of another uid or one that ended.
    fn own_process_name(&self, process_dir: &Path) -> Result<Option<String>, ScanError> {
        let path = process_dir.join("status");
        let status = match fs::read_to_string(&path) {
            Ok(status) => status,
            Err(error) if has_ended(&error) => return Ok(None),
            Err(source) => return Err(ScanError { path, source }),
        };
        let (name, uid) = parse_status(&status).ok_or_else(|| ScanError {
            path,
            source: io::Error::new(io::ErrorKind::InvalidData, "no Name or Uid line"),
        })?;
        Ok((uid == self.uid).then(|| name.to_owned()))
    }

    fn record(&self, facts: &mut DesktopFacts, pid: u32, name: &str) {
        facts.locked |= find(&self.lock_processes, name).is_some();
        if let Some(grabber) = find(&self.keyboard_grabbers, name) {
            facts.grabbers.push(Grabber {
                pid,
                name: grabber.clone(),
            });
        }
    }
}

fn find<'a>(names: &'a [ProcessName], name: &str) -> Option<&'a ProcessName> {
    names.iter().find(|known| known.as_str() == name)
}

fn parse_pid(file_name: &str) -> Option<u32> {
    let digits = !file_name.is_empty() && file_name.bytes().all(|byte| byte.is_ascii_digit());
    digits.then(|| file_name.parse().ok()).flatten()
}

fn has_ended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(ESRCH)
}

/// The `Name:` value and the real uid of `Uid:` (its first field).
fn parse_status(status: &str) -> Option<(&str, u32)> {
    let field = |key: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(str::trim)
    };
    let name = field("Name:")?;
    let uid = field("Uid:")?.split_whitespace().next()?.parse().ok()?;
    Some((name, uid))
}

/// Writes a fake `/proc/<pid>/status` under `proc_root` for tests of lock and grab checks.
#[cfg(test)]
pub fn add_fake_process(proc_root: &Path, pid: u32, name: &str, uid: u32) {
    let dir = proc_root.join(pid.to_string());
    fs::create_dir_all(&dir).expect("fake process dir");
    let status = format!(
        "Name:\t{name}\nUmask:\t0022\nState:\tS (sleeping)\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"
    );
    fs::write(dir.join("status"), status).expect("fake status");
}

#[cfg(test)]
mod tests;
