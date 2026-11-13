//! Pairing window, pending device, confirmation (`docs/PROTOCOL.md` 5.4).
//! Written by slice S6a.
//!
//! The window lives in `Security`'s state. A pending device lives in one task
//! (`Security::verify_pair` spawns it) until the owner answers, the deadline passes or the
//! client goes away. Only a confirmed device reaches the device store.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use swayconnect_protocol::limits::PAIRING_CONFIRMATION_DEADLINE_MS;
use swayconnect_protocol::proof::{Context, verify_pair_proof};
use swayconnect_protocol::{DeviceId, Scope};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

use crate::devices::{Device, Secret};

const CONFIRMATION_DEADLINE: Duration = Duration::from_millis(PAIRING_CONFIRMATION_DEADLINE_MS);

/// What `Security::open_pairing` hands to the admin side.
#[derive(Debug)]
pub struct OpenedWindow {
    /// `S`, for the pairing URI. Never log it.
    pub secret: Secret,
    pub expires_at: Instant,
    /// Resolves when a device presented the secret. Dropping it cancels the window.
    pub pending: oneshot::Receiver<PendingNotice>,
}

/// A device that proved knowledge of the secret and waits for the owner's answer.
/// The strings come from the device: print them through `escape_for_terminal`.
#[derive(Debug)]
pub struct PendingNotice {
    pub device_name: String,
    pub platform: Option<String>,
    pub client: Option<String>,
    pub peer: SocketAddr,
    pub confirm_code: String,
    /// The owner's yes or no. Dropping it refuses the device.
    pub answer: oneshot::Sender<bool>,
    pub outcome: oneshot::Receiver<PairingOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingOutcome {
    Paired {
        device_id: DeviceId,
        device_name: String,
        scopes: BTreeSet<Scope>,
    },
    Refused,
    DeadlinePassed,
    ClientGone,
    /// The device could not be stored.
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("a pairing is already in progress")]
    Busy,
    #[error("the system random source failed")]
    Random,
}

#[derive(Debug)]
struct Window {
    secret: Secret,
    scopes: BTreeSet<Scope>,
    expires_at: Instant,
    notice: oneshot::Sender<PendingNotice>,
}

impl Window {
    fn is_open(&self, now: Instant) -> bool {
        now < self.expires_at && !self.notice.is_closed()
    }
}

/// A window that a correct proof used up.
#[derive(Debug)]
pub struct ClaimedWindow {
    pub secret: Secret,
    pub scopes: BTreeSet<Scope>,
    pub notice: oneshot::Sender<PendingNotice>,
}

#[derive(Debug, Default)]
pub struct PairingState {
    window: Option<Window>,
    /// The device that waits for the owner's answer.
    pub pending: Option<DeviceId>,
}

impl PairingState {
    pub fn open(
        &mut self,
        secret: [u8; 32],
        scopes: BTreeSet<Scope>,
        lifetime: Duration,
    ) -> Result<OpenedWindow, OpenError> {
        let now = Instant::now();
        let window_in_use = self
            .window
            .as_ref()
            .is_some_and(|window| window.is_open(now));
        if window_in_use || self.pending.is_some() {
            return Err(OpenError::Busy);
        }
        let expires_at = now + lifetime;
        let (notice, pending) = oneshot::channel();
        self.window = Some(Window {
            secret: Secret::new(secret),
            scopes,
            expires_at,
            notice,
        });
        Ok(OpenedWindow {
            secret: Secret::new(secret),
            expires_at,
            pending,
        })
    }

    /// Checks `pair.proof` and uses the window up when it is correct. A wrong proof leaves the
    /// window open. Exactly one HMAC runs in every case, with `dummy` when no window is open
    /// (spec 5.4 step 4).
    pub fn claim(
        &mut self,
        context: &Context,
        proof: &[u8; 32],
        dummy: &Secret,
    ) -> Option<ClaimedWindow> {
        let now = Instant::now();
        let open_window = self.window.as_ref().filter(|window| window.is_open(now));
        let secret = open_window.map_or(dummy, |window| &window.secret);
        let proof_matches =
            hmac_probe::counted(|| verify_pair_proof(secret.bytes(), context, proof));
        if open_window.is_none() || !proof_matches {
            return None;
        }
        let window = self.window.take()?;
        Some(ClaimedWindow {
            secret: window.secret,
            scopes: window.scopes,
            notice: window.notice,
        })
    }
}

/// A pending device and the channels of everyone who waits for its fate.
#[derive(Debug)]
pub struct PendingPairing {
    pub device: Device,
    pub revoked: watch::Sender<bool>,
    pub confirmed: oneshot::Sender<()>,
    pub answer: oneshot::Receiver<bool>,
    pub outcome: oneshot::Sender<PairingOutcome>,
}

impl PendingPairing {
    /// Waits for the owner. `Err` holds why the device is not to be stored.
    pub async fn verdict(&mut self) -> Result<(), PairingOutcome> {
        tokio::select! {
            answer = &mut self.answer => match answer {
                Ok(true) => Ok(()),
                // A dropped sender is the admin connection going away: that is a no.
                Ok(false) | Err(_) => Err(PairingOutcome::Refused),
            },
            () = tokio::time::sleep(CONFIRMATION_DEADLINE) => Err(PairingOutcome::DeadlinePassed),
            () = self.confirmed.closed() => Err(PairingOutcome::ClientGone),
        }
    }
}

/// Lets tests count HMAC computations of the proof checks. Outside tests it only calls through.
pub(crate) mod hmac_probe {
    #[cfg(test)]
    thread_local! {
        static COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub fn counted(verify: impl FnOnce() -> bool) -> bool {
        #[cfg(test)]
        COUNT.with(|count| count.set(count.get() + 1));
        verify()
    }

    /// The number of proof checks on this thread since the last call.
    #[cfg(test)]
    pub fn take_count() -> usize {
        COUNT.with(|count| count.replace(0))
    }
}

#[cfg(test)]
mod tests {
    use swayconnect_protocol::proof::pair_proof;

    use super::*;

    const SECRET: [u8; 32] = [7; 32];
    const LIFETIME: Duration = Duration::from_secs(300);

    fn context() -> Context {
        Context {
            fp: [1; 32],
            server_nonce: [2; 32],
            client_nonce: [3; 32],
        }
    }

    fn scopes() -> BTreeSet<Scope> {
        BTreeSet::from([Scope::StateRead])
    }

    fn claim(state: &mut PairingState, secret: &[u8; 32]) -> Option<ClaimedWindow> {
        let proof = pair_proof(secret, &context());
        state.claim(&context(), &proof, &Secret::new([9; 32]))
    }

    #[tokio::test(start_paused = true)]
    async fn a_bad_proof_leaves_the_window_open_and_a_correct_one_uses_it_up() {
        let mut state = PairingState::default();
        let _opened = state.open(SECRET, scopes(), LIFETIME).expect("open");
        hmac_probe::take_count();

        assert!(claim(&mut state, &[8; 32]).is_none());
        let claimed = claim(&mut state, &SECRET).expect("correct proof");
        assert_eq!(claimed.scopes, scopes());
        assert_eq!(claimed.secret.bytes(), &SECRET);
        assert!(claim(&mut state, &SECRET).is_none(), "single use");
        assert_eq!(hmac_probe::take_count(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_window_one_hmac_is_still_computed() {
        let mut state = PairingState::default();
        hmac_probe::take_count();
        assert!(claim(&mut state, &SECRET).is_none());
        assert_eq!(hmac_probe::take_count(), 1);
        // The dummy key itself opens nothing.
        assert!(claim(&mut state, &[9; 32]).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn an_expired_or_cancelled_window_fails_and_can_be_replaced() {
        let mut state = PairingState::default();
        let opened = state.open(SECRET, scopes(), LIFETIME).expect("open");
        assert_eq!(
            state.open(SECRET, scopes(), LIFETIME).err(),
            Some(OpenError::Busy)
        );
        tokio::time::advance(LIFETIME - Duration::from_millis(1)).await;
        assert_eq!(opened.expires_at - Instant::now(), Duration::from_millis(1));
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(claim(&mut state, &SECRET).is_none(), "expired");

        let opened = state.open(SECRET, scopes(), LIFETIME).expect("open again");
        drop(opened);
        assert!(
            claim(&mut state, &SECRET).is_none(),
            "cancelled by the admin side"
        );
        let _opened = state
            .open(SECRET, scopes(), LIFETIME)
            .expect("open a third time");
        assert!(claim(&mut state, &SECRET).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_pending_device_blocks_a_new_window() {
        let mut state = PairingState {
            pending: Some(DeviceId::from_bytes([1; 16])),
            ..PairingState::default()
        };
        assert_eq!(
            state.open(SECRET, scopes(), LIFETIME).err(),
            Some(OpenError::Busy)
        );
    }

    #[test]
    fn debug_output_shows_no_secret() {
        let mut state = PairingState::default();
        let opened = state.open(SECRET, scopes(), LIFETIME).expect("open");
        let text = format!("{state:?} {opened:?}");
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("7, 7"));
    }
}
