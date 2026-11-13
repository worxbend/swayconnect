//! Tests of `security.rs`.

use std::fs;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use swayconnect_protocol::fingerprint::encode_base64url;

use super::*;
use crate::paths::write_atomic;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1");
const SERVER_NAME: &str = "desk-of-the-owner";
const LIFETIME: Duration = Duration::from_secs(300);
const LAN_PEER: &str = "192.168.1.77";

/// A `Security` with the certificate of the spec vectors, so the proofs can be compared
/// with `protocol/vectors/v1/proofs.json`.
struct Fixture {
    root: tempfile::TempDir,
    paths: Paths,
    security: Security,
    proofs: serde_json::Value,
}

fn vector(file: &str) -> serde_json::Value {
    let text = fs::read_to_string(format!("{VECTORS}/{file}")).expect("vector file");
    serde_json::from_str(&text).expect("vector JSON")
}

fn hex32(value: &serde_json::Value) -> [u8; 32] {
    let text = value.as_str().expect("hex string");
    let byte = |index: usize| u8::from_str_radix(&text[2 * index..2 * index + 2], 16);
    std::array::from_fn(|index| byte(index).expect("hex digit pair"))
}

impl Fixture {
    fn new() -> Fixture {
        let root = tempfile::tempdir().expect("temp dir");
        let paths = Paths {
            config_file: root.path().join("config.toml"),
            data_dir: root.path().join("data"),
            runtime_dir: root.path().join("runtime"),
        };
        ensure_private_dir(&paths.data_dir, own_uid().expect("uid")).expect("data dir");
        let identity = &vector("fingerprint.json")[0];
        for (file, member) in [
            (paths.identity_cert(), "cert_der_b64"),
            (paths.identity_key(), "key_pkcs8_b64"),
        ] {
            let der = STANDARD.decode(identity[member].as_str().expect("base64"));
            write_atomic(&file, &der.expect("standard base64")).expect("identity file");
        }
        let security = Self::open(&paths);
        let proofs = vector("proofs.json");
        assert_eq!(security.fingerprint(), hex32(&proofs["inputs"]["fp"]));
        Fixture {
            root,
            paths,
            security,
            proofs,
        }
    }

    fn open(paths: &Paths) -> Security {
        let toml = format!(
            "server_name = \"{SERVER_NAME}\"\nlisten = [\"127.0.0.1:0\"]\nallow_from = [\"192.168.1.0/24\"]"
        );
        let config = Config::from_toml(&toml).expect("config");
        Security::open(&config, paths).expect("security")
    }

    fn secret(&self) -> [u8; 32] {
        hex32(&self.proofs["inputs"]["s"])
    }

    fn handshake(&self, connection: &str) -> Handshake {
        Handshake {
            server_nonce: hex32(&self.proofs["inputs"][connection]["server_nonce"]),
            peer: SocketAddr::new(LAN_PEER.parse().expect("address"), 40_000),
        }
    }

    fn context(&self, connection: &str) -> Context {
        Context {
            fp: self.security.fingerprint(),
            server_nonce: self.handshake(connection).server_nonce,
            client_nonce: hex32(&self.proofs["inputs"][connection]["client_nonce"]),
        }
    }

    /// Opens a window with the secret of the vectors instead of a random one.
    fn open_window(&self) -> OpenedWindow {
        let scopes = BTreeSet::from([Scope::StateRead, Scope::Control]);
        let mut state = self.security.inner.state();
        state
            .pairing
            .open(self.secret(), scopes, LIFETIME)
            .expect("window")
    }

    fn pair_with(&self, secret: &[u8; 32]) -> Result<Admitted, RejectCode> {
        let context = self.context("pairing");
        let msg = Pair {
            protocol: 1,
            nonce: context.client_nonce,
            proof: proof::pair_proof(secret, &context),
            device_name: "Pixel".to_owned(),
            platform: Some("android".to_owned()),
            client: Some("pair-client/1".to_owned()),
        };
        self.security.verify_pair(&self.handshake("pairing"), &msg)
    }

    fn pair(&self) -> Result<Admitted, RejectCode> {
        self.pair_with(&self.secret())
    }

    fn auth_with(&self, device_id: &DeviceId, key: &[u8; 32]) -> Result<Admitted, RejectCode> {
        let context = self.context("later");
        let msg = Auth {
            protocol: 1,
            device_id: device_id.clone(),
            nonce: context.client_nonce,
            proof: proof::client_proof(key, &context, device_id),
            client: Some("auth-client/2".to_owned()),
        };
        self.security.verify_auth(&self.handshake("later"), &msg)
    }

    /// Pairs one device with the owner's yes. Returns its admission and its key `K`.
    async fn paired_device(&self) -> (Admitted, [u8; 32]) {
        let opened = self.open_window();
        let mut admitted = self.pair().expect("pair");
        let notice = opened.pending.await.expect("notice");
        notice.answer.send(true).expect("task waits");
        assert!(matches!(
            notice.outcome.await,
            Ok(PairingOutcome::Paired { .. })
        ));
        let pending = admitted.pending.take().expect("pending");
        pending.confirmed.await.expect("confirmed fires");
        let key = proof::device_key(&self.secret(), &self.context("pairing"));
        (admitted, key)
    }

    fn stored_devices(&self) -> Option<serde_json::Value> {
        let text = fs::read_to_string(self.paths.devices_file()).ok()?;
        let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        Some(json["devices"].clone())
    }
}

#[tokio::test(start_paused = true)]
async fn a_yes_stores_the_device_with_the_key_and_code_of_the_spec_vectors() {
    let fixture = Fixture::new();
    let expect = &fixture.proofs["expect"];
    let opened = fixture.open_window();

    let admitted = fixture.pair().expect("pair");
    let pending = admitted.pending.expect("pending after pair");
    assert_eq!(pending.confirm_code, expect["confirm_code"]);
    assert!(admitted.scopes.is_empty());
    assert!(fixture.security.is_active(&admitted.device_id));
    assert_eq!(
        fixture.stored_devices(),
        None,
        "a pending device is not written"
    );
    let notice = opened.pending.await.expect("notice");
    assert_eq!(notice.confirm_code, pending.confirm_code);
    assert_eq!(
        (notice.device_name.as_str(), notice.peer.port()),
        ("Pixel", 40_000)
    );
    assert_eq!(
        fixture.pair().err(),
        Some(RejectCode::PairingFailed),
        "single use"
    );

    notice.answer.send(true).expect("task waits");
    pending.confirmed.await.expect("confirmed fires");

    let stored = fixture.stored_devices().expect("devices.json");
    assert_eq!(stored[0]["key"], encode_base64url(&hex32(&expect["k"])));
    assert_eq!(stored[0]["id"], admitted.device_id.as_str());
    assert_eq!(
        stored[0]["scopes"],
        serde_json::json!(["state.read", "control"])
    );
    assert!(!*admitted.revoked.borrow());
    // The server proof is the vector's formula with this connection's random device id.
    let key = hex32(&expect["k"]);
    let context = fixture.context("pairing");
    assert!(proof::verify_server_proof(
        &key,
        &context,
        &admitted.device_id,
        &admitted.server_proof
    ));
}

#[tokio::test(start_paused = true)]
async fn a_no_the_deadline_or_a_dropped_session_delete_the_pending_device() {
    let fixture = Fixture::new();

    let opened = fixture.open_window();
    let mut refused = fixture.pair().expect("pair");
    let notice = opened.pending.await.expect("notice");
    notice.answer.send(false).expect("task waits");
    assert_eq!(notice.outcome.await, Ok(PairingOutcome::Refused));
    refused.revoked.changed().await.expect("revoke signal");
    assert!(*refused.revoked.borrow());
    assert!(!fixture.security.is_active(&refused.device_id));

    let opened = fixture.open_window();
    let mut late = fixture.pair().expect("pair");
    let notice = opened.pending.await.expect("notice");
    tokio::time::advance(Duration::from_millis(59_999)).await;
    assert!(fixture.security.is_active(&late.device_id));
    tokio::time::advance(Duration::from_millis(1)).await;
    assert_eq!(notice.outcome.await, Ok(PairingOutcome::DeadlinePassed));
    late.revoked.changed().await.expect("revoke signal");

    let opened = fixture.open_window();
    let gone = fixture.pair().expect("pair");
    let notice = opened.pending.await.expect("notice");
    let device_id = gone.device_id.clone();
    drop(gone);
    assert_eq!(notice.outcome.await, Ok(PairingOutcome::ClientGone));
    assert!(!fixture.security.is_active(&device_id));

    // The admin side going away is a no as well.
    let opened = fixture.open_window();
    let mut orphan = fixture.pair().expect("pair");
    drop(opened.pending.await.expect("notice"));
    orphan.revoked.changed().await.expect("revoke signal");

    assert_eq!(fixture.stored_devices(), None, "nothing was written");
    assert!(fixture.security.devices().is_empty());
}

#[tokio::test(start_paused = true)]
async fn pairing_fails_alike_without_a_window_after_expiry_and_on_a_bad_proof() {
    let fixture = Fixture::new();
    hmac_probe::take_count();
    assert_eq!(fixture.pair().err(), Some(RejectCode::PairingFailed));
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "no window: one HMAC with the dummy key"
    );

    let opened = fixture.open_window();
    assert_eq!(
        fixture.pair_with(&[0x55; 32]).err(),
        Some(RejectCode::PairingFailed)
    );
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "bad proof: one HMAC with the window's secret"
    );
    let mut admitted = fixture.pair().expect("the bad proof left the window open");
    drop(opened);
    admitted
        .revoked
        .changed()
        .await
        .expect("refused: the admin side is gone");

    let _expired = fixture.open_window();
    tokio::time::advance(LIFETIME).await;
    hmac_probe::take_count();
    assert_eq!(fixture.pair().err(), Some(RejectCode::PairingFailed));
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "expired window: one HMAC with the dummy key"
    );
}

#[tokio::test(start_paused = true)]
async fn unknown_device_pending_device_and_wrong_proof_fail_alike() {
    let fixture = Fixture::new();
    let (stored, key) = fixture.paired_device().await;
    let opened = fixture.open_window();
    let pending = fixture.pair().expect("pair");
    let _notice = opened.pending.await.expect("notice");
    let unknown = DeviceId::from_bytes([9; 16]);

    hmac_probe::take_count();
    let unknown_outcome = fixture.auth_with(&unknown, &key).err();
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "unknown device: one HMAC all the same"
    );
    // The pending device and the stored one have the same `K` here: same `S`, same nonces.
    let pending_outcome = fixture.auth_with(&pending.device_id, &key).err();
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "pending device: one HMAC with the dummy key"
    );
    let wrong_proof_outcome = fixture.auth_with(&stored.device_id, &[0x55; 32]).err();
    assert_eq!(
        hmac_probe::take_count(),
        1,
        "wrong proof: one HMAC with the device's key"
    );

    assert_eq!(unknown_outcome, Some(RejectCode::AuthFailed));
    assert_eq!(pending_outcome, unknown_outcome);
    assert_eq!(wrong_proof_outcome, unknown_outcome);
}

#[tokio::test(start_paused = true)]
async fn auth_admits_a_stored_device_and_keeps_the_client_of_pair_on_disk() {
    let fixture = Fixture::new();
    let (paired, key) = fixture.paired_device().await;

    let admitted = fixture.auth_with(&paired.device_id, &key).expect("auth");

    assert!(admitted.pending.is_none());
    assert_eq!(
        admitted.scopes,
        BTreeSet::from([Scope::StateRead, Scope::Control])
    );
    let context = fixture.context("later");
    assert!(proof::verify_server_proof(
        &key,
        &context,
        &paired.device_id,
        &admitted.server_proof
    ));
    let listed = fixture.security.devices();
    assert_eq!(listed[0].client.as_deref(), Some("pair-client/1"));
    assert_eq!(listed[0].last_client.as_deref(), Some("auth-client/2"));
    assert!(listed[0].last_seen.is_some());
    assert_eq!(
        listed[0].connections, 2,
        "the pairing session and the new one"
    );
    let stored = fixture.stored_devices().expect("devices.json");
    assert_eq!(stored[0]["client"], "pair-client/1");

    // After a restart the device still authenticates.
    let reopened = Fixture::open(&fixture.paths);
    let context = fixture.context("later");
    let msg = Auth {
        protocol: 1,
        device_id: paired.device_id.clone(),
        nonce: context.client_nonce,
        proof: proof::client_proof(&key, &context, &paired.device_id),
        client: None,
    };
    assert!(
        reopened
            .verify_auth(&fixture.handshake("later"), &msg)
            .is_ok()
    );
}

#[tokio::test(start_paused = true)]
async fn revoke_reaches_receivers_created_before_and_after_it() {
    let fixture = Fixture::new();
    let (paired, key) = fixture.paired_device().await;
    let mut session = fixture.auth_with(&paired.device_id, &key).expect("auth");
    assert!(fixture.security.is_active(&paired.device_id));

    assert!(fixture.security.revoke(&paired.device_id).expect("revoke"));

    assert!(!fixture.security.is_active(&paired.device_id));
    session.revoked.changed().await.expect("revoke signal");
    assert!(*session.revoked.borrow());
    assert!(
        *session.revoked.clone().borrow(),
        "a receiver made after the revoke"
    );
    assert!(*paired.revoked.borrow());
    assert_eq!(
        fixture.auth_with(&paired.device_id, &key).err(),
        Some(RejectCode::AuthFailed)
    );
    assert!(
        fixture
            .security
            .take_action_token(&paired.device_id)
            .is_err()
    );
    assert_eq!(fixture.stored_devices(), Some(serde_json::json!([])));
    assert!(
        !fixture
            .security
            .revoke(&paired.device_id)
            .expect("second revoke")
    );
}

#[tokio::test(start_paused = true)]
async fn twenty_failed_handshakes_do_not_lock_the_address_out() {
    let fixture = Fixture::new();
    let (paired, key) = fixture.paired_device().await;
    let peer: IpAddr = LAN_PEER.parse().expect("address");
    for _ in 0..20 {
        let _permit = fixture
            .security
            .admit(peer)
            .expect("admitted at the refill rate");
        assert!(fixture.auth_with(&paired.device_id, &[0x55; 32]).is_err());
        tokio::time::advance(Duration::from_secs(2)).await;
    }
    let _permit = fixture.security.admit(peer).expect("no lockout");
    assert!(fixture.auth_with(&paired.device_id, &key).is_ok());
}

#[tokio::test(start_paused = true)]
async fn admit_applies_allow_from_the_bucket_and_the_caps() {
    let fixture = Fixture::new();
    let security = &fixture.security;
    let loopback: IpAddr = "127.0.0.1".parse().expect("address");
    let lan: IpAddr = LAN_PEER.parse().expect("address");
    assert_eq!(
        security.admit("10.0.0.1".parse().expect("address")).err(),
        Some(Refused::NotAllowed)
    );
    for _ in 0..5 {
        drop(security.admit(lan).expect("burst"));
    }
    assert_eq!(security.admit(lan).err(), Some(Refused::RateLimited));

    let mut permits: Vec<ConnectionPermit> = (0..8)
        .map(|_| security.admit(loopback).expect("loopback is exempt"))
        .collect();
    let pending_eviction = tokio::time::timeout(Duration::from_secs(1), permits[0].evicted());
    assert!(
        pending_eviction.await.is_err(),
        "nobody is evicted below the cap"
    );
    permits.push(security.admit(loopback).expect("the ninth takes a slot"));
    permits[0].evicted().await;
    permits[0].evicted().await;
    fn assert_send(_: &impl Send) {}
    assert_send(&permits[0].evicted());
    let other = tokio::time::timeout(Duration::from_secs(1), permits[1].evicted());
    assert!(other.await.is_err(), "only the oldest is evicted");

    for permit in &mut permits {
        permit.welcomed();
    }
    permits.extend((0..7).map(|_| security.admit(loopback).expect("up to the total cap")));
    assert_eq!(security.admit(loopback).err(), Some(Refused::Full));
    permits.pop();
    assert!(security.admit(loopback).is_ok());
}

#[tokio::test(start_paused = true)]
async fn the_action_bucket_is_per_device_and_returns_the_retry_delay() {
    let fixture = Fixture::new();
    let (paired, _key) = fixture.paired_device().await;
    for _ in 0..5 {
        assert_eq!(
            fixture.security.take_action_token(&paired.device_id),
            Ok(())
        );
    }
    let half_second = Duration::from_millis(500);
    assert_eq!(
        fixture.security.take_action_token(&paired.device_id),
        Err(half_second)
    );
    tokio::time::advance(half_second).await;
    assert_eq!(
        fixture.security.take_action_token(&paired.device_id),
        Ok(())
    );
    assert!(
        fixture
            .security
            .take_action_token(&DeviceId::from_bytes([9; 16]))
            .is_err()
    );
}

#[tokio::test]
async fn the_server_name_is_not_in_the_certificate_and_loose_data_is_refused() {
    let fixture = Fixture::new();
    let created = tempfile::tempdir().expect("temp dir");
    let paths = Paths {
        data_dir: created.path().join("share").join("swayconnect"),
        ..fixture.paths.clone()
    };
    Fixture::open(&paths);
    let der = fs::read(paths.identity_cert()).expect("created certificate");
    let name = SERVER_NAME.as_bytes();
    assert!(!der.windows(name.len()).any(|window| window == name));

    let loose = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    fs::set_permissions(&paths.data_dir, loose).expect("chmod");
    let config = Config::from_toml("listen = [\"127.0.0.1:0\"]").expect("config");
    assert!(matches!(
        Security::open(&config, &paths),
        Err(StartError::Path(_))
    ));
    drop(fixture.root);
}
