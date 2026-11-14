//! Tests of `admin.rs`.

use swayconnect_protocol::proof::{Context, pair_proof};
use swayconnect_protocol::{Pair, RejectCode};

use super::*;
use crate::config::Config;
use crate::security::{Admitted, Handshake};

const WAIT: Duration = Duration::from_secs(10);

struct Fixture {
    _root: tempfile::TempDir,
    paths: Paths,
    security: Security,
    stop: watch::Sender<bool>,
    server: tokio::task::JoinHandle<()>,
}

fn temp_paths() -> (tempfile::TempDir, Paths) {
    let root = tempfile::tempdir().expect("temp dir");
    let paths = Paths {
        config_file: root.path().join("config.toml"),
        data_dir: root.path().join("data"),
        runtime_dir: root.path().join("runtime"),
    };
    let uid = own_uid().expect("uid");
    ensure_private_dir(&paths.runtime_dir, uid).expect("runtime dir");
    (root, paths)
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").mode() & 0o777
}

fn request(host: Option<&str>) -> PairingRequest {
    PairingRequest {
        scopes: BTreeSet::from([Scope::StateRead]),
        lifetime: Duration::from_secs(300),
        host: host.map(str::to_owned),
    }
}

impl Fixture {
    fn start() -> Fixture {
        let (root, paths) = temp_paths();
        let toml = "server_name = \"desk\"\nlisten = [\"127.0.0.1:0\"]";
        let config = Config::from_toml(toml).expect("config");
        let security = Security::open(&config, &paths).expect("security");
        let advertised = Advertised {
            server_name: config.server_name,
            bound: vec!["127.0.0.1:40123".parse().expect("address")],
        };
        let (stop, shutdown) = watch::channel(false);
        let server = AdminServer::bind(&paths).expect("bind");
        let server = tokio::spawn(server.run(security.clone(), advertised, shutdown));
        Fixture {
            _root: root,
            paths,
            security,
            stop,
            server,
        }
    }

    async fn client(&self) -> AdminClient {
        AdminClient::connect(&self.paths).await.expect("connect")
    }

    /// Plays the device: presents the secret of the pairing URI.
    fn present(&self, uri: &str) -> Result<Admitted, RejectCode> {
        let uri = PairingUri::parse(uri).expect("the daemon wrote a valid URI");
        assert_eq!(uri.fp, self.security.fingerprint());
        let hs = Handshake {
            server_nonce: [1; 32],
            peer: "127.0.0.1:50000".parse().expect("address"),
        };
        let context = Context {
            fp: uri.fp,
            server_nonce: hs.server_nonce,
            client_nonce: [2; 32],
        };
        let msg = Pair {
            protocol: 1,
            nonce: context.client_nonce,
            proof: pair_proof(&uri.secret, &context),
            device_name: "Pixel".to_owned(),
            platform: Some("android".to_owned()),
            client: None,
        };
        self.security.verify_pair(&hs, &msg)
    }
}

#[tokio::test]
async fn pairing_with_a_yes_then_list_and_revoke_round_trip() {
    let fixture = Fixture::start();
    assert_eq!(mode_of(&fixture.paths.admin_dir()), 0o700);
    assert_eq!(mode_of(&fixture.paths.admin_socket()), 0o600);
    let mut client = fixture.client().await;

    let uri = client.open_pairing(&request(None)).await.expect("window");
    let parsed = PairingUri::parse(&uri).expect("valid URI");
    assert_eq!((parsed.host.as_str(), parsed.port), ("127.0.0.1", 40123));
    assert_eq!(parsed.name.as_deref(), Some("desk"));
    let admitted = fixture.present(&uri).expect("pair");
    let pending = admitted.pending.expect("pending");

    let device = client
        .pending_device()
        .await
        .expect("reply")
        .expect("pending device");
    assert_eq!(device.device_name, "Pixel");
    assert_eq!(device.platform.as_deref(), Some("android"));
    assert_eq!(device.peer.port(), 50000);
    assert_eq!(device.confirm_code, pending.confirm_code);
    client.answer(true).await.expect("answer");
    let PairingResult::Paired(paired) = client.pairing_result().await.expect("result") else {
        panic!("not paired");
    };
    pending.confirmed.await.expect("confirmed fires");
    assert_eq!(paired.device_id, admitted.device_id.as_str());
    assert_eq!(paired.scopes, ["state.read"]);

    let mut client = fixture.client().await;
    let devices = client.devices().await.expect("list");
    assert_eq!(devices.len(), 1);
    assert_eq!(
        (devices[0].id.as_str(), devices[0].name.as_str()),
        (paired.device_id.as_str(), "Pixel")
    );
    assert_eq!(devices[0].connections, 1);
    assert!(client.revoke(&admitted.device_id).await.expect("revoke"));
    assert!(
        !client
            .revoke(&admitted.device_id)
            .await
            .expect("second revoke")
    );
    assert!(client.devices().await.expect("list").is_empty());
    assert!(*admitted.revoked.borrow());
}

#[tokio::test]
async fn pairing_with_a_no_stores_nothing() {
    let fixture = Fixture::start();
    let mut client = fixture.client().await;
    let uri = client
        .open_pairing(&request(Some("desk.example")))
        .await
        .expect("window");
    assert_eq!(
        PairingUri::parse(&uri).expect("valid URI").host,
        "desk.example"
    );
    let mut admitted = fixture.present(&uri).expect("pair");
    client
        .pending_device()
        .await
        .expect("reply")
        .expect("pending device");

    client.answer(false).await.expect("answer");

    assert_eq!(
        client.pairing_result().await.expect("result"),
        PairingResult::Refused
    );
    admitted.revoked.changed().await.expect("revoke signal");
    assert!(
        fixture
            .client()
            .await
            .devices()
            .await
            .expect("list")
            .is_empty()
    );
    assert!(!fixture.paths.devices_file().exists());
}

#[tokio::test]
async fn closing_the_connection_cancels_the_window_and_refuses_a_pending_device() {
    let fixture = Fixture::start();
    let mut client = fixture.client().await;
    let uri = client.open_pairing(&request(None)).await.expect("window");
    let mut second = fixture.client().await;
    assert!(matches!(
        second.open_pairing(&request(None)).await,
        Err(AdminError::Daemon(message)) if message.contains("in progress")
    ));
    drop(client);
    // The server cancels when it reads the end of the connection: wait for that, not for time.
    let reopened = tokio::time::timeout(WAIT, async {
        loop {
            let mut client = fixture.client().await;
            if let Ok(uri) = client.open_pairing(&request(None)).await {
                return (client, uri);
            }
            tokio::task::yield_now().await;
        }
    });
    let (mut client, new_uri) = reopened.await.expect("the first window was cancelled");
    assert_eq!(fixture.present(&uri).err(), Some(RejectCode::PairingFailed));

    let mut admitted = fixture.present(&new_uri).expect("pair");
    client
        .pending_device()
        .await
        .expect("reply")
        .expect("pending device");
    drop(client);
    let refused = tokio::time::timeout(WAIT, admitted.revoked.changed());
    refused
        .await
        .expect("refused in time")
        .expect("revoke signal");
    assert!(*admitted.revoked.borrow());
    assert!(!fixture.paths.devices_file().exists());
}

#[tokio::test(start_paused = true)]
async fn bad_requests_get_an_error_and_an_unused_window_expires() {
    let fixture = Fixture::start();
    let mut client = fixture.client().await;
    for (bad, expected) in [
        (
            PairingRequest {
                lifetime: Duration::from_secs(29),
                ..request(None)
            },
            "lifetime",
        ),
        (
            PairingRequest {
                lifetime: Duration::from_secs(601),
                ..request(None)
            },
            "lifetime",
        ),
        (
            PairingRequest {
                scopes: BTreeSet::new(),
                ..request(None)
            },
            "scope",
        ),
        (request(Some("bad host")), "host"),
    ] {
        match client.open_pairing(&bad).await {
            Err(AdminError::Daemon(message)) => {
                assert!(message.contains(expected), "{message}")
            }
            _ => panic!("accepted a bad request ({expected})"),
        }
    }
    client
        .open_pairing(&request(None))
        .await
        .expect("the bad host left no window behind");
    assert_eq!(
        client.pending_device().await.expect("reply"),
        None,
        "expired after 300 s"
    );
}

#[tokio::test]
async fn an_unset_relative_loose_or_foreign_runtime_dir_is_refused() {
    let (_root, paths) = temp_paths();
    let uid = own_uid().expect("uid");
    for runtime_dir in ["", "run/user"] {
        let paths = Paths {
            runtime_dir: PathBuf::from(runtime_dir),
            ..paths.clone()
        };
        assert!(matches!(
            AdminServer::bind(&paths),
            Err(StartError::Path(PathError::Refused {
                problem: Problem::NotAbsolute,
                ..
            }))
        ));
        assert!(matches!(
            AdminClient::connect(&paths).await,
            Err(AdminError::Path(PathError::Refused {
                problem: Problem::NotAbsolute,
                ..
            }))
        ));
    }
    assert!(matches!(
        check_runtime_dir(&paths, uid + 1),
        Err(PathError::Refused {
            problem: Problem::WrongOwner,
            ..
        })
    ));
    fs::set_permissions(&paths.runtime_dir, Permissions::from_mode(0o750)).expect("chmod");
    assert!(matches!(
        AdminServer::bind(&paths),
        Err(StartError::Path(PathError::Refused {
            problem: Problem::LooseMode,
            ..
        }))
    ));
}

#[tokio::test]
async fn an_admin_dir_that_is_loose_a_symlink_or_foreign_is_refused_on_both_ends() {
    let (root, paths) = temp_paths();
    let uid = own_uid().expect("uid");
    fs::create_dir(paths.admin_dir()).expect("mkdir");
    fs::set_permissions(paths.admin_dir(), Permissions::from_mode(0o755)).expect("chmod");
    let is_refused = |problem: Problem, bound: Result<AdminServer, StartError>| matches!(bound, Err(StartError::Path(PathError::Refused { problem: found, .. })) if found == problem);
    assert!(is_refused(Problem::LooseMode, AdminServer::bind(&paths)));
    assert!(matches!(
        AdminClient::connect(&paths).await,
        Err(AdminError::Path(PathError::Refused {
            problem: Problem::LooseMode,
            ..
        }))
    ));

    fs::remove_dir(paths.admin_dir()).expect("rmdir");
    let elsewhere = root.path().join("elsewhere");
    ensure_private_dir(&elsewhere, uid).expect("mkdir");
    std::os::unix::fs::symlink(&elsewhere, paths.admin_dir()).expect("symlink");
    assert!(is_refused(Problem::Symlink, AdminServer::bind(&paths)));
    assert!(matches!(
        AdminClient::connect(&paths).await,
        Err(AdminError::Path(PathError::Refused {
            problem: Problem::Symlink,
            ..
        }))
    ));
    assert!(matches!(
        ensure_private_dir(&elsewhere, uid + 1),
        Err(PathError::Refused {
            problem: Problem::WrongOwner,
            ..
        })
    ));
}

#[test]
fn a_peer_with_another_uid_is_refused() {
    assert!(check_peer_uid(1000, 1000).is_ok());
    assert!(matches!(
        check_peer_uid(1001, 1000),
        Err(AdminError::WrongPeer {
            peer: 1001,
            own: 1000
        })
    ));
}

#[tokio::test]
async fn without_a_daemon_the_client_says_so_and_a_stale_socket_is_replaced() {
    let (_root, paths) = temp_paths();
    assert!(matches!(
        AdminClient::connect(&paths).await,
        Err(AdminError::NotRunning(_))
    ));

    // A socket nobody listens on, as a crashed daemon leaves it.
    let uid = own_uid().expect("uid");
    ensure_private_dir(&paths.admin_dir(), uid).expect("admin dir");
    drop(std::os::unix::net::UnixListener::bind(paths.admin_socket()).expect("bind"));
    assert!(matches!(
        AdminClient::connect(&paths).await,
        Err(AdminError::NotRunning(_))
    ));
    let server = AdminServer::bind(&paths).expect("the stale socket is unlinked");
    drop(server);

    fs::remove_file(paths.admin_socket()).expect("remove");
    fs::write(paths.admin_socket(), b"not a socket").expect("write");
    assert!(matches!(
        AdminServer::bind(&paths),
        Err(StartError::Path(PathError::Refused {
            problem: Problem::WrongKind,
            ..
        }))
    ));
}

#[tokio::test]
async fn a_second_daemon_does_not_take_the_socket_of_a_running_one() {
    let fixture = Fixture::start();

    let second = AdminServer::bind(&fixture.paths);

    assert!(matches!(second, Err(StartError::AlreadyRunning(_))));
    let devices = fixture.client().await.devices().await;
    assert!(devices.expect("the first daemon still answers").is_empty());
}

#[tokio::test]
async fn shutdown_removes_the_socket() {
    let fixture = Fixture::start();
    assert!(fixture.paths.admin_socket().exists());
    fixture.stop.send(true).expect("server listens");
    tokio::time::timeout(WAIT, fixture.server)
        .await
        .expect("in time")
        .expect("no panic");
    assert!(!fixture.paths.admin_socket().exists());
}

impl Fixture {
    /// Pairs one device with `state.read` and returns its id.
    async fn paired_device(&self) -> DeviceId {
        let mut client = self.client().await;
        let uri = client.open_pairing(&request(None)).await.expect("window");
        let admitted = self.present(&uri).expect("pair");
        client
            .pending_device()
            .await
            .expect("reply")
            .expect("pending");
        client.answer(true).await.expect("answer");
        let result = client.pairing_result().await.expect("result");
        assert!(matches!(result, PairingResult::Paired(_)), "{result:?}");
        admitted.device_id
    }
}

#[tokio::test]
async fn grant_and_deny_by_prefix_change_the_scopes_a_session_watches() {
    let fixture = Fixture::start();
    let device = fixture.paired_device().await;
    let held = BTreeSet::from([Scope::StateRead]);
    let mut watch = fixture.security.scope_watch(&device).expect("stored");
    let mut client = fixture.client().await;
    let prefix = &device.as_str()[..4];

    let granted = client
        .change_scopes(
            prefix,
            ScopeChange::Grant,
            &BTreeSet::from([Scope::Control]),
        )
        .await
        .expect("grant");

    assert_eq!(granted.device_id, device.as_str());
    assert_eq!(granted.scopes, ["state.read", "control"]);
    assert!(granted.changed);
    tokio::time::timeout(WAIT, crate::devices::scopes_changed(&mut watch, &held))
        .await
        .expect("the session sees the change");
    let again = client
        .change_scopes(
            prefix,
            ScopeChange::Grant,
            &BTreeSet::from([Scope::Control]),
        )
        .await
        .expect("grant again");
    assert!(!again.changed);
    let denied = client
        .change_scopes(
            device.as_str(),
            ScopeChange::Deny,
            &BTreeSet::from([Scope::Control]),
        )
        .await
        .expect("deny");
    assert_eq!(denied.scopes, ["state.read"]);
    assert_eq!(*watch.borrow(), held);
    assert_eq!(
        client.devices().await.expect("list")[0].scopes,
        ["state.read"]
    );
}

#[tokio::test]
async fn a_bad_scope_change_gets_an_error_and_changes_nothing() {
    let fixture = Fixture::start();
    let device = fixture.paired_device().await;
    let mut client = fixture.client().await;
    let control = BTreeSet::from([Scope::Control]);
    for (prefix, scopes, expected) in [
        (&device.as_str()[..3], &control, "at least 4"),
        ("zzzz", &control, "no device"),
        (device.as_str(), &BTreeSet::new(), "at least one scope"),
    ] {
        match client
            .change_scopes(prefix, ScopeChange::Grant, scopes)
            .await
        {
            Err(AdminError::Daemon(message)) => assert!(message.contains(expected), "{message}"),
            other => panic!("accepted a bad change ({expected}): {other:?}"),
        }
    }
    let unknown = Request::DevicesGrant {
        prefix: device.as_str().to_owned(),
        scopes: vec!["root".to_owned()],
    };
    client.channel.send(&unknown).await.expect("send");
    match client.reply().await {
        Err(AdminError::Daemon(message)) => assert!(message.contains("unknown scope"), "{message}"),
        _ => panic!("accepted an unknown scope"),
    }
    assert_eq!(
        client.devices().await.expect("list")[0].scopes,
        ["state.read"]
    );
}

mod audit_lines;
