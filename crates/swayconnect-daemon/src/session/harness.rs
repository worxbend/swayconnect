//! The in-memory daemon and client of the session tests. No TLS, no sway, no TCP.

use serde_json::{Value, json};
use swayconnect_protocol::proof::{self, Context};
use swayconnect_protocol::{Auth, Desktop, Pair, SwayStatus, Window};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::TestDirs;
use tokio::io::DuplexStream;

use super::wire::READ_CHUNK_BYTES;
use super::*;
use crate::apps::index::IndexDirs;
use crate::audit::{AuditLog, audit_file};
use crate::config::Config;
use crate::daemon::controls::{self, HostParts};
use crate::daemon::services::{self as m2b, KeyboardHost, MediaHost, ServiceHost};
use crate::input::fake::FakeKeyboard;
use crate::media::MediaBackend;
use crate::media::fake::FakeMedia;
use crate::pairing::PendingNotice;
use crate::paths::Paths;
use crate::proc::fake::FakeRunner;
use std::path::PathBuf;
use swayconnect_protocol::frame::{FrameDecoder, encode_frame};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

pub(super) const PIPE_BYTES: usize = 1 << 20;
pub(super) const CLIENT_NONCE: [u8; 32] = [7; 32];
pub(super) const WINDOW_LIFETIME: Duration = Duration::from_secs(300);
pub(super) const TITLE: &str = "Packing list for the lake trip";
pub(super) const EVERY_SCOPE: [Scope; 5] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Control,
    Scope::IdleInhibit,
    Scope::Destructive,
];
/// Every program names a path that does not exist: the fake runner starts nothing.
pub(super) const CONFIG: &str = r#"server_name = "desk"
[session]
lock = ["/fake/lock.sh"]
[keep_awake]
inhibit = "/fake/systemd-inhibit"
sleep = "/fake/sleep"
[notify]
notify_send = "/fake/notify-send"
[audio]
pactl = "/fake/pactl"
[apps]
systemd_run = "/fake/systemd-run"
terminal = ["/fake/alacritty", "-e"]
[launcher]
argv = ["/fake/fuzzel"]
"#;

/// The desktop entries of topic `apps`: one plain app and one terminal (risky) app.
const DESKTOP_ENTRIES: [(&str, &str); 2] = [
    (
        "notes.desktop",
        "[Desktop Entry]\nType=Application\nName=Notes\nExec=/opt/notes --new %U\n",
    ),
    (
        "term.desktop",
        "[Desktop Entry]\nType=Application\nName=Term\nExec=/opt/term\nTerminal=true\n",
    ),
];

/// A data directory with [`DESKTOP_ENTRIES`] in its `applications`.
fn desktop_entries(dirs: &TestDirs) -> IndexDirs {
    let data_dir = dirs.data_dir().join("share");
    let applications = data_dir.join("applications");
    std::fs::create_dir_all(&applications).expect("applications dir");
    for (name, text) in DESKTOP_ENTRIES {
        std::fs::write(applications.join(name), text).expect("desktop entry");
    }
    IndexDirs {
        data_dirs: vec![data_dir],
        exec_path: Vec::new(),
    }
}

pub(super) struct Daemon {
    pub(super) dirs: TestDirs,
    pub(super) shared: Shared,
    pub(super) runner: FakeRunner,
    /// The M2b services' own runner, so their `pactl` reads leave `runner` to the M2a tests.
    pub(super) services_runner: FakeRunner,
    pub(super) keyboard: FakeKeyboard,
    pub(super) media: Arc<FakeMedia>,
    /// The fake `/proc` of the lock heuristic.
    pub(super) proc_root: PathBuf,
    pub(super) desktop: watch::Sender<Arc<Published>>,
    pub(super) shutdown: watch::Sender<bool>,
}

/// The client end of one connection.
pub(super) struct Client {
    pub(super) stream: DuplexStream,
    pub(super) decoder: FrameDecoder,
}

/// A credential, as a client holds it after a confirmed pairing.
pub(super) struct Device {
    pub(super) id: DeviceId,
    pub(super) key: [u8; 32],
}

/// A connection in state `PENDING`, with the admin side's view of it.
pub(super) struct PendingClient {
    pub(super) client: Client,
    pub(super) welcome: JsonObject,
    pub(super) notice: PendingNotice,
    pub(super) device: Device,
}

pub(super) fn published(rev: u64) -> Arc<Published> {
    let window = Window {
        id: 6,
        workspace_id: 4,
        app_id: Some("notes".to_owned()),
        title: Some(TITLE.to_owned()),
        focused: true,
        urgent: false,
        floating: false,
        fullscreen: false,
        icon: None,
    };
    let desktop = Desktop {
        sway: SwayStatus {
            online: true,
            version: Some("1.12".to_owned()),
        },
        workspaces: Vec::new(),
        windows: vec![window],
        truncated: false,
        outputs: None,
        scratchpad: None,
        layouts_cut: None,
    };
    Arc::new(Published { rev, desktop })
}

pub(super) fn frame(message: &Value) -> Vec<u8> {
    let object = message.as_object().expect("a JSON object");
    encode_frame(object, u32::MAX).expect("frame")
}

pub(super) fn ping(id: u64) -> Value {
    json!({"type": "req", "id": id, "op": "ping"})
}

pub(super) fn subscribe(id: u64) -> Value {
    json!({"type": "req", "id": id, "op": "state.subscribe", "args": {"topics": ["desktop"]}})
}

impl Daemon {
    pub(super) async fn start() -> Daemon {
        let dirs = TestDirs::new();
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml(CONFIG).expect("config");
        let security = Security::open(&config, &paths).expect("security");
        let (desktop, desktop_reader) = watch::channel(published(1));
        let (shutdown, shutdown_reader) = watch::channel(false);
        // No sway here: the supervisor refuses every action while it tries to connect. It
        // runs until shutdown, because topic `session` reads it.
        let sway = Arc::new(SwayHandle::spawn(
            SwayTarget::Explicit(dirs.sway_socket()),
            shutdown_reader.clone(),
        ));
        let runner = FakeRunner::default();
        let proc_root = dirs.data_dir().join("proc");
        std::fs::create_dir_all(&proc_root).expect("fake /proc");
        let uid = rustix::process::getuid().as_raw();
        let audit = AuditLog::open(&audit_file(&paths), uid).expect("audit log");
        let host = HostParts {
            runner: Arc::new(runner.clone()),
            proc_root: proc_root.clone(),
        };
        let controls = controls::start(&config, host, &sway, audit, &shutdown_reader);
        let services_runner = FakeRunner::default();
        let keyboard = FakeKeyboard::default();
        let media = Arc::new(FakeMedia::default());
        let service_host = ServiceHost {
            keyboard: KeyboardHost::Sink(Arc::new(keyboard.clone())),
            media: MediaHost::Backend(Arc::clone(&media) as Arc<dyn MediaBackend>),
            apps: desktop_entries(&dirs),
            ..ServiceHost::offline(
                Arc::new(services_runner.clone()),
                proc_root.clone(),
                SwayTarget::Explicit(dirs.sway_socket()),
            )
        };
        let services = m2b::start(&config, service_host, &shutdown_reader)
            .await
            .expect("services");
        let shared = Shared {
            security,
            desktop: desktop_reader,
            sway,
            server_name: Arc::from("desk"),
            read_only: Arc::new(AtomicBool::new(false)),
            shutdown: shutdown_reader,
            icons: None,
            controls,
            services,
        };
        Daemon {
            dirs,
            shared,
            runner,
            services_runner,
            keyboard,
            media,
            proc_root,
            desktop,
            shutdown,
        }
    }

    pub(super) fn connect(&self) -> Client {
        let peer = SocketAddr::from(([127, 0, 0, 1], 40_000));
        let (client, server) = tokio::io::duplex(PIPE_BYTES);
        let permit = self.shared.security.admit(peer.ip()).expect("admitted");
        tokio::spawn(serve(server, peer, permit, self.shared.clone()));
        Client {
            stream: client,
            decoder: FrameDecoder::new(MAX_FRAME_S2C_BYTES),
        }
    }

    pub(super) fn context(&self, hello: &JsonObject) -> Context {
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
    pub(super) async fn pair(&self, scopes: &[Scope]) -> PendingClient {
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
    pub(super) async fn paired_device(&self, scopes: &[Scope]) -> Device {
        let mut pending = self.pair(scopes).await;
        pending.notice.answer.send(true).expect("answer taken");
        assert_eq!(pending.client.receive().await["event"], "pairing.confirmed");
        pending.device
    }

    pub(super) fn auth(&self, device: &Device, hello: &JsonObject) -> Auth {
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
    pub(super) async fn ready(&self, device: &Device) -> (Client, JsonObject) {
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
    pub(super) async fn send(&mut self, message: &Value) {
        self.send_bytes(&frame(message)).await;
    }

    pub(super) async fn send_bytes(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.expect("client write");
    }

    /// The next frame, or `None` when the server closed the connection.
    pub(super) async fn next(&mut self) -> Option<JsonObject> {
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

    pub(super) async fn receive(&mut self) -> JsonObject {
        self.next().await.expect("a frame, not a close")
    }

    /// Asserts the last frame is `kind` with `code` and that the server then closes.
    pub(super) async fn expect_last(&mut self, kind: &str, code: &str) {
        let last = self.receive().await;
        assert_eq!((&last["type"], &last["code"]), (&json!(kind), &json!(code)));
        assert_eq!(self.next().await, None, "the server closes after `{kind}`");
    }
}
