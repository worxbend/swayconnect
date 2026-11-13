//! Authentication, scopes and the end of a session against a running daemon
//! (`docs/PROTOCOL.md` 5.5, 7.1, 15.10, 15.11; `docs/DESIGN.md` 4.6).

mod support;

use serde_json::json;
use support::{EVERY_SCOPE, Harness, assert_bye, assert_rejected, connector, soon, wire};
use swayconnect_cli::client::ClientError;
use swayconnect_protocol::limits::{MAX_FRAME_C2S_BEFORE_WELCOME_BYTES, MAX_FRAME_C2S_BYTES};
use swayconnect_protocol::ops::Args;
use swayconnect_protocol::{ByeCode, RejectCode, Scope};

const FAILED_ATTEMPTS: usize = 20;

/// A frame payload of `length` bytes that is a valid JSON object.
fn padded_ping(length: u32) -> Vec<u8> {
    let length = usize::try_from(length).expect("a frame length fits usize");
    let empty = json!({"type": "req", "id": 1, "op": "ping", "pad": ""}).to_string();
    let padding = "x".repeat(length - empty.len());
    let payload = json!({"type": "req", "id": 1, "op": "ping", "pad": padding}).to_string();
    assert_eq!(payload.len(), length);
    payload.into_bytes()
}

#[tokio::test]
async fn welcome_and_state_follow_the_scopes_of_the_device() {
    let harness = Harness::start().await;
    let reader = harness.pair(&[Scope::StateRead]).await;
    let controller = harness.pair(&[Scope::Control]).await;

    let mut session = harness.connect(&reader).await;
    assert_eq!(session.welcome().scopes, [Scope::StateRead]);
    assert_eq!(
        session.welcome().ops,
        ["ping", "state.subscribe", "icon.get"]
    );
    let (_rev, desktop) = support::online_desktop(&mut session).await;
    assert_eq!(desktop.windows.len(), 2);
    assert!(desktop.windows.iter().all(|window| window.title.is_none()));
    assert_eq!(desktop.windows[0].app_id.as_deref(), Some("notes"));

    let mut session = harness.connect(&controller).await;
    assert_eq!(session.welcome().scopes, [Scope::Control]);
    let actions = ["workspace.focus", "window.focus", "window.fullscreen"];
    assert_eq!(session.welcome().ops, [&["ping"][..], &actions].concat());
    assert_eq!(session.welcome().topics, Vec::<String>::new());
    let subscribe = Args::StateSubscribe {
        topics: vec!["desktop".to_owned()],
    };
    assert!(matches!(
        session.send(&subscribe).await,
        Err(ClientError::NotOffered("state.subscribe"))
    ));
    let request = json!({"type": "req", "id": 7, "op": "state.subscribe",
        "args": {"topics": ["desktop"]}});
    let refused = support::raw_exchange(&mut session, request.to_string().as_bytes()).await;
    assert_eq!((&refused["id"], &refused["ok"]), (&json!(7), &json!(false)));
    assert_eq!(refused["error"]["code"], "forbidden");
    assert_eq!(refused["error"]["detail"], json!({"scope": "state.read"}));
}

#[tokio::test]
async fn wrong_proofs_are_refused_alike_and_never_lock_the_device_out() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut wrong_key = profile.clone();
    wrong_key.key[0] ^= 1;
    let mut unknown_device = profile.clone();
    unknown_device.device_id = swayconnect_protocol::DeviceId::from_bytes([9; 16]);

    for _attempt in 0..FAILED_ATTEMPTS {
        let outcome = soon("auth", connector().connect(&wrong_key)).await;
        assert_rejected(outcome, RejectCode::AuthFailed);
    }
    let outcome = soon("auth", connector().connect(&unknown_device)).await;
    assert_rejected(outcome, RejectCode::AuthFailed);

    let mut session = harness.connect(&profile).await;
    soon("ping", session.request(&Args::Ping))
        .await
        .expect("pong");
}

#[tokio::test]
async fn a_client_with_another_pinned_fingerprint_reports_a_pin_mismatch() {
    let harness = Harness::start().await;
    let mut profile = harness.pair(&EVERY_SCOPE).await;
    profile.fp[0] ^= 1;

    let outcome = soon("connect", connector().connect(&profile)).await;

    assert!(matches!(outcome, Err(ClientError::PinMismatch)));
}

#[tokio::test]
async fn a_proof_over_another_fingerprint_is_refused() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    // What a device computes when a relay shows it another certificate (spec 15.11).
    let relay_fingerprint = [0; 32];
    let mut client = wire::connect(harness.address()).await;
    let context = client.hello(relay_fingerprint).await;

    client
        .send(&wire::auth(&profile.device_id, &profile.key, &context))
        .await;

    let reject = soon("reject", client.receive()).await;
    assert_eq!(reject["type"], "reject");
    assert_eq!(reject["code"], "auth_failed");
    assert_eq!(soon("close", client.next()).await, None);
}

#[tokio::test]
async fn a_replayed_auth_frame_is_refused_on_another_connection() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let fp = harness.daemon.fingerprint();
    let mut first = wire::connect(harness.address()).await;
    let auth = wire::auth(&profile.device_id, &profile.key, &first.hello(fp).await);
    first.send(&auth).await;
    assert_eq!(soon("welcome", first.receive()).await["type"], "welcome");

    let mut second = wire::connect(harness.address()).await;
    // Read `hello`, then answer with the frame that was valid for the first nonce.
    second.hello(fp).await;
    second.send(&auth).await;

    let reject = soon("reject", second.receive()).await;
    assert_eq!(reject["code"], "auth_failed");
}

#[tokio::test]
async fn read_only_is_reported_from_the_config_and_after_the_switch() {
    let configured = Harness::start_with("read_only = true\n").await;
    let profile = configured.pair(&EVERY_SCOPE).await;
    assert!(configured.connect(&profile).await.welcome().read_only);

    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    assert!(!harness.connect(&profile).await.welcome().read_only);
    // What SIGUSR1 calls.
    harness.daemon.enter_read_only();
    assert!(harness.connect(&profile).await.welcome().read_only);
}

#[tokio::test]
async fn a_revoke_between_auth_and_the_first_request_gets_bye_revoked() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;

    harness.revoke(&profile).await;

    let first_request = soon("first request", session.request(&Args::Ping)).await;
    assert_bye(first_request, ByeCode::Revoked);
}

#[tokio::test]
async fn a_revoke_drops_every_session_of_that_device_only() {
    let harness = Harness::start().await;
    let revoked = harness.pair(&EVERY_SCOPE).await;
    let kept = harness.pair(&EVERY_SCOPE).await;
    let mut first = harness.connect(&revoked).await;
    let mut second = harness.connect(&revoked).await;
    let mut other = harness.connect(&kept).await;
    support::online_desktop(&mut first).await;

    harness.revoke(&revoked).await;

    assert_bye(soon("bye", first.next_event()).await, ByeCode::Revoked);
    assert_bye(soon("bye", second.next_event()).await, ByeCode::Revoked);
    soon("ping", other.request(&Args::Ping))
        .await
        .expect("the other device stays connected");
}

#[tokio::test]
async fn a_frame_over_the_limit_ends_the_connection() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;

    let mut session = harness.connect(&profile).await;
    let pong = support::raw_exchange(&mut session, &padded_ping(MAX_FRAME_C2S_BYTES)).await;
    assert_eq!(pong["ok"], true);
    let bye = support::raw_exchange(&mut session, &padded_ping(MAX_FRAME_C2S_BYTES + 1)).await;
    assert_eq!(
        (&bye["type"], &bye["code"]),
        (&json!("bye"), &json!("too_large"))
    );

    let mut client = wire::connect(harness.address()).await;
    client.hello(harness.daemon.fingerprint()).await;
    let length = MAX_FRAME_C2S_BEFORE_WELCOME_BYTES + 1;
    client.send_bytes(&length.to_be_bytes()).await;
    let reject = soon("reject", client.receive()).await;
    assert_eq!(
        (&reject["type"], &reject["code"]),
        (&json!("reject"), &json!("bad_message"))
    );
    assert_eq!(soon("close", client.next()).await, None);
}
