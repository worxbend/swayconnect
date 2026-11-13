//! The client side of the admin socket, for `swayconnectd pair`, `devices` and `revoke`.

use std::collections::BTreeSet;
use std::io;

use swayconnect_protocol::{DeviceId, Scope};
use tokio::net::UnixStream;

use super::{
    AdminError, Answer, ChangedScopes, Channel, PairingRequest, PairingResult, PendingDevice,
    Reply, Request, check_peer_uid, check_runtime_dir,
};
use crate::devices::{DeviceInfo, ScopeChange};
use crate::paths::{Kind, PathError, Paths, check_private, own_uid};

/// The client side, for `swayconnectd pair`, `devices` and `revoke`.
pub struct AdminClient {
    pub(super) channel: Channel,
}

impl AdminClient {
    /// Applies the directory checks of the server and checks the server's uid, so a planted
    /// socket cannot hand out a pairing URI for another server.
    pub async fn connect(paths: &Paths) -> Result<AdminClient, AdminError> {
        let uid = own_uid()?;
        check_runtime_dir(paths, uid)?;
        let socket = paths.admin_socket();
        let not_running = |error: &io::Error| {
            let kind = error.kind();
            kind == io::ErrorKind::NotFound || kind == io::ErrorKind::ConnectionRefused
        };
        match check_private(&paths.admin_dir(), Kind::Directory, uid) {
            Err(PathError::Io { source, .. }) if not_running(&source) => {
                return Err(AdminError::NotRunning(socket));
            }
            checked => checked?,
        }
        let stream = match UnixStream::connect(&socket).await {
            Err(error) if not_running(&error) => return Err(AdminError::NotRunning(socket)),
            // Names the path: the usual cause is a runtime directory too long for a socket.
            connected => connected.map_err(PathError::io(&socket))?,
        };
        check_peer_uid(stream.peer_cred()?.uid(), uid)?;
        Ok(AdminClient {
            channel: Channel::new(stream),
        })
    }

    pub async fn devices(&mut self) -> Result<Vec<DeviceInfo>, AdminError> {
        self.channel.send(&Request::DevicesList).await?;
        match self.reply().await? {
            Reply::Devices { devices } => Ok(devices),
            _ => Err(AdminError::Protocol),
        }
    }

    /// `Ok(false)`: no such device.
    pub async fn revoke(&mut self, device: &DeviceId) -> Result<bool, AdminError> {
        let device_id = device.as_str().to_owned();
        self.channel
            .send(&Request::DevicesRevoke { device_id })
            .await?;
        match self.reply().await? {
            Reply::Revoked { found } => Ok(found),
            _ => Err(AdminError::Protocol),
        }
    }

    /// Grants or denies `scopes` to the device whose id starts with `prefix`. The daemon ends
    /// the device's live sessions with `bye` `scopes_changed` when the set changed.
    pub async fn change_scopes(
        &mut self,
        prefix: &str,
        change: ScopeChange,
        scopes: &BTreeSet<Scope>,
    ) -> Result<ChangedScopes, AdminError> {
        let prefix = prefix.to_owned();
        let scopes = scopes
            .iter()
            .map(|scope| scope.as_str().to_owned())
            .collect();
        let request = match change {
            ScopeChange::Grant => Request::DevicesGrant { prefix, scopes },
            ScopeChange::Deny => Request::DevicesDeny { prefix, scopes },
        };
        self.channel.send(&request).await?;
        match self.reply().await? {
            Reply::Scopes(changed) => Ok(changed),
            _ => Err(AdminError::Protocol),
        }
    }

    /// Opens the pairing window and returns the pairing URI. The URI holds the pairing
    /// secret: show it to the owner, never log it. Closing this client cancels the pairing.
    pub async fn open_pairing(&mut self, request: &PairingRequest) -> Result<String, AdminError> {
        let scopes = request.scopes.iter().map(|scope| scope.as_str().to_owned());
        let request = Request::PairOpen {
            scopes: scopes.collect(),
            lifetime_s: request.lifetime.as_secs(),
            host: request.host.clone(),
        };
        self.channel.send(&request).await?;
        match self.reply().await? {
            Reply::Window { uri } => Ok(uri),
            _ => Err(AdminError::Protocol),
        }
    }

    /// Waits until a device presents the secret. `None`: the window expired.
    pub async fn pending_device(&mut self) -> Result<Option<PendingDevice>, AdminError> {
        match self.reply().await? {
            Reply::Pending(device) => Ok(Some(device)),
            Reply::WindowExpired => Ok(None),
            _ => Err(AdminError::Protocol),
        }
    }

    /// Sends the owner's yes or no for the pending device.
    pub async fn answer(&mut self, confirm: bool) -> Result<(), AdminError> {
        self.channel.send(&Answer { confirm }).await
    }

    /// The end of the pairing. It can arrive before any answer (deadline, device gone), so
    /// wait for it together with the owner's input. Cancel safe.
    pub async fn pairing_result(&mut self) -> Result<PairingResult, AdminError> {
        match self.reply().await? {
            Reply::Paired(device) => Ok(PairingResult::Paired(device)),
            Reply::Refused => Ok(PairingResult::Refused),
            Reply::DeadlinePassed => Ok(PairingResult::DeadlinePassed),
            Reply::ClientGone => Ok(PairingResult::ClientGone),
            _ => Err(AdminError::Protocol),
        }
    }

    pub(super) async fn reply(&mut self) -> Result<Reply, AdminError> {
        match self.channel.receive().await? {
            Some(Reply::Error { message }) => Err(AdminError::Daemon(message)),
            Some(reply) => Ok(reply),
            None => Err(AdminError::Closed),
        }
    }
}
