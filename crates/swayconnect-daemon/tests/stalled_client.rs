//! A client that stops reading, over an in-memory pipe that fills up after a few hundred
//! bytes (`docs/DESIGN.md` 4.6 rules 9 and 10).

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;
use support::wire::{self, Wire};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::security::Security;
use swayconnect_daemon::session::{Shared, serve};
use swayconnect_daemon::sway_handle::{Published, SwayHandle};
use swayconnect_protocol::proof::device_key;
use swayconnect_protocol::{Desktop, DeviceId, JsonObject, Scope, SwayStatus, Window};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::TestDirs;
use tokio::io::DuplexStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

const PIPE_BYTES: usize = 512;
/// Longer than several pipes, so one `state.changed` cannot be written in one go.
const TITLE_BYTES: usize = 16 * PIPE_BYTES;
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const WINDOW_LIFETIME: Duration = Duration::from_secs(300);
const PEER: ([u8; 4], u16) = ([127, 0, 0, 1], 40_000);

struct Bench {
    _dirs: TestDirs,
    shared: Shared,
    desktop: watch::Sender<Arc<Published>>,
    _shutdown: watch::Sender<bool>,
}

struct Device {
    id: DeviceId,
    key: [u8; 32],
}

fn published(rev: u64) -> Arc<Published> {
    let window = Window {
        id: 6,
        workspace_id: 4,
        app_id: None,
        title: Some("t".repeat(TITLE_BYTES)),
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

fn rev_of(frame: &JsonObject) -> Option<u64> {
    frame["data"]["rev"].as_u64()
}

impl Bench {
    fn start() -> Bench {
        let dirs = TestDirs::new();
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let config = Config::from_toml("").expect("config");
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
        Bench {
            _dirs: dirs,
            shared,
            desktop,
            _shutdown: shutdown,
        }
    }

    fn connect(&self) -> (Wire<DuplexStream>, JoinHandle<()>) {
        let peer = SocketAddr::from(PEER);
        let (client, server) = tokio::io::duplex(PIPE_BYTES);
        let permit = self.shared.security.admit(peer.ip()).expect("admitted");
        let session = tokio::spawn(serve(server, peer, permit, self.shared.clone()));
        (Wire::new(client), session)
    }

    /// Pairs a device with the owner's yes, without the admin socket.
    async fn paired_device(&self) -> Device {
        let security = &self.shared.security;
        let scopes = [Scope::StateRead, Scope::StateTitles].into();
        let window = security
            .open_pairing(scopes, WINDOW_LIFETIME)
            .expect("pairing window");
        let (mut client, _session) = self.connect();
        let context = client.hello(security.fingerprint()).await;
        client
            .send(&wire::pair(window.secret.bytes(), &context))
            .await;
        let welcome = client.receive().await;
        let id = welcome["device_id"].as_str().expect("device_id");
        let notice = window.pending.await.expect("pending notice");
        notice.answer.send(true).expect("answer taken");
        assert_eq!(client.receive().await["event"], "pairing.confirmed");
        Device {
            id: DeviceId::parse(id).expect("a device id"),
            key: device_key(window.secret.bytes(), &context),
        }
    }

    /// A session of `device` that has subscribed to `desktop` and read the first state.
    async fn subscriber(&self, device: &Device) -> (Wire<DuplexStream>, JoinHandle<()>) {
        let (mut client, session) = self.connect();
        let context = client.hello(self.shared.security.fingerprint()).await;
        client
            .send(&wire::auth(&device.id, &device.key, &context))
            .await;
        assert_eq!(client.receive().await["type"], "welcome");
        let subscribe = json!({"type": "req", "id": 1, "op": "state.subscribe",
            "args": {"topics": ["desktop"]}});
        client.send(&subscribe).await;
        let response = client.receive().await;
        assert_eq!(response["result"]["states"][0]["rev"], 1);
        (client, session)
    }
}

#[tokio::test(start_paused = true)]
async fn a_revoke_during_a_stalled_write_closes_the_session_within_the_write_timeout() {
    let bench = Bench::start();
    let device = bench.paired_device().await;
    let (mut client, session) = bench.subscriber(&device).await;

    // The client reads nothing from here on, so the write of this state stalls.
    bench.desktop.send_replace(published(2));
    let stalled_since = Instant::now();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!session.is_finished());
    let was_stored = bench.shared.security.revoke(&device.id);
    assert!(was_stored.expect("revoke"));

    session.await.expect("the session task ends");
    assert!(stalled_since.elapsed() <= WRITE_TIMEOUT + Duration::from_millis(100));
    // What is left in the pipe is the start of one frame. Nothing complete arrives.
    assert_eq!(client.next().await, None);
    assert!(!bench.shared.security.is_active(&device.id));
}

#[tokio::test]
async fn a_stalled_client_gets_the_newest_snapshot_and_none_in_between() {
    let bench = Bench::start();
    let device = bench.paired_device().await;
    let (mut client, _session) = bench.subscriber(&device).await;

    bench.desktop.send_replace(published(2));
    // The first bytes show that the session is inside the write of `rev` 2. The rest of
    // that frame does not fit the pipe, so it stays there while three more are published.
    assert!(client.fill().await > 0);
    for rev in 3..=5 {
        bench.desktop.send_replace(published(rev));
    }

    assert_eq!(rev_of(&client.receive().await), Some(2));
    assert_eq!(rev_of(&client.receive().await), Some(5));
    client
        .send(&json!({"type": "req", "id": 2, "op": "ping"}))
        .await;
    let next = client.receive().await;
    assert_eq!((&next["type"], &next["id"]), (&json!("res"), &json!(2)));
}
