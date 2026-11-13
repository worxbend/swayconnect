//! Device store (`devices.json`), one revoke signal per device. Written by slice S6a.

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

    fn push(&mut self, device: Device, revoked: watch::Sender<bool>) {
        self.entries.push(Entry {
            device,
            revoked,
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
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt as _;

    use super::*;

    const KEY: [u8; 32] = [0xAB; 32];
    const KEY_TEXT: &str = "q6urq6urq6urq6urq6urq6urq6urq6urq6urq6urq6s";

    fn device(id_byte: u8) -> Device {
        Device {
            id: DeviceId::from_bytes([id_byte; 16]),
            name: "phone".to_owned(),
            platform: Some("android".to_owned()),
            client: None,
            key: Secret::new(KEY),
            scopes: BTreeSet::from([Scope::StateRead, Scope::Control]),
            created_at: 1_800_000_000,
        }
    }

    fn store_in(root: &tempfile::TempDir) -> (DeviceStore, PathBuf, u32) {
        let file = root.path().join("devices.json");
        let uid = fs::metadata(root.path()).expect("metadata").uid();
        let store = DeviceStore::load(file.clone(), uid).expect("empty store");
        (store, file, uid)
    }

    fn format_error(file: PathBuf, uid: u32, content: &str) -> String {
        write_atomic(&file, content.as_bytes()).expect("write");
        match DeviceStore::load(file, uid) {
            Err(StoreError::Format { reason, .. }) => reason,
            other => panic!("expected a format error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_stored_device_survives_a_reload_in_the_documented_format() {
        let root = tempfile::tempdir().expect("temp dir");
        let (mut store, file, uid) = store_in(&root);
        assert!(!file.exists());
        let (revoked, _receiver) = watch::channel(false);
        store.insert(device(1), revoked).expect("insert");

        let text = fs::read_to_string(&file).expect("file");
        let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(json["version"], 1);
        assert_eq!(json["devices"][0]["key"], KEY_TEXT);
        assert_eq!(json["devices"][0]["scopes"][1], "control");
        assert_eq!(fs::metadata(&file).expect("metadata").mode() & 0o777, 0o600);

        let mut reloaded = DeviceStore::load(file, uid).expect("reload");
        let id = DeviceId::from_bytes([1; 16]);
        let entry = reloaded.get_mut(&id).expect("device");
        assert_eq!(entry.device.key.bytes(), &KEY);
        assert_eq!(entry.device.created_at, 1_800_000_000);
        assert_eq!(reloaded.list()[0].connections, 0);
        assert_eq!(reloaded.list()[0].name, "phone");
    }

    #[tokio::test]
    async fn revoke_signals_receivers_created_before_and_after_it() {
        let root = tempfile::tempdir().expect("temp dir");
        let (mut store, file, uid) = store_in(&root);
        let (revoked, mut before) = watch::channel(false);
        store.insert(device(1), revoked).expect("insert");
        store
            .insert(device(2), watch::channel(false).0)
            .expect("insert");
        let id = DeviceId::from_bytes([1; 16]);
        assert_eq!(store.list()[0].connections, 1);

        assert!(store.revoke(&id).expect("revoke"));

        assert!(!store.contains(&id));
        before.changed().await.expect("the revoke is a change");
        assert!(*before.borrow());
        let after = before.clone();
        assert!(*after.borrow());
        assert!(!store.revoke(&id).expect("second revoke"));
        let reloaded = DeviceStore::load(file, uid).expect("reload");
        assert!(!reloaded.contains(&id));
        assert!(reloaded.contains(&DeviceId::from_bytes([2; 16])));
    }

    #[tokio::test]
    async fn a_failed_write_adds_nothing_and_closes_the_revoke_channel() {
        let root = tempfile::tempdir().expect("temp dir");
        let uid = fs::metadata(root.path()).expect("metadata").uid();
        let missing_dir = root.path().join("missing").join("devices.json");
        let mut store = DeviceStore::load(missing_dir, uid).expect("empty store");
        let (revoked, mut receiver) = watch::channel(false);

        assert!(store.insert(device(1), revoked).is_err());

        assert!(store.list().is_empty());
        assert!(receiver.changed().await.is_err());
    }

    #[tokio::test]
    async fn a_bad_file_is_refused_without_echoing_the_key() {
        let root = tempfile::tempdir().expect("temp dir");
        let (_, file, uid) = store_in(&root);
        let entry = |id: &str, key: &str, scope: &str| {
            format!(
                r#"{{"id":"{id}","name":"n","platform":null,"client":null,"key":"{key}","scopes":["{scope}"],"created_at":1}}"#
            )
        };
        let id = DeviceId::from_bytes([1; 16]);
        let good = entry(id.as_str(), KEY_TEXT, "control");
        let cases = [
            (r#"{"version":2,"devices":[]}"#.to_owned(), "version"),
            (
                format!(r#"{{"version":1,"devices":[{good},{good}]}}"#),
                "twice",
            ),
            (
                format!(
                    r#"{{"version":1,"devices":[{}]}}"#,
                    entry("short", KEY_TEXT, "control")
                ),
                "invalid id",
            ),
            (
                format!(
                    r#"{{"version":1,"devices":[{}]}}"#,
                    entry(id.as_str(), "SECRETKEY", "control")
                ),
                "invalid key",
            ),
            (
                format!(
                    r#"{{"version":1,"devices":[{}]}}"#,
                    entry(id.as_str(), KEY_TEXT, "root")
                ),
                "unknown scope",
            ),
            (
                r#"{"version":1,"devices":[],"extra":1}"#.to_owned(),
                "extra",
            ),
        ];
        for (content, expected) in cases {
            let reason = format_error(file.clone(), uid, &content);
            assert!(reason.contains(expected), "{reason}");
            assert!(!reason.contains("SECRETKEY") && !reason.contains(KEY_TEXT));
        }
    }

    #[test]
    fn debug_output_redacts_the_key() {
        let text = format!("{:?}", device(1));
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("171") && !text.to_lowercase().contains("ab, ab"));
    }
}
