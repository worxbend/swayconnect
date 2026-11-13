//! Seam between the security half and the session code. Signatures frozen by
//! `docs/IMPLEMENTATION_PLAN.md` section 5: private fields may be added, public names and
//! meanings stay.
//!
//! Slice S0 wrote the stubs. Slice S6a owns the file.

use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use swayconnect_protocol::proof::{self, Context, random_bytes};
use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_protocol::{Auth, DeviceId, Pair, RejectCode, Scope};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

use crate::config::{Config, Network, StartError};
use crate::devices::{Device, DeviceInfo, DeviceStore, Secret, StoreError, unix_now};
use crate::identity::Identity;
use crate::pairing::{
    OpenError, OpenedWindow, PairingOutcome, PairingState, PendingNotice, PendingPairing,
    hmac_probe,
};
use crate::paths::{Paths, ensure_private_dir, own_uid};
use crate::throttle::{ConnectionBuckets, ConnectionSlots, LogBudget, Rate};

/// Identity, device store, pairing window and throttle behind one handle. Clones share it.
#[derive(Clone)]
pub struct Security {
    inner: Arc<Inner>,
}

struct Inner {
    identity: Identity,
    allow_from: Vec<Network>,
    /// Key of the HMAC that runs when there is no device or window to check against.
    dummy_key: Secret,
    state: Mutex<State>,
}

/// One lock for all of it: a proof is verified, and a revoke applied, in one step.
struct State {
    devices: DeviceStore,
    pairing: PairingState,
    buckets: ConnectionBuckets,
    slots: ConnectionSlots,
    connection_refusal_log: LogBudget,
    handshake_refusal_log: LogBudget,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        // No code path leaves the state half-changed, so a poisoned lock is still usable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The checks of `docs/DESIGN.md` 4.6 step 1, in their order. `peer` is canonical.
    fn take_slot(
        &self,
        state: &mut State,
        peer: IpAddr,
    ) -> Result<(u64, watch::Receiver<bool>), Refused> {
        let is_allowed = |network: &Network| network.contains(peer);
        if !peer.is_loopback() && !self.allow_from.iter().any(is_allowed) {
            return Err(Refused::NotAllowed);
        }
        if state.slots.is_full() {
            return Err(Refused::Full);
        }
        if state.buckets.take(peer, Instant::now()).is_err() {
            return Err(Refused::RateLimited);
        }
        state.slots.take(peer).ok_or(Refused::Full)
    }
}

/// A total-cap slot and a slot in the set of connections before `welcome`.
pub struct ConnectionPermit {
    inner: Arc<Inner>,
    slot: u64,
    evicted: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotAllowed,
    Full,
    RateLimited,
}

impl Refused {
    fn reason(self) -> &'static str {
        match self {
            Refused::NotAllowed => "source address is not in allow_from",
            Refused::Full => "connection cap reached",
            Refused::RateLimited => "too many connections from this address",
        }
    }
}

pub struct Handshake {
    pub server_nonce: [u8; 32],
    pub peer: SocketAddr,
}

pub struct Admitted {
    pub device_id: DeviceId,
    /// Empty while pending.
    pub scopes: BTreeSet<Scope>,
    pub server_proof: [u8; 32],
    /// `true` or a closed channel: revoked, or pairing refused.
    pub revoked: watch::Receiver<bool>,
    /// `Some` after `pair`.
    pub pending: Option<Pending>,
}

pub struct Pending {
    pub confirm_code: String,
    /// Fires on the owner's yes. Dropping it tells `pairing.rs` the client is gone.
    pub confirmed: oneshot::Receiver<()>,
}

impl Security {
    /// Needs a tokio runtime (`paths::own_uid`).
    pub fn open(config: &Config, paths: &Paths) -> Result<Security, StartError> {
        let uid = own_uid()?;
        ensure_private_dir(&paths.data_dir, uid)?;
        let identity = Identity::open(paths, uid)?;
        let devices = DeviceStore::load(paths.devices_file(), uid)
            .map_err(|error| StartError::Devices(error.to_string()))?;
        let dummy_key = random_bytes().map_err(std::io::Error::other)?;
        let state = State {
            devices,
            pairing: PairingState::default(),
            buckets: ConnectionBuckets::default(),
            slots: ConnectionSlots::default(),
            connection_refusal_log: LogBudget::default(),
            handshake_refusal_log: LogBudget::default(),
        };
        Ok(Security {
            inner: Arc::new(Inner {
                identity,
                allow_from: config.allow_from.clone(),
                dummy_key: Secret::new(dummy_key),
                state: Mutex::new(state),
            }),
        })
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.inner.identity.fingerprint()
    }

    pub fn tls_config(&self) -> Arc<rustls::ServerConfig> {
        self.inner.identity.tls_config()
    }

    /// A refusal is logged here, at most one line per interval: any host can cause it.
    pub fn admit(&self, peer: IpAddr) -> Result<ConnectionPermit, Refused> {
        let peer = peer.to_canonical();
        let mut state = self.inner.state();
        match self.inner.take_slot(&mut state, peer) {
            Ok((slot, evicted)) => Ok(ConnectionPermit {
                inner: Arc::clone(&self.inner),
                slot,
                evicted,
            }),
            Err(refusal) => {
                if let Some(held_back) = state.connection_refusal_log.take(Instant::now()) {
                    let reason = refusal.reason();
                    tracing::info!(%peer, reason, held_back, "connection refused");
                }
                Err(refusal)
            }
        }
    }

    /// `Some(count)`: the caller writes one log line about a refused handshake; `count` such
    /// lines were held back since the last one. `None`: it stays quiet, so that no peer can
    /// fill the log.
    pub fn handshake_refusal_line(&self) -> Option<u64> {
        let mut state = self.inner.state();
        state.handshake_refusal_log.take(Instant::now())
    }

    /// `msg` is already validated by the protocol crate (spec 5.7 step 1).
    /// Needs a tokio runtime: a task holds the pending device until the owner answers.
    pub fn verify_pair(&self, hs: &Handshake, msg: &Pair) -> Result<Admitted, RejectCode> {
        let device_id = random_bytes().map(DeviceId::from_bytes).map_err(|error| {
            tracing::error!(%error, "pairing refused");
            RejectCode::PairingFailed
        })?;
        let context = self.context(hs, msg.nonce);
        let mut state = self.inner.state();
        let window = state
            .pairing
            .claim(&context, &msg.proof, &self.inner.dummy_key)
            .ok_or(RejectCode::PairingFailed)?;
        state.pairing.pending = Some(device_id.clone());
        drop(state);

        let key = Secret::new(proof::device_key(window.secret.bytes(), &context));
        let confirm_code = proof::confirm_code(key.bytes(), &context);
        let server_proof = proof::server_proof(key.bytes(), &context, &device_id);
        let (revoked, revoked_signal) = watch::channel(false);
        let (confirmed, confirmed_signal) = oneshot::channel();
        let (answer, answer_signal) = oneshot::channel();
        let (outcome, outcome_signal) = oneshot::channel();
        tracing::info!(
            device = %escape_for_terminal(&msg.device_name),
            peer = %hs.peer,
            "pairing pending: waiting for the owner's answer"
        );
        let notice = PendingNotice {
            device_name: msg.device_name.clone(),
            platform: msg.platform.clone(),
            client: msg.client.clone(),
            peer: hs.peer,
            confirm_code: confirm_code.clone(),
            answer,
            outcome: outcome_signal,
        };
        // When the admin side just went away the notice is dropped here, and with it `answer`:
        // the task below reads that as a no.
        let _ = window.notice.send(notice);
        let device = Device {
            id: device_id.clone(),
            name: msg.device_name.clone(),
            platform: msg.platform.clone(),
            client: msg.client.clone(),
            key,
            scopes: window.scopes,
            created_at: unix_now(),
        };
        tokio::spawn(self.clone().conclude_pairing(PendingPairing {
            device,
            revoked,
            confirmed,
            answer: answer_signal,
            outcome,
        }));
        Ok(Admitted {
            device_id,
            scopes: BTreeSet::new(),
            server_proof,
            revoked: revoked_signal,
            pending: Some(Pending {
                confirm_code,
                confirmed: confirmed_signal,
            }),
        })
    }

    /// `msg` is already validated by the protocol crate (spec 5.7 step 1).
    pub fn verify_auth(&self, hs: &Handshake, msg: &Auth) -> Result<Admitted, RejectCode> {
        let context = self.context(hs, msg.nonce);
        let mut state = self.inner.state();
        // A pending device is not in the store: it fails like an unknown one (spec 5.4).
        let entry = state.devices.get_mut(&msg.device_id);
        let key = entry
            .as_ref()
            .map_or(&self.inner.dummy_key, |entry| &entry.device.key);
        let proof_matches = hmac_probe::counted(|| {
            proof::verify_client_proof(key.bytes(), &context, &msg.device_id, &msg.proof)
        });
        let entry = entry
            .filter(|_| proof_matches)
            .ok_or(RejectCode::AuthFailed)?;
        entry.last_seen = Some(unix_now());
        entry.last_client.clone_from(&msg.client);
        Ok(Admitted {
            device_id: entry.device.id.clone(),
            scopes: entry.device.scopes.clone(),
            server_proof: proof::server_proof(entry.device.key.bytes(), &context, &msg.device_id),
            revoked: entry.revoked.subscribe(),
            pending: None,
        })
    }

    /// `true` for a stored device and for the device that waits for the owner's answer.
    pub fn is_active(&self, device: &DeviceId) -> bool {
        let state = self.inner.state();
        state.devices.contains(device) || state.pairing.pending.as_ref() == Some(device)
    }

    /// `Err` holds the delay after which the device may retry. An empty bucket is logged
    /// here, at most one line per device and interval: the device decides how often it asks.
    pub fn take_action_token(&self, device: &DeviceId) -> Result<(), Duration> {
        let mut state = self.inner.state();
        let Some(entry) = state.devices.get_mut(device) else {
            // Not a stored device (revoked a moment ago, or pending): it has no bucket.
            return Err(Duration::from_secs(1));
        };
        let now = Instant::now();
        let taken = Rate::ACTIONS.take(&mut entry.actions_full_again_at, now);
        if taken.is_err()
            && let Some(held_back) = entry.rate_limit_log.take(now)
        {
            tracing::info!(
                device = device.as_str(),
                held_back,
                "action refused: rate limit"
            );
        }
        taken
    }

    /// Opens the pairing window (admin socket only). The new device gets `scopes`.
    pub fn open_pairing(
        &self,
        scopes: BTreeSet<Scope>,
        lifetime: Duration,
    ) -> Result<OpenedWindow, OpenError> {
        let secret = random_bytes().map_err(|_| OpenError::Random)?;
        let opened = self.inner.state().pairing.open(secret, scopes, lifetime)?;
        tracing::info!(lifetime_s = lifetime.as_secs(), "pairing window opened");
        Ok(opened)
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.inner.state().devices.list()
    }

    /// Deletes the device and drops its sessions (admin socket only). `Ok(false)`: no such
    /// device. On `Err` the device is gone from memory but still in `devices.json`.
    pub fn revoke(&self, device: &DeviceId) -> Result<bool, StoreError> {
        let outcome = self.inner.state().devices.revoke(device);
        match &outcome {
            Ok(true) => tracing::info!(device = device.as_str(), "device revoked"),
            Ok(false) => {}
            Err(error) => {
                tracing::error!(device = device.as_str(), %error, "device revoked in memory only");
            }
        }
        outcome
    }

    fn context(&self, hs: &Handshake, client_nonce: [u8; 32]) -> Context {
        Context {
            fp: self.fingerprint(),
            server_nonce: hs.server_nonce,
            client_nonce,
        }
    }

    async fn conclude_pairing(self, mut pending: PendingPairing) {
        let verdict = pending.verdict().await;
        let device_id = pending.device.id.clone();
        let outcome =
            self.settle_pairing(verdict, pending.device, pending.revoked, pending.confirmed);
        match &outcome {
            PairingOutcome::Paired { .. } => {
                tracing::info!(device = device_id.as_str(), "pairing confirmed");
            }
            PairingOutcome::Failed(reason) => tracing::error!(%reason, "pairing failed"),
            refusal => tracing::info!(?refusal, "pairing not confirmed"),
        }
        // The admin side may be gone; the log line above is then the only record.
        let _ = pending.outcome.send(outcome);
    }

    /// Stores the device on a yes. In every other case its revoke signal fires or closes.
    fn settle_pairing(
        &self,
        verdict: Result<(), PairingOutcome>,
        device: Device,
        revoked: watch::Sender<bool>,
        confirmed: oneshot::Sender<()>,
    ) -> PairingOutcome {
        let mut state = self.inner.state();
        state.pairing.pending = None;
        if let Err(refusal) = verdict {
            revoked.send_replace(true);
            return refusal;
        }
        if confirmed.is_closed() {
            return PairingOutcome::ClientGone;
        }
        let paired = PairingOutcome::Paired {
            device_id: device.id.clone(),
            device_name: device.name.clone(),
            scopes: device.scopes.clone(),
        };
        match state.devices.insert(device, revoked) {
            Ok(()) => {
                // A client that left in this instant holds no credential; the owner revokes
                // the device (spec 5.4, last paragraph).
                let _ = confirmed.send(());
                paired
            }
            Err(error) => PairingOutcome::Failed(error.to_string()),
        }
    }
}

impl ConnectionPermit {
    /// Resolves when this connection must close to make room before `welcome`.
    pub async fn evicted(&mut self) {
        if self
            .evicted
            .wait_for(|must_close| *must_close)
            .await
            .is_err()
        {
            // The slot left the set before `welcome`: it can no longer be evicted.
            std::future::pending::<()>().await;
        }
    }

    /// Leaves the set of connections before `welcome`.
    pub fn welcomed(&mut self) {
        self.inner.state().slots.welcomed(self.slot);
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.inner.state().slots.release(self.slot);
    }
}

#[cfg(test)]
mod tests;
