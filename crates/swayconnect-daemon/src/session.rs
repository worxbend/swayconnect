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
use swayconnect_protocol::ops::{EVENT_PAIRING_CONFIRMED, encode_subscribe_result};
use swayconnect_protocol::proof::random_bytes;
use swayconnect_protocol::{
    Bye, ByeCode, DecodeError, DeviceId, Evt, Hello, IconGetResult, IconHash, JsonObject, Limits,
    Message, ProtocolRange, Reject, RejectCode, Req, Res, Scope, ServerInfo, Welcome,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{oneshot, watch};
use tokio::time::{Instant, sleep_until, timeout};

use self::feeds::{Feeds, Sources};
use self::wire::{FirstFrameError, Unwritable, Wire};
use crate::audit::AuditLog;
use crate::daemon::services::Services;
use crate::devices;
use crate::dispatch::{self, Caller, Decision, Features, Topic};
use crate::icons::IconService;
use crate::input::InputConnection;
use crate::keep_awake::KeepAwake;
use crate::notify;
use crate::security::{Admitted, ConnectionPermit, Handshake, Security};
use crate::session_topic::{Locker, SessionTopic};
use crate::sway_handle::{Published, SwayHandle};

mod answers;
mod feeds;
mod services;
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
    pub controls: Controls,
    /// Audio, media, apps, the launcher and the remote keyboard (M2b).
    pub services: Services,
}

/// The M2a services every session shares (`docs/DESIGN.md` 11.6, 11.8). Clones are cheap.
#[derive(Clone)]
pub struct Controls {
    pub keep_awake: KeepAwake,
    pub session_topic: Arc<SessionTopic>,
    /// `None` without `[session] lock`: `session.lock` is not offered (spec 17.6 rule 1).
    pub locker: Option<Arc<Locker>>,
    /// The daemon's one audit log.
    pub audit: AuditLog,
}

impl Controls {
    pub fn features(&self) -> Features {
        Features {
            lock: self.locker.is_some(),
        }
    }
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
    let feeds = Feeds::new(Sources {
        desktop: shared.desktop.clone(),
        sway: Arc::clone(&shared.sway),
        session_topic: Arc::clone(&shared.controls.session_topic),
        services: shared.services.clone(),
    });
    let session = Session {
        wire,
        peer,
        security: shared.security,
        desktop: shared.desktop,
        sway: shared.sway,
        read_only: shared.read_only,
        shutdown: shared.shutdown,
        icons: shared.icons,
        controls: shared.controls,
        input: shared.services.input.connection(device_id.clone()),
        services: shared.services,
        device_id,
        scopes,
        scope_watch,
        revoked,
        is_unconfirmed_pairing: pending.is_some(),
        confirmed: pending.map(|pending| pending.confirmed),
        feeds,
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
    // In read-only mode no tier 2 op is offered, so the connect is no risk to announce.
    let can_act = !shared.read_only.load(Ordering::SeqCst);
    if admitted.pending.is_none() && can_act && notify::holds_tier_2(&admitted.scopes) {
        let device = admitted.device_id.clone();
        shared.services.notifier.announce_connect(device);
    }
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
    let features = shared.controls.features();
    Welcome {
        protocol: PROTOCOL_VERSION,
        device_id: admitted.device_id.clone(),
        server_proof: admitted.server_proof,
        confirm_code: admitted
            .pending
            .as_ref()
            .map(|pending| pending.confirm_code.clone()),
        scopes: admitted.scopes.iter().copied().collect(),
        ops: dispatch::offered_ops(&admitted.scopes, is_read_only, features),
        topics: dispatch::offered_topics(&admitted.scopes),
        known_scopes: Some(dispatch::known_scopes()),
        known_ops: Some(dispatch::known_ops(features)),
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
    TopicChanged(Topic),
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
    peer: SocketAddr,
    security: Security,
    desktop: watch::Receiver<Arc<Published>>,
    sway: Arc<SwayHandle>,
    read_only: Arc<AtomicBool>,
    shutdown: watch::Receiver<bool>,
    icons: Option<Arc<IconService>>,
    controls: Controls,
    services: Services,
    /// This connection's remote keyboard. Dropping it ends its input session.
    input: InputConnection,
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
    /// The topics the device subscribed to.
    feeds: Feeds,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    async fn run(mut self) {
        let mut idle_deadline = Instant::now() + IDLE_TIMEOUT;
        loop {
            let wake = self.next_wake(idle_deadline).await;
            let flow = match wake {
                Wake::Bytes(Ok(0) | Err(_)) => Flow::Close,
                Wake::Bytes(Ok(_)) => self.answer_frames(&mut idle_deadline).await,
                Wake::TopicChanged(topic) => self.push(topic).await,
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
        tokio::select! {
            read = self.wire.fill() => Wake::Bytes(read),
            changed = self.feeds.changed() => match changed {
                Ok(topic) => Wake::TopicChanged(topic),
                // A topic's source only ends when the daemon stops.
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
            features: self.controls.features(),
        };
        let response = match dispatch::decide(&caller, request) {
            Decision::Revoked => return Flow::Bye(ByeCode::Revoked),
            Decision::Refuse(error) => Res::failure(request.id, error),
            Decision::Pong => Res::success(request.id, JsonObject::new()),
            Decision::Subscribe(topics) => {
                let states = self.subscribe(&topics);
                Res::success(request.id, encode_subscribe_result(&states))
            }
            Decision::Icons(hashes) => Res::success(request.id, self.icons(hashes).encode()),
            Decision::Act(action) => self.act(request.id, action).await,
            Decision::KeepAwake(args) => self.keep_awake(request.id, args).await,
            Decision::Lock => self.lock(request.id).await,
            Decision::Audio { device_id, change } => {
                self.audio(request.id, device_id, change).await
            }
            Decision::Media { player_id, command } => {
                self.media(request.id, player_id, command).await
            }
            Decision::Launch { entry_id, number } => {
                self.launch(request.id, &entry_id, number).await
            }
            Decision::OpenLauncher => self.open_launcher(request.id).await,
            Decision::Type { target, text } => self.type_text(request.id, target, &text).await,
            Decision::Key { target, press } => self.press_key(request.id, target, &press).await,
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
}

#[cfg(test)]
mod ext_tests;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod m2a_tests;
#[cfg(test)]
mod m2b_tests;
#[cfg(test)]
mod tests;
