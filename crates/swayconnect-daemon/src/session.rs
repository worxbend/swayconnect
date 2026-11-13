//! Handshake and per-connection loop (`docs/DESIGN.md` 4.6, `docs/PROTOCOL.md` 5 to 12).
//!
//! Generic over the stream, so the unit tests run it over an in-memory pipe.

use std::collections::BTreeSet;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use swayconnect_protocol::frame::FrameError;
use swayconnect_protocol::limits::{
    FIRST_FRAME_DEADLINE_MS, IDLE_TIMEOUT_MS, MAX_FRAME_C2S_BYTES, MAX_FRAME_S2C_BYTES,
    PROTOCOL_VERSION,
};
use swayconnect_protocol::ops::{
    EVENT_PAIRING_CONFIRMED, EVENT_STATE_CHANGED, TOPIC_DESKTOP, TopicState,
    encode_subscribe_result,
};
use swayconnect_protocol::proof::random_bytes;
use swayconnect_protocol::{
    Bye, ByeCode, DecodeError, DeviceId, Evt, Hello, IconGetResult, IconHash, JsonObject, Limits,
    Message, ProtocolRange, Reject, RejectCode, Req, Res, Scope, ServerInfo, Welcome,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{oneshot, watch};
use tokio::time::{Instant, sleep_until, timeout};

use self::wire::{FirstFrameError, Unwritable, Wire};
use crate::devices;
use crate::dispatch::{self, Caller, Decision};
use crate::icons::IconService;
use crate::security::{Admitted, ConnectionPermit, Handshake, Security};
use crate::sway_handle::{DesktopInterest, Published, SwayHandle};

mod wire;

const FIRST_FRAME_DEADLINE: Duration = Duration::from_millis(FIRST_FRAME_DEADLINE_MS);
const IDLE_TIMEOUT: Duration = Duration::from_millis(IDLE_TIMEOUT_MS);

/// What every session of one daemon shares. Clones are cheap.
#[derive(Clone)]
pub struct Shared {
    pub security: Security,
    pub desktop: watch::Receiver<Arc<Published>>,
    /// Where the actions go.
    pub sway: Arc<SwayHandle>,
    pub server_name: Arc<str>,
    /// Set at start from the config, or later by the panic switch (`docs/DESIGN.md` 4.7).
    pub read_only: Arc<AtomicBool>,
    pub shutdown: watch::Receiver<bool>,
    /// Answers `icon.get`. `None`: the daemon runs without icon lookup, so it holds no PNG
    /// and every hash is `missing`.
    pub icons: Option<Arc<IconService>>,
}

/// Resolves when the daemon stops: on `true` and when the sender is gone. Cancel safe.
pub async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    // Both outcomes mean stop.
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// Serves one client from `hello` to the close. `permit` is held until the end, because it
/// is this connection's slot in the total cap.
pub async fn serve<S>(stream: S, peer: SocketAddr, mut permit: ConnectionPermit, shared: Shared)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    let Some(admitted) = handshake(&mut wire, peer, &mut permit, &shared).await else {
        return;
    };
    let Admitted {
        device_id,
        scopes,
        revoked,
        pending,
        ..
    } = admitted;
    // `None` for a pending pairing: its scopes stay empty whatever the owner grants.
    let scope_watch = shared.security.scope_watch(&device_id);
    let session = Session {
        wire,
        security: shared.security,
        desktop: shared.desktop,
        sway: shared.sway,
        read_only: shared.read_only,
        shutdown: shared.shutdown,
        icons: shared.icons,
        device_id,
        scopes,
        scope_watch,
        revoked,
        is_unconfirmed_pairing: pending.is_some(),
        confirmed: pending.map(|pending| pending.confirmed),
        desktop_interest: None,
    };
    session.run().await;
}

/// Spec 5.1 up to `welcome`. `None`: the connection is over.
async fn handshake<S>(
    wire: &mut Wire<S>,
    peer: SocketAddr,
    permit: &mut ConnectionPermit,
    shared: &Shared,
) -> Option<Admitted>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let server_nonce = random_bytes()
        .inspect_err(|error| tracing::error!(%error, "no nonce for `hello`"))
        .ok()?;
    let version = ProtocolRange {
        min: PROTOCOL_VERSION,
        max: PROTOCOL_VERSION,
    };
    let hello = Hello {
        protocol: version,
        nonce: server_nonce,
    };
    wire.send(Message::Hello(hello)).await.ok()?;

    let mut shutdown = shared.shutdown.clone();
    // Deadline, eviction and shutdown close with no frame (spec 5.1, 14).
    let first = tokio::select! {
        first = timeout(FIRST_FRAME_DEADLINE, wire.first_frame()) => first.ok()?,
        () = permit.evicted() => return None,
        () = stopped(&mut shutdown) => return None,
    };
    let handshake = Handshake { server_nonce, peer };
    let verdict = match first {
        Ok(frame) => verify(&shared.security, &handshake, &frame),
        Err(FirstFrameError::Refused) => Err(RejectCode::BadMessage),
        Err(FirstFrameError::Gone) => return None,
    };
    let admitted = match verdict {
        Ok(admitted) => admitted,
        Err(code) => {
            // Any peer that is let in can cause this line.
            if let Some(held_back) = shared.security.handshake_refusal_line() {
                tracing::info!(%peer, code = code.as_str(), held_back, "handshake refused");
            }
            wire.send_last(Message::Reject(reject(code))).await;
            return None;
        }
    };
    if admitted.pending.is_none() {
        tracing::info!(%peer, device = admitted.device_id.as_str(), "device authenticated");
    }
    wire.send(Message::Welcome(welcome(&admitted, shared)))
        .await
        .ok()?;
    permit.welcomed();
    wire.decoder.set_limit(MAX_FRAME_C2S_BYTES);
    wire.send_limit = MAX_FRAME_S2C_BYTES;
    Some(admitted)
}

/// Steps 1 to 3 of spec 5.7. The protocol crate has done the member checks of step 1.
fn verify(
    security: &Security,
    handshake: &Handshake,
    frame: &JsonObject,
) -> Result<Admitted, RejectCode> {
    let check_version = |protocol: u32| {
        if protocol == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(RejectCode::UnsupportedVersion)
        }
    };
    match Message::decode(frame) {
        Ok(Message::Pair(pair)) => {
            check_version(pair.protocol)?;
            security.verify_pair(handshake, &pair)
        }
        Ok(Message::Auth(auth)) => {
            check_version(auth.protocol)?;
            security.verify_auth(handshake, &auth)
        }
        Ok(_) | Err(_) => Err(RejectCode::BadMessage),
    }
}

/// `reject` in the words of the transcripts (spec 15.6, 15.7).
fn reject(code: RejectCode) -> Reject {
    let message = match code {
        RejectCode::BadMessage => "the first frame is not a valid pair or auth message".to_owned(),
        RejectCode::UnsupportedVersion => {
            format!("supported protocol versions: {PROTOCOL_VERSION} to {PROTOCOL_VERSION}")
        }
        RejectCode::AuthFailed => "authentication failed".to_owned(),
        RejectCode::PairingFailed => "pairing failed".to_owned(),
    };
    Reject { code, message }
}

fn welcome(admitted: &Admitted, shared: &Shared) -> Welcome {
    // Read once, so `ops` and `read_only` agree.
    let is_read_only = shared.read_only.load(Ordering::Relaxed);
    Welcome {
        protocol: PROTOCOL_VERSION,
        device_id: admitted.device_id.clone(),
        server_proof: admitted.server_proof,
        confirm_code: admitted
            .pending
            .as_ref()
            .map(|pending| pending.confirm_code.clone()),
        scopes: admitted.scopes.iter().copied().collect(),
        ops: dispatch::offered_ops(&admitted.scopes, is_read_only),
        topics: dispatch::offered_topics(&admitted.scopes),
        known_scopes: Some(dispatch::known_scopes()),
        known_ops: Some(dispatch::known_ops()),
        read_only: is_read_only,
        server: ServerInfo {
            name: shared.server_name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        limits: Limits {
            max_frame_c2s: MAX_FRAME_C2S_BYTES,
            max_frame_s2c: MAX_FRAME_S2C_BYTES,
            idle_timeout_ms: IDLE_TIMEOUT_MS,
        },
    }
}

/// Why the loop woke up.
enum Wake {
    Bytes(io::Result<usize>),
    DesktopChanged,
    Revoked,
    ScopesChanged,
    /// The owner's answer arrived; `true` is a yes.
    PairingAnswered(bool),
    Shutdown,
    Idle,
}

/// Whether the connection goes on after one step.
enum Flow {
    Continue,
    /// Send `bye` with this code, then close.
    Bye(ByeCode),
    /// Close without a frame.
    Close,
}

struct Session<S> {
    wire: Wire<S>,
    security: Security,
    desktop: watch::Receiver<Arc<Published>>,
    sway: Arc<SwayHandle>,
    read_only: Arc<AtomicBool>,
    shutdown: watch::Receiver<bool>,
    icons: Option<Arc<IconService>>,
    device_id: DeviceId,
    scopes: BTreeSet<Scope>,
    /// The device's scopes as the owner changes them. `None` for a pending pairing.
    scope_watch: Option<watch::Receiver<BTreeSet<Scope>>>,
    revoked: watch::Receiver<bool>,
    /// The connection began with `pair` and the owner has not said yes.
    is_unconfirmed_pairing: bool,
    /// `Some` while the owner has not answered a pairing. Dropping it tells the security
    /// half that the client is gone.
    confirmed: Option<oneshot::Receiver<()>>,
    /// `Some` once the device subscribed to `desktop`: the supervisor polls while one lives.
    desktop_interest: Option<DesktopInterest>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    async fn run(mut self) {
        let mut idle_deadline = Instant::now() + IDLE_TIMEOUT;
        loop {
            let wake = self.next_wake(idle_deadline).await;
            let flow = match wake {
                Wake::Bytes(Ok(0) | Err(_)) => Flow::Close,
                Wake::Bytes(Ok(_)) => self.answer_frames(&mut idle_deadline).await,
                Wake::DesktopChanged => self.push_desktop().await,
                Wake::Revoked => Flow::Bye(ByeCode::Revoked),
                Wake::ScopesChanged => Flow::Bye(ByeCode::ScopesChanged),
                Wake::PairingAnswered(is_confirmed) => self.end_pending(is_confirmed).await,
                Wake::Shutdown => Flow::Bye(ByeCode::Shutdown),
                Wake::Idle => Flow::Bye(ByeCode::Idle),
            };
            match flow {
                Flow::Continue => {}
                Flow::Bye(code) => {
                    let bye = Message::Bye(self.bye(code));
                    return self.wire.send_last(bye).await;
                }
                Flow::Close => return,
            }
        }
    }

    async fn next_wake(&mut self, idle_deadline: Instant) -> Wake {
        let pairing_answer = async {
            match &mut self.confirmed {
                Some(confirmed) => confirmed.await.is_ok(),
                None => std::future::pending().await,
            }
        };
        let (scope_watch, held) = (&mut self.scope_watch, &self.scopes);
        // A grant before the watch was taken resolves at once: the value differs from
        // `held`. After a revoke it never resolves; the revoke signal ends the session.
        let scopes_changed = async {
            match scope_watch {
                Some(scope_watch) => devices::scopes_changed(scope_watch, held).await,
                None => std::future::pending().await,
            }
        };
        let is_subscribed = self.desktop_interest.is_some();
        tokio::select! {
            read = self.wire.fill() => Wake::Bytes(read),
            changed = self.desktop.changed(), if is_subscribed => match changed {
                Ok(()) => Wake::DesktopChanged,
                // The supervisor only ends when the daemon stops.
                Err(_) => Wake::Shutdown,
            },
            // `true` and a closed channel both mean revoked.
            _ = self.revoked.wait_for(|is_revoked| *is_revoked) => Wake::Revoked,
            () = scopes_changed => Wake::ScopesChanged,
            is_confirmed = pairing_answer => Wake::PairingAnswered(is_confirmed),
            () = stopped(&mut self.shutdown) => Wake::Shutdown,
            () = sleep_until(idle_deadline) => Wake::Idle,
        }
    }

    /// Answers every complete frame that has arrived, in order.
    async fn answer_frames(&mut self, idle_deadline: &mut Instant) -> Flow {
        loop {
            let frame = match self.wire.decoder.next_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => return Flow::Continue,
                Err(FrameError::Bad) => return Flow::Bye(ByeCode::BadFrame),
                Err(FrameError::TooLarge) => return Flow::Bye(ByeCode::TooLarge),
            };
            *idle_deadline = Instant::now() + IDLE_TIMEOUT;
            let flow = match Message::decode(&frame) {
                Ok(Message::Req(request)) => self.answer(&request).await,
                Err(DecodeError::UnknownType) => Flow::Continue,
                Ok(_) | Err(_) => Flow::Bye(ByeCode::ProtocolError),
            };
            if !matches!(flow, Flow::Continue) {
                return flow;
            }
        }
    }

    async fn answer(&mut self, request: &Req) -> Flow {
        let caller = Caller {
            security: &self.security,
            device: &self.device_id,
            scopes: &self.scopes,
            is_read_only: self.read_only.load(Ordering::Relaxed),
        };
        let response = match dispatch::decide(&caller, request) {
            Decision::Revoked => return Flow::Bye(ByeCode::Revoked),
            Decision::Refuse(error) => Res::failure(request.id, error),
            Decision::Pong => Res::success(request.id, JsonObject::new()),
            Decision::SubscribeDesktop => {
                let sway = &self.sway;
                self.desktop_interest
                    .get_or_insert_with(|| sway.watch_desktop());
                let states = [self.desktop_state()];
                Res::success(request.id, encode_subscribe_result(&states))
            }
            Decision::Icons(hashes) => Res::success(request.id, self.icons(hashes).encode()),
            Decision::Act(action) => match self.sway.act(action).await {
                Ok(()) => Res::success(request.id, JsonObject::new()),
                Err(error) => Res::failure(request.id, dispatch::action_refusal(action, error)),
            },
        };
        self.send(Message::Res(response)).await
    }

    /// The `icon.get` result. Never waits for the icon worker.
    fn icons(&self, hashes: Vec<IconHash>) -> IconGetResult {
        match &self.icons {
            Some(service) => service.icons(&hashes),
            None => IconGetResult {
                icons: Vec::new(),
                missing: hashes,
            },
        }
    }

    async fn push_desktop(&mut self) -> Flow {
        let event = Evt {
            event: EVENT_STATE_CHANGED.to_owned(),
            data: self.desktop_state().encode(),
        };
        self.send(Message::Evt(event)).await
    }

    async fn end_pending(&mut self, is_confirmed: bool) -> Flow {
        self.confirmed = None;
        if !is_confirmed {
            // Refused, deadline or admin gone: the revoke signal fires next.
            return Flow::Continue;
        }
        self.is_unconfirmed_pairing = false;
        let event = Evt {
            event: EVENT_PAIRING_CONFIRMED.to_owned(),
            data: JsonObject::new(),
        };
        self.send(Message::Evt(event)).await
    }

    async fn send(&mut self, message: Message) -> Flow {
        match self.wire.send(message).await {
            Ok(()) => Flow::Continue,
            Err(Unwritable) => Flow::Close,
        }
    }

    /// `bye` in the words of the transcripts (spec 15.5, 15.9).
    fn bye(&self, code: ByeCode) -> Bye {
        let message = match code {
            ByeCode::Shutdown => "daemon is shutting down",
            ByeCode::Revoked if self.is_unconfirmed_pairing => "pairing was not confirmed",
            ByeCode::Revoked => "this device is not paired",
            ByeCode::Idle => "no frame within the idle timeout",
            ByeCode::BadFrame => "bad frame",
            ByeCode::TooLarge => "frame too large",
            ByeCode::ProtocolError => "only req frames are allowed after welcome",
            ByeCode::ScopesChanged => "scopes of this device changed",
        };
        Bye {
            code,
            message: message.to_owned(),
        }
    }

    /// The newest snapshot as this device may see it, marked as seen.
    fn desktop_state(&mut self) -> TopicState {
        let published = Arc::clone(&self.desktop.borrow_and_update());
        let data = if self.scopes.contains(&Scope::StateTitles) {
            published.desktop.encode()
        } else {
            let mut desktop = published.desktop.clone();
            for window in &mut desktop.windows {
                window.title = None;
            }
            for window in desktop.scratchpad.iter_mut().flatten() {
                window.title = None;
            }
            desktop.encode()
        };
        TopicState {
            topic: TOPIC_DESKTOP.to_owned(),
            rev: published.rev,
            data,
        }
    }
}

#[cfg(test)]
mod ext_tests;
#[cfg(test)]
mod tests;
