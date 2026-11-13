//! Device store (`devices.json`), one revoke signal and one scope signal per device. Written by
//! slice S6a; scope grant and deny by slice D-SEC (`docs/DESIGN.md` 11.8).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use swayconnect_protocol::fingerprint::{decode_base64url, encode_base64url};
use swayconnect_protocol::{DeviceId, Scope};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::paths::{PathError, read_private, write_atomic};
use crate::throttle::LogBudget;

const FILE_VERSION: u32 = 1;
/// The shortest device id prefix that names a device (`docs/DESIGN.md` 11.8).
pub const MIN_PREFIX_CHARS: usize = 4;

/// The pairing secret `S` or a device key `K`. `Debug` never shows the bytes.
pub struct Secret([u8; 32]);

impl Secret {
    pub fn new(bytes: [u8; 32]) -> Secret {
        Secret(bytes)
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

/// A confirmed device, as stored.
#[derive(Debug)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub platform: Option<String>,
    /// From `pair` only.
    pub client: Option<String>,
    pub key: Secret,
    pub scopes: BTreeSet<Scope>,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
}

/// A stored device with what the daemon keeps in memory only.
#[derive(Debug)]
pub struct Entry {
    pub device: Device,
    /// `true`: revoked. Every session of the device holds a receiver.
    pub revoked: watch::Sender<bool>,
    /// The device's scopes; a new value is sent after each change is on disk.
    pub scopes_signal: watch::Sender<BTreeSet<Scope>>,
    /// State of the action bucket (`throttle::Rate::take`).
    pub actions_full_again_at: Instant,
    /// For the log line about an empty action bucket.
    pub rate_limit_log: LogBudget,
    pub last_seen: Option<u64>,
    /// `auth.client` of the latest authentication.
    pub last_client: Option<String>,
}

/// One row of `swayconnectd devices`. The strings come from the device: print them through
/// `escape_for_terminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub platform: Option<String>,
    pub client: Option<String>,
    pub last_client: Option<String>,
    pub scopes: Vec<String>,
    pub created_at: u64,
    pub last_seen: Option<u64>,
    pub connections: usize,
}

/// What `swayconnectd devices grant` and `deny` do with the named scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeChange {
    Grant,
    Deny,
}

/// The scopes of a device after a grant or deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopesSet {
    pub device_id: DeviceId,
    pub scopes: BTreeSet<Scope>,
    /// `false`: the device already had (or lacked) every named scope; nothing was written.
    pub changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    #[error(
        "a device prefix is at least {MIN_PREFIX_CHARS} characters of a device id (see `swayconnectd devices`)"
    )]
    BadPrefix,
    #[error("no device id starts with this prefix")]
    NotFound,
    /// The ids that start with the prefix.
    #[error("the prefix matches {} devices: {}; give more characters", .0.len(), .0.join(", "))]
    Clash(Vec<String>),
    #[error("scopes not changed: {0}")]
    Store(#[from] StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("{}: {reason}", file.display())]
    Format { file: PathBuf, reason: String },
}

// No `Debug`: `key` is `K` in base64url.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredFile {
    version: u32,
    devices: Vec<StoredDevice>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDevice {
    id: String,
    name: String,
    platform: Option<String>,
    client: Option<String>,
    key: String,
    scopes: Vec<String>,
    created_at: u64,
}

impl StoredDevice {
    fn of(device: &Device) -> StoredDevice {
        StoredDevice {
            id: device.id.as_str().to_owned(),
            name: device.name.clone(),
            platform: device.platform.clone(),
            client: device.client.clone(),
            key: encode_base64url(device.key.bytes()),
            scopes: scope_names(&device.scopes),
            created_at: device.created_at,
        }
    }

    /// `Err` names the bad member, never its value.
    fn into_device(self) -> Result<Device, &'static str> {
        let scopes = self
            .scopes
            .iter()
            .map(|name| Scope::from_wire(name).ok_or("a device has an unknown scope"))
            .collect::<Result<BTreeSet<Scope>, _>>()?;
        Ok(Device {
            id: DeviceId::parse(&self.id).map_err(|_| "a device has an invalid id")?,
            name: self.name,
            platform: self.platform,
            client: self.client,
            key: Secret::new(
                decode_base64url(&self.key).map_err(|_| "a device has an invalid key")?,
            ),
            scopes,
            created_at: self.created_at,
        })
    }
}

fn scope_names(scopes: &BTreeSet<Scope>) -> Vec<String> {
    scopes
        .iter()
        .map(|scope| scope.as_str().to_owned())
        .collect()
}

/// Resolves once the scopes in `watch` differ from `held`, the scopes a session was admitted
/// with. A value sent before the session subscribed counts. Never resolves after the device is
/// gone: its revoke signal ends the session then.
pub async fn scopes_changed(watch: &mut watch::Receiver<BTreeSet<Scope>>, held: &BTreeSet<Scope>) {
    if watch.wait_for(|scopes| scopes != held).await.is_err() {
        std::future::pending::<()>().await;
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

/// The confirmed devices. The caller holds the lock; the daemon is the only writer of the file.
#[derive(Debug)]
pub struct DeviceStore {
    file: PathBuf,
    entries: Vec<Entry>,
}

impl DeviceStore {
    /// Loads `devices.json`. A missing file is an empty store.
    pub fn load(file: PathBuf, uid: u32) -> Result<DeviceStore, StoreError> {
        let devices = match read_private(&file, uid)? {
            Some(content) => parse(&content).map_err(|reason| StoreError::Format {
                file: file.clone(),
                reason,
            })?,
            None => Vec::new(),
        };
        let mut store = DeviceStore {
            file,
            entries: Vec::new(),
        };
        for device in devices {
            let (revoked, _) = watch::channel(false);
            store.push(device, revoked);
        }
        Ok(store)
    }

    /// Writes the file first. On failure the device is not added and `revoked` is dropped,
    /// which its receivers read as "revoked".
    pub fn insert(
        &mut self,
        device: Device,
        revoked: watch::Sender<bool>,
    ) -> Result<(), StoreError> {
        let mut stored: Vec<StoredDevice> = self.stored().collect();
        stored.push(StoredDevice::of(&device));
        self.write(stored)?;
        self.push(device, revoked);
        Ok(())
    }

    /// Removes the device and signals its sessions, then writes the file. `Ok(false)`: no such
    /// device. On `Err` the device is revoked in memory but would return after a restart.
    pub fn revoke(&mut self, id: &DeviceId) -> Result<bool, StoreError> {
        let Some(index) = self.position(id) else {
            return Ok(false);
        };
        let entry = self.entries.remove(index);
        entry.revoked.send_replace(true);
        self.write(self.stored().collect())?;
        Ok(true)
    }

    /// Grants or denies `scopes` to the one device whose id starts with `prefix`. Writes the
    /// file first: on `Err` nothing changed. On a change, the device's scope signal fires.
    pub fn change_scopes(
        &mut self,
        prefix: &str,
        change: ScopeChange,
        scopes: &BTreeSet<Scope>,
    ) -> Result<ScopesSet, ChangeError> {
        let index = self.position_by_prefix(prefix)?;
        let entry = &self.entries[index];
        let current = &entry.device.scopes;
        let wanted: BTreeSet<Scope> = match change {
            ScopeChange::Grant => current.union(scopes).copied().collect(),
            ScopeChange::Deny => current.difference(scopes).copied().collect(),
        };
        let changed = wanted != *current;
        if changed {
            let mut stored: Vec<StoredDevice> = self.stored().collect();
            stored[index].scopes = scope_names(&wanted);
            self.write(stored)?;
            let entry = &mut self.entries[index];
            entry.device.scopes.clone_from(&wanted);
            entry.scopes_signal.send_replace(wanted.clone());
        }
        Ok(ScopesSet {
            device_id: self.entries[index].device.id.clone(),
            scopes: wanted,
            changed,
        })
    }

    /// The scopes of a stored device, as they change. `None`: not a stored device.
    pub fn scope_watch(&self, id: &DeviceId) -> Option<watch::Receiver<BTreeSet<Scope>>> {
        let index = self.position(id)?;
        Some(self.entries[index].scopes_signal.subscribe())
    }

    /// Lookup by exact string (spec 5.5).
    pub fn get_mut(&mut self, id: &DeviceId) -> Option<&mut Entry> {
        let index = self.position(id)?;
        self.entries.get_mut(index)
    }

    pub fn contains(&self, id: &DeviceId) -> bool {
        self.position(id).is_some()
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let info = |entry: &Entry| DeviceInfo {
            id: entry.device.id.as_str().to_owned(),
            name: entry.device.name.clone(),
            platform: entry.device.platform.clone(),
            client: entry.device.client.clone(),
            last_client: entry.last_client.clone(),
            scopes: scope_names(&entry.device.scopes),
            created_at: entry.device.created_at,
            last_seen: entry.last_seen,
            connections: entry.revoked.receiver_count(),
        };
        self.entries.iter().map(info).collect()
    }

    fn position(&self, id: &DeviceId) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.device.id.as_str() == id.as_str())
    }

    /// The prefix comes from the owner's command line: checked before any lookup.
    fn position_by_prefix(&self, prefix: &str) -> Result<usize, ChangeError> {
        let is_id_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if prefix.len() < MIN_PREFIX_CHARS || !prefix.chars().all(is_id_char) {
            return Err(ChangeError::BadPrefix);
        }
        let matches: Vec<usize> = (0..self.entries.len())
            .filter(|&index| self.entries[index].device.id.as_str().starts_with(prefix))
            .collect();
        match matches.as_slice() {
            [] => Err(ChangeError::NotFound),
            [index] => Ok(*index),
            clash => Err(ChangeError::Clash(
                clash
                    .iter()
                    .map(|&index| self.entries[index].device.id.as_str().to_owned())
                    .collect(),
            )),
        }
    }

    fn push(&mut self, device: Device, revoked: watch::Sender<bool>) {
        let (scopes_signal, _) = watch::channel(device.scopes.clone());
        self.entries.push(Entry {
            device,
            revoked,
            scopes_signal,
            actions_full_again_at: Instant::now(),
            rate_limit_log: LogBudget::default(),
            last_seen: None,
            last_client: None,
        });
    }

    fn stored(&self) -> impl Iterator<Item = StoredDevice> {
        self.entries
            .iter()
            .map(|entry| StoredDevice::of(&entry.device))
    }

    fn write(&self, devices: Vec<StoredDevice>) -> Result<(), StoreError> {
        let content = serde_json::to_vec(&StoredFile {
            version: FILE_VERSION,
            devices,
        })
        .map_err(|error| StoreError::Format {
            file: self.file.clone(),
            reason: error.to_string(),
        })?;
        Ok(write_atomic(&self.file, &content)?)
    }
}

fn parse(content: &[u8]) -> Result<Vec<Device>, String> {
    // serde_json errors name a position and a member, never a member's value.
    let stored: StoredFile = serde_json::from_slice(content).map_err(|error| error.to_string())?;
    if stored.version != FILE_VERSION {
        return Err(format!("unsupported version {}", stored.version));
    }
    let mut devices: Vec<Device> = Vec::with_capacity(stored.devices.len());
    for stored_device in stored.devices {
        let device = stored_device.into_device()?;
        if devices.iter().any(|other| other.id == device.id) {
            return Err("a device id appears twice".to_owned());
        }
        devices.push(device);
    }
    Ok(devices)
}

#[cfg(test)]
mod tests;
