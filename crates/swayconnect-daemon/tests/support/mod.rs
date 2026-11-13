//! A daemon in this process with the fake sway, and the two ways a test talks to it: the
//! client library and the admin socket (`docs/DESIGN.md` 9.2, 9.3).

#![allow(
    dead_code,
    reason = "every test binary compiles this module and uses a part of it"
)]

pub mod logs;
pub mod vectors;
pub mod wire;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use swayconnect_cli::client::{ClientError, Connector, Event, PendingPairing, Profile, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest, PairingResult};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle};
use swayconnect_daemon::paths::Paths;
use swayconnect_protocol::ops::TOPIC_DESKTOP;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{ByeCode, Desktop, ErrorBody, JsonObject, RejectCode, Scope};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{FakeSway, TestDirs};

pub const PATIENCE: Duration = Duration::from_secs(20);
pub const EVERY_SCOPE: [Scope; 3] = [Scope::StateRead, Scope::StateTitles, Scope::Control];
pub const DEVICE_NAME: &str = "test phone";
const WINDOW_LIFETIME: Duration = Duration::from_secs(300);

/// Fails the test instead of hanging it. Not for a paused clock, which would jump here.
pub async fn soon<T>(what: &str, outcome: impl Future<Output = T>) -> T {
    match tokio::time::timeout(PATIENCE, outcome).await {
        Ok(outcome) => outcome,
        Err(_elapsed) => panic!("timed out: {what}"),
    }
}

pub fn connector() -> Connector {
    Connector::new(Some("integration-test".to_owned()))
}

pub struct Harness {
    pub paths: Paths,
    pub daemon: DaemonHandle,
    sway: Option<FakeSway>,
    // Last, so that the directories are removed after the daemon and the fake sway are gone.
    pub dirs: TestDirs,
}

impl Harness {
    pub async fn start() -> Harness {
        Harness::start_with("").await
    }

    /// `extra_config` is TOML that follows the `listen` line.
    pub async fn start_with(extra_config: &str) -> Harness {
        Harness::start_prepared(extra_config, |_default_replies| {}).await
    }

    /// `prepare` scripts the fake sway before the daemon first connects to it.
    pub async fn start_prepared(extra_config: &str, prepare: impl FnOnce(&FakeSway)) -> Harness {
        let dirs = TestDirs::new();
        let sway = start_sway(&dirs);
        prepare(&sway);
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = format!("listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\n{extra_config}");
        let config = Config::from_toml(&config).expect("config");
        let target = SwayTarget::Explicit(dirs.sway_socket());
        let daemon = Daemon::start(config, paths.clone(), target)
            .await
            .expect("daemon starts");
        Harness {
            paths,
            daemon,
            sway: Some(sway),
            dirs,
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.daemon.bound()[0]
    }

    pub fn sway(&self) -> &FakeSway {
        self.sway.as_ref().expect("the fake sway runs")
    }

    /// Sway goes away: its connections close and its socket file is removed.
    pub fn stop_sway(&mut self) {
        self.sway = None;
    }

    pub fn restart_sway(&mut self) {
        self.sway = Some(start_sway(&self.dirs));
    }

    pub async fn admin(&self) -> AdminClient {
        soon("admin connection", AdminClient::connect(&self.paths))
            .await
            .expect("admin socket")
    }

    /// Opens a pairing window. Dropping the admin client cancels it.
    pub async fn open_window(&self, scopes: &[Scope], lifetime: Duration) -> Pairing {
        let mut admin = self.admin().await;
        let request = PairingRequest {
            scopes: BTreeSet::from_iter(scopes.iter().copied()),
            lifetime,
            host: None,
        };
        let uri = soon("pairing window", admin.open_pairing(&request))
            .await
            .expect("window opened");
        let uri = PairingUri::parse(&uri).expect("a valid pairing URI");
        Pairing { admin, uri }
    }

    /// The whole pairing flow with the owner's yes. The device gets `scopes`.
    pub async fn pair(&self, scopes: &[Scope]) -> Profile {
        let mut pairing = self.open_window(scopes, WINDOW_LIFETIME).await;
        let pending = pairing.present_secret().await;
        pairing.answer(true).await;
        let profile = soon("pairing.confirmed", pending.confirmed())
            .await
            .expect("pairing confirmed");
        assert!(matches!(
            pairing.result().await,
            PairingResult::Paired(device) if device.device_id == profile.device_id.as_str()
        ));
        profile
    }

    pub async fn connect(&self, profile: &Profile) -> Session {
        soon("auth", connector().connect(profile))
            .await
            .expect("authenticated")
    }

    pub async fn revoke(&self, profile: &Profile) {
        let found = soon("revoke", self.admin().await.revoke(&profile.device_id)).await;
        assert!(found.expect("revoke answered"), "the device was stored");
    }
}

/// Where the owner's real sway listens, as far as the environment of the test run says.
/// The harness reads the environment, so the fake sway can refuse these paths.
pub fn real_sway_sockets() -> Vec<PathBuf> {
    sockets_named_by(|variable| std::env::var_os(variable))
}

/// Both variables a sway or i3 client reads; `lookup` stands for the environment.
pub fn sockets_named_by(lookup: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    ["SWAYSOCK", "I3SOCK"]
        .into_iter()
        .filter_map(lookup)
        .map(PathBuf::from)
        .collect()
}

fn start_sway(dirs: &TestDirs) -> FakeSway {
    FakeSway::start(&dirs.runtime_dir(), &real_sway_sockets())
}

/// An open pairing window, seen from the owner's side.
pub struct Pairing {
    pub admin: AdminClient,
    pub uri: PairingUri,
}

impl Pairing {
    /// A client pairs with the URI. Checks that both sides show the same code.
    pub async fn present_secret(&mut self) -> PendingPairing {
        let connector = connector();
        let pairing = connector.pair(&self.uri, DEVICE_NAME, Some("linux"));
        let pending = soon("pair", pairing).await.expect("pending pairing");
        let shown = soon("pending device", self.admin.pending_device())
            .await
            .expect("admin answer")
            .expect("a pending device, not an expired window");
        assert_eq!(shown.device_name, DEVICE_NAME);
        assert_eq!(shown.confirm_code, pending.confirm_code());
        pending
    }

    pub async fn answer(&mut self, confirm: bool) {
        soon("answer", self.admin.answer(confirm))
            .await
            .expect("answer sent");
    }

    pub async fn result(&mut self) -> PairingResult {
        soon("pairing result", self.admin.pairing_result())
            .await
            .expect("pairing result")
    }
}

/// Subscribes to `desktop` and returns the first state in which sway is online, with its
/// `rev`. The daemon may still be connecting to sway when the test gets here.
pub async fn online_desktop(session: &mut Session) -> (u64, Desktop) {
    subscribed_desktop(session, |desktop| desktop.sway.online).await
}

/// Subscribes to `desktop` and returns the first state that satisfies `wanted`.
pub async fn subscribed_desktop(
    session: &mut Session,
    wanted: impl Fn(&Desktop) -> bool,
) -> (u64, Desktop) {
    let states = soon("state.subscribe", session.subscribe(&[TOPIC_DESKTOP]))
        .await
        .expect("subscribed");
    let [state] = states.as_slice() else {
        panic!("one state, got {}", states.len());
    };
    let first = (state.rev, Desktop::decode(&state.data).expect("desktop"));
    if wanted(&first.1) {
        return first;
    }
    next_desktop(session, wanted).await
}

/// The next `state.changed` of `desktop` that satisfies `wanted`.
pub async fn next_desktop(
    session: &mut Session,
    wanted: impl Fn(&Desktop) -> bool,
) -> (u64, Desktop) {
    loop {
        let event = soon("state.changed", session.next_event())
            .await
            .expect("an event");
        let Event::State(state) = event else {
            panic!("not a state event: {event:?}");
        };
        let desktop = Desktop::decode(&state.data).expect("desktop");
        if wanted(&desktop) {
            return (state.rev, desktop);
        }
    }
}

/// Sends `payload` as one unchecked frame and returns the next frame from the server.
pub async fn raw_exchange(session: &mut Session, payload: &[u8]) -> JsonObject {
    let sent = soon("raw frame", session.send_raw_frame(payload)).await;
    sent.expect("frame sent");
    let answer = soon("answer", session.receive_raw_frame()).await;
    answer.expect("an answer frame")
}

/// One `req` frame payload, for `raw_exchange` and `Session::send_raw_frame`.
pub fn request_frame(id: u64, op: &str, args: &serde_json::Value) -> Vec<u8> {
    let request = serde_json::json!({"type": "req", "id": id, "op": op, "args": args});
    request.to_string().into_bytes()
}

/// The `res.error` of a request that the server refused.
pub fn refusal(outcome: Result<JsonObject, ClientError>) -> ErrorBody {
    match outcome {
        Err(ClientError::Failed(error)) => error,
        other => panic!("expected an error response, got {other:?}"),
    }
}

pub fn assert_bye(outcome: Result<impl Sized, ClientError>, expected: ByeCode) {
    match outcome.err() {
        Some(ClientError::Bye { code, .. }) => assert_eq!(code, expected),
        other => panic!("expected `bye`, got {other:?}"),
    }
}

pub fn assert_rejected(outcome: Result<impl Sized, ClientError>, expected: RejectCode) {
    match outcome.err() {
        Some(ClientError::Rejected { code, .. }) => assert_eq!(code, expected),
        other => panic!("expected `reject`, got {other:?}"),
    }
}
