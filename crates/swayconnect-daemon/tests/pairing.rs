//! Pairing against a running daemon: the owner's answer, the deadline, the window
//! (`docs/PROTOCOL.md` 5.4).

mod support;

use std::time::Duration;

use serde_json::json;
use support::{EVERY_SCOPE, Harness, assert_bye, assert_rejected, connector, soon, wire};
use swayconnect_daemon::admin::PairingResult;
use swayconnect_protocol::limits::PAIRING_CONFIRMATION_DEADLINE_MS;
use swayconnect_protocol::proof::device_key;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{ByeCode, DeviceId, RejectCode, Scope};
use tokio::time::Instant;

const WINDOW_LIFETIME: Duration = Duration::from_secs(300);
const SHORTEST_WINDOW_LIFETIME: Duration = Duration::from_secs(30);
const CONFIRMATION_DEADLINE: Duration = Duration::from_millis(PAIRING_CONFIRMATION_DEADLINE_MS);

async fn stored_devices(harness: &Harness) -> usize {
    let devices = soon("devices", harness.admin().await.devices()).await;
    devices.expect("device list").len()
}

async fn pair_with(uri: &PairingUri) -> Result<(), swayconnect_cli::client::ClientError> {
    let connector = connector();
    let pairing = connector.pair(uri, "second phone", None);
    soon("pair", pairing).await.map(drop)
}

#[tokio::test]
async fn a_no_ends_the_pairing_with_bye_revoked_and_stores_nothing() {
    let harness = Harness::start().await;
    let mut pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let pending = pairing.present_secret().await;

    pairing.answer(false).await;

    assert_bye(soon("bye", pending.confirmed()).await, ByeCode::Revoked);
    assert_eq!(pairing.result().await, PairingResult::Refused);
    assert_eq!(stored_devices(&harness).await, 0);
}

#[tokio::test]
async fn no_answer_within_the_deadline_ends_the_pairing_with_bye_revoked() {
    let harness = Harness::start().await;
    let mut pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let pending = pairing.present_secret().await;

    // Paused from here: the clock jumps to the keepalive ping, then to the deadline.
    tokio::time::pause();
    let waiting_since = Instant::now();
    let outcome = tokio::time::timeout(2 * CONFIRMATION_DEADLINE, pending.confirmed()).await;
    let waited = waiting_since.elapsed();
    tokio::time::resume();

    assert_bye(
        outcome.expect("the server ends the pairing"),
        ByeCode::Revoked,
    );
    // The deadline runs from `welcome`, which was sent shortly before the pause.
    let margin = Duration::from_secs(5);
    assert!(
        (CONFIRMATION_DEADLINE - margin..CONFIRMATION_DEADLINE + margin).contains(&waited),
        "waited {waited:?}"
    );
    assert_eq!(pairing.result().await, PairingResult::DeadlinePassed);
    assert_eq!(stored_devices(&harness).await, 0);
}

#[tokio::test]
async fn a_pending_device_can_only_ping_and_cannot_authenticate() {
    let harness = Harness::start().await;
    let fp = harness.daemon.fingerprint();
    let mut pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let mut client = wire::connect(harness.address()).await;
    let context = client.hello(fp).await;
    client
        .send(&wire::pair(&pairing.uri.secret, &context))
        .await;

    let welcome = soon("welcome", client.receive()).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["scopes"], json!([]));
    assert_eq!(welcome["ops"], json!(["ping"]));
    assert_eq!(welcome["topics"], json!([]));
    let shown = soon("pending device", pairing.admin.pending_device()).await;
    let shown = shown.expect("admin answer").expect("a pending device");
    assert_eq!(welcome["confirm_code"], shown.confirm_code.as_str());

    client
        .send(&json!({"type": "req", "id": 1, "op": "ping"}))
        .await;
    assert_eq!(soon("pong", client.receive()).await["ok"], true);
    let subscribe = json!({"type": "req", "id": 2, "op": "state.subscribe",
        "args": {"topics": ["desktop"]}});
    client.send(&subscribe).await;
    let refused = soon("refusal", client.receive()).await;
    assert_eq!(refused["error"]["code"], "forbidden");
    assert_eq!(refused["error"]["detail"], json!({"scope": "state.read"}));

    let device_id = welcome["device_id"].as_str().expect("device_id");
    let device_id = DeviceId::parse(device_id).expect("a device id");
    let key = device_key(&pairing.uri.secret, &context);
    let mut second = wire::connect(harness.address()).await;
    let second_context = second.hello(fp).await;
    second
        .send(&wire::auth(&device_id, &key, &second_context))
        .await;
    let reject = soon("reject", second.receive()).await;
    assert_eq!(
        (&reject["type"], &reject["code"]),
        (&json!("reject"), &json!("auth_failed"))
    );

    pairing.answer(true).await;
    let confirmed = soon("pairing.confirmed", client.receive()).await;
    assert_eq!(confirmed["event"], "pairing.confirmed");
    let mut third = wire::connect(harness.address()).await;
    let third_context = third.hello(fp).await;
    third
        .send(&wire::auth(&device_id, &key, &third_context))
        .await;
    assert_eq!(soon("welcome", third.receive()).await["type"], "welcome");
}

#[tokio::test]
async fn a_bad_proof_leaves_the_window_open_and_a_used_secret_is_dead() {
    let harness = Harness::start().await;
    let mut pairing = harness
        .open_window(&[Scope::StateRead], WINDOW_LIFETIME)
        .await;
    let mut guessed = pairing.uri.clone();
    guessed.secret[0] ^= 1;

    for _attempt in 0..3 {
        assert_rejected(pair_with(&guessed).await, RejectCode::PairingFailed);
    }

    let pending = pairing.present_secret().await;
    // The window is used up by the first correct proof, before the owner answers.
    assert_rejected(pair_with(&pairing.uri).await, RejectCode::PairingFailed);
    pairing.answer(true).await;
    let profile = soon("pairing.confirmed", pending.confirmed()).await;
    let profile = profile.expect("pairing confirmed");
    assert_rejected(pair_with(&pairing.uri).await, RejectCode::PairingFailed);

    let session = harness.connect(&profile).await;
    assert_eq!(session.welcome().scopes, [Scope::StateRead]);
    assert_eq!(stored_devices(&harness).await, 1);
}

#[tokio::test]
async fn an_expired_window_refuses_the_secret() {
    let harness = Harness::start().await;
    let mut pairing = harness
        .open_window(&EVERY_SCOPE, SHORTEST_WINDOW_LIFETIME)
        .await;

    tokio::time::pause();
    tokio::time::sleep(SHORTEST_WINDOW_LIFETIME + Duration::from_secs(1)).await;
    tokio::time::resume();

    assert_rejected(pair_with(&pairing.uri).await, RejectCode::PairingFailed);
    let pending = soon("window expired", pairing.admin.pending_device()).await;
    assert_eq!(pending.expect("admin answer"), None);
    assert_eq!(stored_devices(&harness).await, 0);
}

#[tokio::test]
async fn pairing_without_a_window_fails_like_a_bad_proof() {
    let harness = Harness::start().await;
    let pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let uri = pairing.uri.clone();
    // Closing the admin connection cancels the window.
    drop(pairing);
    // The admin server has seen the close once it answers a later request.
    assert_eq!(stored_devices(&harness).await, 0);

    assert_rejected(pair_with(&uri).await, RejectCode::PairingFailed);
}
