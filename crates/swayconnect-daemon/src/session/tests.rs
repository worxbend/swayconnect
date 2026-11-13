//! Tests of `session.rs` over an in-memory stream. No TLS, no sway, no TCP.

use serde_json::{Value, json};
use swayconnect_protocol::proof::{self, Context};
use swayconnect_protocol::{Auth, Desktop, Pair, SwayStatus, Window};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::TestDirs;
use tokio::io::DuplexStream;

use super::*;
use crate::config::Config;
use crate::pairing::PendingNotice;
use crate::paths::Paths;

const PIPE_BYTES: usize = 1 << 20;
const CLIENT_NONCE: [u8; 32] = [7; 32];
const WINDOW_LIFETIME: Duration = Duration::from_secs(300);
const TITLE: &str = "Packing list for the lake trip";
const EVERY_SCOPE: [Scope; 3] = [Scope::StateRead, Scope::StateTitles, Scope::Control];

struct Daemon {
    _dirs: TestDirs,
    shared: Shared,
    desktop: watch::Sender<Arc<Published>>,
    shutdown: watch::Sender<bool>,
}

/// The client end of one connection.
struct Client {
    stream: DuplexStream,
    decoder: FrameDecoder,
}

/// A credential, as a client holds it after a confirmed pairing.
struct Device {
    id: DeviceId,
    key: [u8; 32],
}

/// A connection in state `PENDING`, with the admin side's view of it.
struct PendingClient {
    client: Client,
    welcome: JsonObject,
    notice: PendingNotice,
    device: Device,
}

fn published(rev: u64) -> Arc<Published> {
    let window = Window {
        id: 6,
        workspace_id: 4,
        app_id: Some("notes".to_owned()),
        title: Some(TITLE.to_owned()),
        focused: true,
        urgent: false,
        floating: false,
        fullscreen: false,
    };
    let desktop = Desktop {
        sway: SwayStatus {
            online: true,
            version: Some("1.12".to_owned()),
        },
        workspaces: Vec::new(),
        windows: vec![window],
        truncated: false,
    };
    Arc::new(Published { rev, desktop })
}

fn frame(message: &Value) -> Vec<u8> {
    let object = message.as_object().expect("a JSON object");
    encode_frame(object, u32::MAX).expect("frame")
}

fn ping(id: u64) -> Value {
    json!({"type": "req", "id": id, "op": "ping"})
}

fn subscribe(id: u64) -> Value {
    json!({"type": "req", "id": id, "op": "state.subscribe", "args": {"topics": ["desktop"]}})
}

impl Daemon {
    fn start() -> Daemon {
        let dirs = TestDirs::new();
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml("server_name = \"desk\"").expect("config");
        let security = Security::open(&config, &paths).expect("security");
        let (desktop, desktop_reader) = watch::channel(published(1));
        let (shutdown, shutdown_reader) = watch::channel(false);
        let shared = Shared {
            security,
            desktop: desktop_reader,
            // No sway here: a supervisor that is told to stop at once refuses every action.
            sway: Arc::new(SwayHandle::spawn(
                SwayTarget::Explicit(dirs.sway_socket()),
                watch::channel(true).1,
            )),
            server_name: Arc::from("desk"),
            read_only: Arc::new(AtomicBool::new(false)),
            shutdown: shutdown_reader,
        };
        Daemon {
            _dirs: dirs,
            shared,
            desktop,
            shutdown,
        }
    }

    fn connect(&self) -> Client {
        let peer = SocketAddr::from(([127, 0, 0, 1], 40_000));
        let (client, server) = tokio::io::duplex(PIPE_BYTES);
        let permit = self.shared.security.admit(peer.ip()).expect("admitted");
        tokio::spawn(serve(server, peer, permit, self.shared.clone()));
        Client {
            stream: client,
            decoder: FrameDecoder::new(MAX_FRAME_S2C_BYTES),
        }
    }

    fn context(&self, hello: &JsonObject) -> Context {
        let Ok(Message::Hello(hello)) = Message::decode(hello) else {
            panic!("the first frame is not `hello`");
        };
        Context {
            fp: self.shared.security.fingerprint(),
            server_nonce: hello.nonce,
            client_nonce: CLIENT_NONCE,
        }
    }

    /// Runs `pair` with a fresh window that grants `scopes`, up to `welcome`.
    async fn pair(&self, scopes: &[Scope]) -> PendingClient {
        let scopes = scopes.iter().copied().collect();
        let security = &self.shared.security;
        let window = security
            .open_pairing(scopes, WINDOW_LIFETIME)
            .expect("window");
        let mut client = self.connect();
        let context = self.context(&client.receive().await);
        let pair = Pair {
            protocol: PROTOCOL_VERSION,
            nonce: CLIENT_NONCE,
            proof: proof::pair_proof(window.secret.bytes(), &context),
            device_name: "test phone".to_owned(),
            platform: None,
            client: None,
        };
        client
            .send(&Value::Object(Message::Pair(pair).encode()))
            .await;
        let welcome = client.receive().await;
        let id = welcome["device_id"].as_str().expect("device_id");
        let device = Device {
            id: DeviceId::parse(id).expect("device id"),
            key: proof::device_key(window.secret.bytes(), &context),
        };
        PendingClient {
            client,
            welcome,
            notice: window.pending.await.expect("pending notice"),
            device,
        }
    }

    /// A stored device with `scopes`: paired and confirmed by the owner.
    async fn paired_device(&self, scopes: &[Scope]) -> Device {
        let mut pending = self.pair(scopes).await;
        pending.notice.answer.send(true).expect("answer taken");
        assert_eq!(pending.client.receive().await["event"], "pairing.confirmed");
        pending.device
    }

    fn auth(&self, device: &Device, hello: &JsonObject) -> Auth {
        let context = self.context(hello);
        Auth {
            protocol: PROTOCOL_VERSION,
            device_id: device.id.clone(),
            nonce: CLIENT_NONCE,
            proof: proof::client_proof(&device.key, &context, &device.id),
            client: None,
        }
    }

    /// A connection of `device` in state `READY`, and its `welcome`.
    async fn ready(&self, device: &Device) -> (Client, JsonObject) {
        let mut client = self.connect();
        let hello = client.receive().await;
        let auth = Message::Auth(self.auth(device, &hello)).encode();
        client.send(&Value::Object(auth)).await;
        let welcome = client.receive().await;
        assert_eq!(welcome["type"], "welcome");
        (client, welcome)
    }
}

impl Client {
    async fn send(&mut self, message: &Value) {
        self.send_bytes(&frame(message)).await;
    }

    async fn send_bytes(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.expect("client write");
    }

    /// The next frame, or `None` when the server closed the connection.
    async fn next(&mut self) -> Option<JsonObject> {
        loop {
            if let Some(frame) = self.decoder.next_frame().expect("a valid server frame") {
                return Some(frame);
            }
            let mut chunk = [0u8; READ_CHUNK_BYTES];
            match self.stream.read(&mut chunk).await.expect("client read") {
                0 => return None,
                length => self.decoder.push(&chunk[..length]),
            }
        }
    }

    async fn receive(&mut self) -> JsonObject {
        self.next().await.expect("a frame, not a close")
    }

    /// Asserts the last frame is `kind` with `code` and that the server then closes.
    async fn expect_last(&mut self, kind: &str, code: &str) {
        let last = self.receive().await;
        assert_eq!((&last["type"], &last["code"]), (&json!(kind), &json!(code)));
        assert_eq!(self.next().await, None, "the server closes after `{kind}`");
    }
}

#[tokio::test]
async fn hello_is_the_first_frame_and_holds_the_version_range_and_a_nonce_only() {
    let daemon = Daemon::start();
    let hello = daemon.connect().receive().await;
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["protocol"], json!({"min": 1, "max": 1}));
    assert_eq!(hello.len(), 3);
    let other_nonce = daemon.connect().receive().await["nonce"].clone();
    assert_ne!(hello["nonce"], other_nonce);
}

#[tokio::test]
async fn auth_gets_welcome_with_scopes_ops_topics_and_limits() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (_client, welcome) = daemon.ready(&device).await;
    assert_eq!(welcome["device_id"], device.id.as_str());
    assert_eq!(
        welcome["scopes"],
        json!(["state.read", "state.titles", "control"])
    );
    let ops = json!([
        "ping",
        "state.subscribe",
        "workspace.focus",
        "window.focus",
        "window.fullscreen"
    ]);
    assert_eq!(welcome["ops"], ops);
    assert_eq!(welcome["topics"], json!(["desktop"]));
    assert_eq!(welcome["read_only"], false);
    assert_eq!(welcome["server"]["name"], "desk");
    let limits =
        json!({"max_frame_c2s": 16384, "max_frame_s2c": 1_048_576, "idle_timeout_ms": 90000});
    assert_eq!(welcome["limits"], limits);
    assert!(!welcome.contains_key("confirm_code"));
}

#[tokio::test]
async fn bytes_after_the_first_frame_and_before_welcome_are_refused() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    for extra in [frame(&ping(1)), vec![0u8]] {
        let mut client = daemon.connect();
        let hello = client.receive().await;
        let auth = Message::Auth(daemon.auth(&device, &hello)).encode();
        let mut bytes = frame(&Value::Object(auth));
        bytes.extend(extra);
        client.send_bytes(&bytes).await;
        client.expect_last("reject", "bad_message").await;
    }
}

#[tokio::test(start_paused = true)]
async fn the_first_frame_deadline_closes_with_no_frame() {
    let daemon = Daemon::start();
    let mut client = daemon.connect();
    client.receive().await;
    let hello_at = Instant::now();
    // Half a frame does not stop the deadline.
    client.send_bytes(&frame(&ping(1))[..10]).await;
    assert_eq!(client.next().await, None);
    assert_eq!(hello_at.elapsed(), FIRST_FRAME_DEADLINE);
}

#[tokio::test]
async fn the_frame_limit_is_4096_before_welcome_and_16384_after() {
    let daemon = Daemon::start();
    let mut early = daemon.connect();
    early.receive().await;
    early.send_bytes(&4097u32.to_be_bytes()).await;
    early.expect_last("reject", "bad_message").await;

    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    let padded_ping = |payload_bytes: usize| {
        let empty = frame(&json!({"type": "req", "id": 1, "op": "ping", "pad": ""}));
        let padding = "x".repeat(payload_bytes + LENGTH_PREFIX_BYTES - empty.len());
        frame(&json!({"type": "req", "id": 1, "op": "ping", "pad": padding}))
    };
    client.send_bytes(&padded_ping(16384)).await;
    assert_eq!(client.receive().await["ok"], true);
    client.send_bytes(&padded_ping(16385)).await;
    client.expect_last("bye", "too_large").await;
}

#[tokio::test]
async fn a_bad_frame_after_welcome_gets_bye_bad_frame() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    client.send_bytes(b"\x00\x00\x00\x02[]").await;
    client.expect_last("bye", "bad_frame").await;
}

#[tokio::test]
async fn pair_auth_and_a_frame_without_type_after_welcome_are_protocol_errors() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let auth = {
        let hello = daemon.connect().receive().await;
        Value::Object(Message::Auth(daemon.auth(&device, &hello)).encode())
    };
    let mut pair = auth.clone();
    pair["type"] = json!("pair");
    pair["device_name"] = json!("test phone");
    let untyped = json!({"id": 1, "op": "ping"});
    let invalid_request = json!({"type": "req", "id": "one", "op": "ping"});
    for refused in [auth, pair, untyped, invalid_request] {
        let (mut client, _) = daemon.ready(&device).await;
        client.send(&refused).await;
        client.expect_last("bye", "protocol_error").await;
    }
}

#[tokio::test]
async fn an_unknown_type_after_welcome_is_ignored() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    client.send(&json!({"type": "telemetry", "id": 1})).await;
    client.send(&ping(2)).await;
    let response = client.receive().await;
    assert_eq!(
        Value::Object(response),
        json!({"type": "res", "id": 2, "ok": true, "result": {}})
    );
}

#[tokio::test]
async fn a_version_outside_the_range_is_reported_before_the_proof() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let mut client = daemon.connect();
    let hello = client.receive().await;
    let mut auth = daemon.auth(&device, &hello);
    auth.protocol = 2;
    auth.proof = [0; 32];
    client
        .send(&Value::Object(Message::Auth(auth).encode()))
        .await;
    client.expect_last("reject", "unsupported_version").await;
}

#[tokio::test]
async fn a_wrong_proof_gets_auth_failed_and_a_wrong_secret_pairing_failed() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let mut client = daemon.connect();
    let hello = client.receive().await;
    let mut auth = daemon.auth(&device, &hello);
    auth.proof = [0; 32];
    client
        .send(&Value::Object(Message::Auth(auth).encode()))
        .await;
    client.expect_last("reject", "auth_failed").await;

    let mut client = daemon.connect();
    client.receive().await;
    let pair = Pair {
        protocol: PROTOCOL_VERSION,
        nonce: CLIENT_NONCE,
        proof: [0; 32],
        device_name: "test phone".to_owned(),
        platform: None,
        client: None,
    };
    client
        .send(&Value::Object(Message::Pair(pair).encode()))
        .await;
    client.expect_last("reject", "pairing_failed").await;
}

#[tokio::test]
async fn a_pending_device_has_no_scopes_and_can_only_ping_until_the_owner_says_yes() {
    let daemon = Daemon::start();
    let mut pending = daemon.pair(&EVERY_SCOPE).await;
    let welcome = &pending.welcome;
    assert_eq!(
        welcome["confirm_code"],
        pending.notice.confirm_code.as_str()
    );
    assert_eq!(welcome["scopes"], json!([]));
    assert_eq!(welcome["ops"], json!(["ping"]));
    assert_eq!(welcome["topics"], json!([]));

    let client = &mut pending.client;
    client.send(&subscribe(1)).await;
    let refused = client.receive().await;
    assert_eq!(refused["error"]["code"], "forbidden");
    assert_eq!(refused["error"]["detail"], json!({"scope": "state.read"}));
    client.send(&ping(2)).await;
    assert_eq!(client.receive().await["ok"], true);

    pending.notice.answer.send(true).expect("answer taken");
    let confirmed = client.receive().await;
    let expected = json!({"type": "evt", "event": "pairing.confirmed", "data": {}});
    assert_eq!(Value::Object(confirmed), expected);
    // The state does not change to READY on this connection (spec 5.1).
    client.send(&subscribe(3)).await;
    assert_eq!(client.receive().await["error"]["code"], "forbidden");
}

#[tokio::test]
async fn a_refused_pairing_gets_bye_revoked() {
    let daemon = Daemon::start();
    let mut pending = daemon.pair(&EVERY_SCOPE).await;
    pending.notice.answer.send(false).expect("answer taken");
    pending.client.expect_last("bye", "revoked").await;
}

#[tokio::test]
async fn a_device_without_state_titles_gets_every_title_as_null() {
    let daemon = Daemon::start();
    let reader = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, _) = daemon.ready(&reader).await;
    client.send(&subscribe(1)).await;
    let response = client.receive().await;
    let state = &response["result"]["states"][0];
    assert_eq!(
        (&state["topic"], &state["rev"]),
        (&json!("desktop"), &json!(1))
    );
    assert_eq!(state["data"]["windows"][0]["title"], Value::Null);
    assert_eq!(state["data"]["windows"][0]["app_id"], "notes");

    daemon.desktop.send_replace(published(2));
    let changed = client.receive().await;
    assert_eq!(changed["event"], "state.changed");
    assert_eq!(changed["data"]["data"]["windows"][0]["title"], Value::Null);
}

#[tokio::test]
async fn a_subscribed_device_with_state_titles_gets_titles_and_every_newer_state() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    // Not subscribed yet: this change must not be pushed.
    daemon.desktop.send_replace(published(2));
    client.send(&subscribe(1)).await;
    let response = client.receive().await;
    let state = &response["result"]["states"][0];
    assert_eq!(state["rev"], 2);
    assert_eq!(state["data"]["windows"][0]["title"], TITLE);

    daemon.desktop.send_replace(published(5));
    let changed = client.receive().await;
    assert_eq!(
        (&changed["type"], &changed["event"]),
        (&json!("evt"), &json!("state.changed"))
    );
    assert_eq!(changed["data"]["topic"], "desktop");
    assert_eq!(changed["data"]["rev"], 5);
    assert_eq!(changed["data"]["data"]["windows"][0]["title"], TITLE);
}

#[tokio::test]
async fn a_revoke_closes_the_session_with_bye_revoked() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    let was_stored = daemon.shared.security.revoke(&device.id).expect("revoke");
    assert!(was_stored);
    client.expect_last("bye", "revoked").await;
}

#[tokio::test]
async fn shutdown_sends_bye_shutdown() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    daemon.shutdown.send_replace(true);
    client.expect_last("bye", "shutdown").await;
}

#[tokio::test(start_paused = true)]
async fn an_idle_connection_gets_bye_idle_and_every_frame_resets_the_timer() {
    let daemon = Daemon::start();
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    tokio::time::sleep(IDLE_TIMEOUT - Duration::from_secs(1)).await;
    client.send(&ping(1)).await;
    assert_eq!(client.receive().await["ok"], true);
    let last_frame_at = Instant::now();
    client.expect_last("bye", "idle").await;
    assert_eq!(last_frame_at.elapsed(), IDLE_TIMEOUT);
}
