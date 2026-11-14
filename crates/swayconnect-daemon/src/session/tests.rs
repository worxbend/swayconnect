//! Tests of `session.rs` over an in-memory stream. No TLS, no sway, no TCP.

use serde_json::{Value, json};
use swayconnect_protocol::Pair;

use super::harness::*;
use super::wire::LENGTH_PREFIX_BYTES;
use super::*;

#[tokio::test]
async fn hello_is_the_first_frame_and_holds_the_version_range_and_a_nonce_only() {
    let daemon = Daemon::start().await;
    let hello = daemon.connect().receive().await;
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["protocol"], json!({"min": 1, "max": 1}));
    assert_eq!(hello.len(), 3);
    let other_nonce = daemon.connect().receive().await["nonce"].clone();
    assert_ne!(hello["nonce"], other_nonce);
}

#[tokio::test]
async fn auth_gets_welcome_with_scopes_ops_topics_and_limits() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (_client, welcome) = daemon.ready(&device).await;
    assert_eq!(welcome["device_id"], device.id.as_str());
    assert_eq!(
        welcome["scopes"],
        json!([
            "state.read",
            "state.titles",
            "control",
            "idle.inhibit",
            "destructive"
        ])
    );
    let ops = json!([
        "ping",
        "state.subscribe",
        "workspace.focus",
        "window.focus",
        "window.fullscreen",
        "icon.get",
        "window.move",
        "window.floating",
        "window.layout",
        "window.scratchpad",
        "scratchpad.show",
        "session.lock",
        "window.close",
        "keep_awake.set"
    ]);
    assert_eq!(welcome["ops"], ops);
    let m2b_ops = [
        "audio.set_volume",
        "audio.set_mute",
        "audio.set_default",
        "media.play_pause",
        "media.next",
        "media.previous",
        "app.launch",
        "launcher.open",
        "input.text",
        "input.key",
    ];
    let mut known_ops = ops.as_array().expect("ops").clone();
    known_ops.extend(m2b_ops.map(Value::from));
    assert_eq!(welcome["known_ops"], Value::Array(known_ops));
    let known_scopes = json!([
        "state.read",
        "state.titles",
        "control",
        "idle.inhibit",
        "destructive",
        "audio",
        "audio.capture",
        "media",
        "launch",
        "launch.risky",
        "input.keys",
        "input.text"
    ]);
    assert_eq!(welcome["known_scopes"], known_scopes);
    assert_eq!(welcome["topics"], json!(["desktop", "session"]));
    assert_eq!(welcome["read_only"], false);
    assert_eq!(welcome["server"]["name"], "desk");
    let limits =
        json!({"max_frame_c2s": 16384, "max_frame_s2c": 1_048_576, "idle_timeout_ms": 90000});
    assert_eq!(welcome["limits"], limits);
    assert!(!welcome.contains_key("confirm_code"));
}

#[tokio::test]
async fn bytes_after_the_first_frame_and_before_welcome_are_refused() {
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    client.send_bytes(b"\x00\x00\x00\x02[]").await;
    client.expect_last("bye", "bad_frame").await;
}

#[tokio::test]
async fn pair_auth_and_a_frame_without_type_after_welcome_are_protocol_errors() {
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
    let mut pending = daemon.pair(&EVERY_SCOPE).await;
    pending.notice.answer.send(false).expect("answer taken");
    pending.client.expect_last("bye", "revoked").await;
}

#[tokio::test]
async fn a_device_without_state_titles_gets_every_title_as_null() {
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
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
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    let was_stored = daemon.shared.security.revoke(&device.id).expect("revoke");
    assert!(was_stored);
    client.expect_last("bye", "revoked").await;
}

#[tokio::test]
async fn shutdown_sends_bye_shutdown() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    daemon.shutdown.send_replace(true);
    client.expect_last("bye", "shutdown").await;
}

#[tokio::test(start_paused = true)]
async fn an_idle_connection_gets_bye_idle_and_every_frame_resets_the_timer() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    tokio::time::sleep(IDLE_TIMEOUT - Duration::from_secs(1)).await;
    client.send(&ping(1)).await;
    assert_eq!(client.receive().await["ok"], true);
    let last_frame_at = Instant::now();
    client.expect_last("bye", "idle").await;
    assert_eq!(last_frame_at.elapsed(), IDLE_TIMEOUT);
}
