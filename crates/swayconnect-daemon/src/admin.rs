//! Admin socket server and client (`docs/DESIGN.md` 5.4). Written by slice S6a.
//!
//! One JSON object per line. Internal and unversioned: the daemon and its subcommands ship in
//! one binary. Lines are never logged: a reply can hold the pairing URI.

use std::collections::BTreeSet;
use std::fs::{self, Permissions};
use std::io;
use std::net::SocketAddr;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{DeviceId, Scope};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::audit::{AuditLog, Op, Outcome, Record, audit_file};
use crate::config::StartError;
use crate::devices::{DeviceInfo, ScopeChange, Secret};
use crate::pairing::{OpenedWindow, PairingOutcome, PendingNotice};
use crate::paths::{
    Kind, PathError, Paths, Problem, check_private, ensure_private_dir, own_uid, require_absolute,
};
use crate::security::Security;

const SOCKET_MODE: u32 = 0o600;
const MIN_LIFETIME_S: u64 = 30;
const MAX_LIFETIME_S: u64 = 600;
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("admin socket: {0}")]
    Io(#[from] io::Error),
    #[error("swayconnectd is not running (no admin socket at {})", .0.display())]
    NotRunning(PathBuf),
    #[error("the admin socket peer has uid {peer}, expected {own}")]
    WrongPeer { peer: u32, own: u32 },
    #[error("unexpected message on the admin socket")]
    Protocol,
    #[error("the daemon closed the admin connection")]
    Closed,
    #[error("daemon: {0}")]
    Daemon(String),
}

/// What `swayconnectd pair` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRequest {
    pub scopes: BTreeSet<Scope>,
    /// 30 to 600 seconds.
    pub lifetime: Duration,
    /// Host for the pairing URI. `None`: the daemon picks a listen address.
    pub host: Option<String>,
}

/// A device that waits for the owner's answer. The strings come from the device: print them
/// through `escape_for_terminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDevice {
    pub device_name: String,
    pub platform: Option<String>,
    pub client: Option<String>,
    pub peer: SocketAddr,
    pub confirm_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub scopes: Vec<String>,
}

/// The scopes of a device after `devices.grant` or `devices.deny`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedScopes {
    pub device_id: String,
    pub scopes: Vec<String>,
    /// `false`: nothing to change, so no session was ended.
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingResult {
    Paired(PairedDevice),
    Refused,
    /// No answer within the confirmation deadline.
    DeadlinePassed,
    /// The device closed its connection before the answer.
    ClientGone,
}

/// What the pairing URI tells a client about this daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advertised {
    pub server_name: String,
    /// The addresses the listeners are bound to, with their real ports.
    pub bound: Vec<SocketAddr>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum Request {
    #[serde(rename = "pair.open")]
    PairOpen {
        scopes: Vec<String>,
        lifetime_s: u64,
        host: Option<String>,
    },
    #[serde(rename = "devices.list")]
    DevicesList,
    #[serde(rename = "devices.revoke")]
    DevicesRevoke { device_id: String },
    #[serde(rename = "devices.grant")]
    DevicesGrant { prefix: String, scopes: Vec<String> },
    #[serde(rename = "devices.deny")]
    DevicesDeny { prefix: String, scopes: Vec<String> },
}

// No `Debug`: `Window` holds the pairing URI.
#[derive(Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
enum Reply {
    Window { uri: String },
    WindowExpired,
    Pending(PendingDevice),
    Paired(PairedDevice),
    Refused,
    DeadlinePassed,
    ClientGone,
    Devices { devices: Vec<DeviceInfo> },
    Revoked { found: bool },
    Scopes(ChangedScopes),
    Error { message: String },
}

#[derive(Serialize, Deserialize)]
struct Answer {
    confirm: bool,
}

/// JSON lines over one admin connection.
struct Channel {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
}

impl Channel {
    fn new(stream: UnixStream) -> Channel {
        let (reader, writer) = stream.into_split();
        Channel {
            lines: BufReader::new(reader).lines(),
            writer,
        }
    }

    async fn send(&mut self, message: &impl Serialize) -> Result<(), AdminError> {
        let mut line = serde_json::to_vec(message).map_err(|_| AdminError::Protocol)?;
        line.push(b'\n');
        Ok(self.writer.write_all(&line).await?)
    }

    /// `None`: the peer closed the connection. Cancel safe.
    async fn receive<T: DeserializeOwned>(&mut self) -> Result<Option<T>, AdminError> {
        match self.lines.next_line().await? {
            Some(line) => serde_json::from_str(&line)
                .map(Some)
                .map_err(|_| AdminError::Protocol),
            None => Ok(None),
        }
    }
}

fn check_peer_uid(peer: u32, own: u32) -> Result<(), AdminError> {
    if peer == own {
        Ok(())
    } else {
        Err(AdminError::WrongPeer { peer, own })
    }
}

/// `XDG_RUNTIME_DIR` must be set, absolute, owned by this user and mode 0700. No fallback.
fn check_runtime_dir(paths: &Paths, uid: u32) -> Result<(), PathError> {
    require_absolute(&paths.runtime_dir)?;
    check_private(&paths.runtime_dir, Kind::Directory, uid)
}

/// A stale `admin.sock` is unlinked only if it is a socket owned by this user and no daemon
/// listens on it. Unlinking the socket of a running daemon would cut it off from `revoke`.
fn remove_stale_socket(socket: &Path, uid: u32) -> Result<(), StartError> {
    match fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == uid => {}
        Ok(_) => return Err(PathError::refused(socket, Problem::WrongKind).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(PathError::io(socket)(error).into()),
    }
    // A local connect is answered at once: accepted by the kernel, or refused.
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(_listening_daemon) => Err(StartError::AlreadyRunning(socket.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            Ok(fs::remove_file(socket).map_err(PathError::io(socket))?)
        }
        Err(error) => Err(PathError::io(socket)(error).into()),
    }
}

/// The admin socket of a running daemon.
#[derive(Debug)]
pub struct AdminServer {
    listener: UnixListener,
    socket: PathBuf,
    uid: u32,
    audit: AuditLog,
}

impl AdminServer {
    /// Checks the directories and refuses the start when a daemon already serves
    /// `admin.sock`. `Daemon::start` calls it before it binds TCP: a second daemon then ends
    /// with "already running" (exit 2) and not with an address in use (exit 1, restarted).
    pub fn claim(paths: &Paths) -> Result<(), StartError> {
        let uid = own_uid()?;
        check_runtime_dir(paths, uid)?;
        ensure_private_dir(&paths.admin_dir(), uid)?;
        remove_stale_socket(&paths.admin_socket(), uid)
    }

    /// Runs [`AdminServer::claim`], opens the audit log and binds `admin.sock`. Needs a tokio
    /// runtime.
    pub fn bind(paths: &Paths) -> Result<AdminServer, StartError> {
        AdminServer::claim(paths)?;
        let uid = own_uid()?;
        let audit = AuditLog::open(&audit_file(paths), uid)?;
        let socket = paths.admin_socket();
        let listener = UnixListener::bind(&socket).map_err(PathError::io(&socket))?;
        // Until this line the socket has the mode the umask gives it; the 0700 directory
        // already keeps every other user out.
        fs::set_permissions(&socket, Permissions::from_mode(SOCKET_MODE))
            .map_err(PathError::io(&socket))?;
        Ok(AdminServer {
            listener,
            socket,
            uid,
            audit,
        })
    }

    /// The daemon's one audit log, for the request path (`docs/DESIGN.md` 11.8).
    pub fn audit(&self) -> AuditLog {
        self.audit.clone()
    }

    /// Serves until `shutdown` turns `true` or closes, then removes the socket.
    pub async fn run(
        self,
        security: Security,
        advertised: Advertised,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let advertised = Arc::new(advertised);
        // Dropping the set at the end aborts the connections that are still open.
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        let session = Session {
                            security: security.clone(),
                            advertised: Arc::clone(&advertised),
                            audit: self.audit.clone(),
                        };
                        connections.spawn(session.serve(stream, self.uid));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "admin socket accept failed");
                        tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                    }
                },
                Some(_) = connections.join_next() => {}
                () = stop_requested(&mut shutdown) => break,
            }
        }
        if let Err(error) = fs::remove_file(&self.socket) {
            tracing::warn!(%error, "admin socket not removed");
        }
    }
}

/// Resolves when the daemon shuts down or the sender is gone. Returns nothing, so the select
/// in `run` holds no borrow of the channel across an await.
async fn stop_requested(shutdown: &mut watch::Receiver<bool>) {
    // Both a `true` and a closed channel mean stop.
    let _ = shutdown.wait_for(|stop| *stop).await;
}

struct Session {
    security: Security,
    advertised: Arc<Advertised>,
    audit: AuditLog,
}

impl Session {
    async fn serve(self, stream: UnixStream, own_uid: u32) {
        if let Err(error) = self.serve_requests(stream, own_uid).await {
            tracing::warn!(%error, "admin connection failed");
        }
    }

    async fn serve_requests(&self, stream: UnixStream, own_uid: u32) -> Result<(), AdminError> {
        check_peer_uid(stream.peer_cred()?.uid(), own_uid)?;
        let mut channel = Channel::new(stream);
        while let Some(request) = channel.receive::<Request>().await? {
            match request {
                Request::DevicesList => {
                    let devices = self.security.devices();
                    channel.send(&Reply::Devices { devices }).await?;
                }
                Request::DevicesRevoke { device_id } => {
                    channel.send(&self.revoke(&device_id)).await?;
                }
                Request::DevicesGrant { prefix, scopes } => {
                    let reply = self.change_scopes(&prefix, ScopeChange::Grant, &scopes);
                    channel.send(&reply).await?;
                }
                Request::DevicesDeny { prefix, scopes } => {
                    let reply = self.change_scopes(&prefix, ScopeChange::Deny, &scopes);
                    channel.send(&reply).await?;
                }
                Request::PairOpen {
                    scopes,
                    lifetime_s,
                    host,
                } => match self.open_window(&scopes, lifetime_s, host) {
                    Ok((uri, opened)) => {
                        channel.send(&Reply::Window { uri }).await?;
                        return pair(&mut channel, opened, &self.audit).await;
                    }
                    Err(message) => channel.send(&Reply::Error { message }).await?,
                },
            }
        }
        Ok(())
    }

    fn revoke(&self, device_id: &str) -> Reply {
        let Ok(device_id) = DeviceId::parse(device_id) else {
            return Reply::Error {
                message: "not a device id".to_owned(),
            };
        };
        let revoked = self.security.revoke(&device_id);
        let outcome = match &revoked {
            Ok(false) => None,
            Ok(true) => Some(Outcome::Ok),
            Err(_) => Some(Outcome::NotStored),
        };
        if let Some(outcome) = outcome {
            let line = Record::new(device_id, None, Op::Revoke, outcome);
            self.audit.record_or_warn(&line);
        }
        match revoked {
            Ok(found) => Reply::Revoked { found },
            Err(error) => Reply::Error {
                message: format!("revoked until the daemon restarts, but not on disk: {error}"),
            },
        }
    }

    fn change_scopes(&self, prefix: &str, change: ScopeChange, scopes: &[String]) -> Reply {
        let scopes = match parse_scopes(scopes) {
            Ok(scopes) => scopes,
            Err(message) => return Reply::Error { message },
        };
        match self.security.change_scopes(prefix, change, &scopes) {
            Ok(set) => {
                self.audit
                    .record_or_warn(&Record::scope_change(change, &set));
                Reply::Scopes(ChangedScopes {
                    device_id: set.device_id.as_str().to_owned(),
                    scopes: set
                        .scopes
                        .iter()
                        .map(|scope| scope.as_str().to_owned())
                        .collect(),
                    changed: set.changed,
                })
            }
            Err(error) => Reply::Error {
                message: error.to_string(),
            },
        }
    }

    /// `Err` holds the message for the owner.
    fn open_window(
        &self,
        scopes: &[String],
        lifetime_s: u64,
        host: Option<String>,
    ) -> Result<(String, OpenedWindow), String> {
        if !(MIN_LIFETIME_S..=MAX_LIFETIME_S).contains(&lifetime_s) {
            return Err(format!(
                "the lifetime must be {MIN_LIFETIME_S} to {MAX_LIFETIME_S} seconds"
            ));
        }
        let scopes = parse_scopes(scopes)?;
        let opened = self
            .security
            .open_pairing(scopes, Duration::from_secs(lifetime_s))
            .map_err(|error| error.to_string())?;
        // On `Err` the window is dropped here, which cancels it.
        let uri = self.pairing_uri(host, &opened.secret)?;
        Ok((uri, opened))
    }

    fn pairing_uri(&self, host: Option<String>, secret: &Secret) -> Result<String, String> {
        let bound = &self.advertised.bound;
        let address = bound
            .iter()
            .find(|address| !address.ip().is_loopback())
            .or(bound.first())
            .ok_or("the daemon listens on no address")?;
        let uri = PairingUri {
            host: host.unwrap_or_else(|| address.ip().to_string()),
            port: address.port(),
            fp: self.security.fingerprint(),
            secret: *secret.bytes(),
            name: Some(self.advertised.server_name.clone()),
        }
        .to_uri_string();
        match PairingUri::parse(&uri) {
            Ok(_) => Ok(uri),
            Err(_) => Err("the host is not an IP address or a DNS name".to_owned()),
        }
    }
}

/// At least one scope, every name known. `Err` holds the message for the owner.
fn parse_scopes(names: &[String]) -> Result<BTreeSet<Scope>, String> {
    let scopes = names
        .iter()
        .map(|name| Scope::from_wire(name).ok_or("unknown scope"))
        .collect::<Result<BTreeSet<Scope>, _>>()?;
    if scopes.is_empty() {
        return Err("at least one scope is needed".to_owned());
    }
    Ok(scopes)
}

/// The rest of a `pair.open` connection: pending device, the owner's answer, the result.
/// Returning early drops the window or the answer channel, which cancels the pairing.
async fn pair(
    channel: &mut Channel,
    opened: OpenedWindow,
    audit: &AuditLog,
) -> Result<(), AdminError> {
    let notice = tokio::select! {
        notice = opened.pending => notice.ok(),
        () = tokio::time::sleep_until(opened.expires_at) => None,
        // The client has nothing to say yet: any input, and the end of the connection, cancel.
        _ = channel.lines.next_line() => return Ok(()),
    };
    let Some(notice) = notice else {
        return channel.send(&Reply::WindowExpired).await;
    };
    let PendingNotice {
        device_name,
        platform,
        client,
        peer,
        confirm_code,
        answer,
        mut outcome,
    } = notice;
    let pending = PendingDevice {
        device_name,
        platform,
        client,
        peer,
        confirm_code,
    };
    channel.send(&Reply::Pending(pending)).await?;
    let outcome = tokio::select! {
        owner_answer = channel.receive::<Answer>() => {
            let Some(owner_answer) = owner_answer? else {
                return Ok(());
            };
            // When the deadline passed first, nobody reads the answer and `outcome` says so.
            let _ = answer.send(owner_answer.confirm);
            (&mut outcome).await
        }
        outcome = &mut outcome => outcome,
    };
    let reply = match outcome {
        Ok(PairingOutcome::Paired {
            device_id,
            device_name,
            scopes,
        }) => {
            let paired = PairedDevice {
                device_id: device_id.as_str().to_owned(),
                device_name,
                scopes: scopes
                    .iter()
                    .map(|scope| scope.as_str().to_owned())
                    .collect(),
            };
            let line = Record {
                scopes,
                ..Record::new(device_id, Some(peer), Op::Pair, Outcome::Ok)
            };
            audit.record_or_warn(&line);
            Reply::Paired(paired)
        }
        Ok(PairingOutcome::Refused) => Reply::Refused,
        Ok(PairingOutcome::DeadlinePassed) => Reply::DeadlinePassed,
        Ok(PairingOutcome::ClientGone) => Reply::ClientGone,
        Ok(PairingOutcome::Failed(message)) => Reply::Error { message },
        Err(_) => Reply::Error {
            message: "the pairing ended without a result".to_owned(),
        },
    };
    channel.send(&reply).await
}

mod client;

pub use client::AdminClient;

#[cfg(test)]
mod tests;
