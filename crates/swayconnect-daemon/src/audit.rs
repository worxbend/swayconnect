//! Audit log (`docs/DESIGN.md` 11.8): `audit.jsonl` in the data directory, mode 0600, append
//! only, one JSON object per line. Written by slice D-SEC. No rotation (11.13).
//!
//! A [`Record`] holds ids, numbers, lengths and closed enums only. No member can carry a title,
//! typed text or a secret, so no caller can put one into the log (`AGENTS.md` 5 rule 6).

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Write as _};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rustix::fs::{Mode, OFlags};
use serde::Serialize;
use swayconnect_protocol::ops::Operation;
use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_protocol::{DeviceId, ErrorCode, Scope};

use crate::devices::{ScopeChange, ScopesSet, unix_now};
use crate::paths::{Kind, PathError, Paths, check_private, ensure_private_dir, read_private};

const FILE_NAME: &str = "audit.jsonl";
const FILE_MODE: u32 = 0o600;

pub fn audit_file(paths: &Paths) -> PathBuf {
    paths.data_dir.join(FILE_NAME)
}

/// What a line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A device request: a tier 2 action, a keep-awake change or an input session start.
    Request(Operation),
    /// The owner confirmed a pairing; `scopes` holds what the device got.
    Pair,
    Grant,
    Deny,
    Revoke,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Request(operation) => operation.as_str(),
            Op::Pair => "pair",
            Op::Grant => "devices.grant",
            Op::Deny => "devices.deny",
            Op::Revoke => "devices.revoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// A grant or deny that found nothing to change.
    Unchanged,
    /// A revoke that holds until the daemon restarts: `devices.json` was not written.
    NotStored,
    /// The request was answered with this error code.
    Error(ErrorCode),
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Unchanged => "unchanged",
            Outcome::NotStored => "not_stored",
            Outcome::Error(code) => code.as_str(),
        }
    }
}

/// One audit line. Build it with [`Record::new`] and struct update syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub device: DeviceId,
    /// The device's address. `None`: the owner acted through the admin socket.
    pub peer: Option<SocketAddr>,
    pub op: Op,
    /// Server-issued ids the request named, such as window ids.
    pub targets: Vec<u64>,
    /// The device's scopes after a pairing, grant or deny.
    pub scopes: BTreeSet<Scope>,
    /// `keep_awake.set` with `enabled: true`: the duration. Absent on such a line means off.
    pub duration_s: Option<u64>,
    /// Byte length of typed text. The text itself is never recorded.
    pub text_len: Option<usize>,
    pub outcome: Outcome,
}

impl Record {
    pub fn new(device: DeviceId, peer: Option<SocketAddr>, op: Op, outcome: Outcome) -> Record {
        Record {
            device,
            peer,
            op,
            targets: Vec::new(),
            scopes: BTreeSet::new(),
            duration_s: None,
            text_len: None,
            outcome,
        }
    }

    /// The owner's grant or deny through the admin socket, with the scopes after it.
    pub fn scope_change(change: ScopeChange, set: &ScopesSet) -> Record {
        let op = match change {
            ScopeChange::Grant => Op::Grant,
            ScopeChange::Deny => Op::Deny,
        };
        let outcome = if set.changed {
            Outcome::Ok
        } else {
            Outcome::Unchanged
        };
        Record {
            scopes: set.scopes.clone(),
            ..Record::new(set.device_id.clone(), None, op, outcome)
        }
    }
}

/// The wire form of a line; empty members are left out.
#[derive(Serialize)]
struct Line<'a> {
    time: u64,
    device: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer: Option<SocketAddr>,
    op: &'static str,
    #[serde(skip_serializing_if = "<[u64]>::is_empty")]
    targets: &'a [u64],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scopes: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_len: Option<usize>,
    result: &'static str,
}

impl Line<'_> {
    fn of(record: &Record, time: u64) -> Line<'_> {
        Line {
            time,
            device: record.device.as_str(),
            peer: record.peer,
            op: record.op.as_str(),
            targets: &record.targets,
            scopes: record.scopes.iter().map(|scope| scope.as_str()).collect(),
            duration_s: record.duration_s,
            text_len: record.text_len,
            result: record.outcome.as_str(),
        }
    }
}

/// The open log. Clones share one file handle, so lines never interleave.
#[derive(Debug, Clone)]
pub struct AuditLog {
    file: Arc<Mutex<File>>,
}

impl AuditLog {
    /// Creates the file with mode 0600 when it is missing. An existing file must be a regular
    /// file of `uid` with no group or other bits; a symlink is refused, never followed.
    pub fn open(path: &Path, uid: u32) -> Result<AuditLog, PathError> {
        if let Some(directory) = path.parent() {
            ensure_private_dir(directory, uid)?;
        }
        let flags = OFlags::WRONLY | OFlags::APPEND | OFlags::CREATE | OFlags::NOFOLLOW;
        let flags = flags | OFlags::CLOEXEC | OFlags::NOCTTY;
        let file = rustix::fs::open(path, flags, Mode::from_raw_mode(FILE_MODE))
            .map_err(|errno| PathError::io(path)(errno.into()))?;
        check_private(path, Kind::File, uid)?;
        Ok(AuditLog {
            file: Arc::new(Mutex::new(File::from(file))),
        })
    }

    /// Appends one line stamped with the current time.
    pub fn record(&self, record: &Record) -> io::Result<()> {
        let mut line =
            serde_json::to_vec(&Line::of(record, unix_now())).map_err(io::Error::other)?;
        line.push(b'\n');
        // A panic while holding the lock cannot leave the file in a state worse than a short
        // line, which the reader prints like any other.
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        file.write_all(&line)
    }

    /// For a change that already took effect: a line that cannot be written does not undo it,
    /// so the journal says so instead.
    pub fn record_or_warn(&self, record: &Record) {
        if let Err(error) = self.record(record) {
            tracing::warn!(%error, op = record.op.as_str(), "audit line not written");
        }
    }
}

/// The newest `count` lines, each escaped for a terminal: `swayconnectd audit -n N`. A missing
/// log has no lines. The file is checked like every private file before it is read.
pub fn newest_lines_for_terminal(
    path: &Path,
    uid: u32,
    count: usize,
) -> Result<Vec<String>, PathError> {
    let Some(content) = read_private(path, uid)? else {
        return Ok(Vec::new());
    };
    let text = String::from_utf8_lossy(&content);
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let first = lines.len().saturating_sub(count);
    Ok(lines[first..]
        .iter()
        .map(|line| escape_for_terminal(line))
        .collect())
}

#[cfg(test)]
mod tests;
