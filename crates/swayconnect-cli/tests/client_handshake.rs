//! The client library against an in-test rustls server that presents the certificate and keys
//! of `protocol/vectors/v1/fingerprint.json` and plays the transcripts of `docs/PROTOCOL.md` 15.

mod client_support;

use std::time::Duration;

use client_support::{
    Server, Step, auth_connector, connect, good_config, pairing_connector, pairing_uri, parse,
    profile, server_config, transcript, vector_fp, with_member,
};
use serde_json::{Value, json};
use swayconnect_cli::client::ClientError;
use swayconnect_protocol::limits::ALPN_ID;
use swayconnect_protocol::{ByeCode, DeviceId};
use tokio::sync::oneshot;

#[tokio::test]
async fn wrong_fingerprint_is_a_pin_error_and_no_application_byte_is_sent() {
    let server = Server::start(good_config(), vec![Step::Receive]).await;
    let other_pin = [0x11; 32];

    let outcome = auth_connector()
        .connect(&profile(server.port, other_pin))
        .await;

    assert!(matches!(outcome, Err(ClientError::PinMismatch)));
    assert!(
        server.received().await.is_err(),
        "the TLS handshake must not complete"
    );
}

#[tokio::test]
async fn pinned_certificate_with_the_wrong_key_fails_in_the_tls_handshake() {
    let config = server_config("wrong_key_pkcs8_b64", &[ALPN_ID]);
    let server = Server::start(config, vec![Step::Receive]).await;

    // A client that accepts this server would wait for `hello` without end.
    let outcome = tokio::time::timeout(Duration::from_secs(10), connect(&server))
        .await
        .expect("the client must fail in the handshake, not continue");

    let Err(ClientError::Tls(tls_error)) = outcome else {
        panic!("expected a TLS handshake failure");
    };
    assert_eq!(
        tls_error,
        rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature)
    );
    assert!(
        server.received().await.is_err(),
        "the TLS handshake must not complete"
    );
}

#[tokio::test]
async fn server_that_selects_no_alpn_makes_the_client_abort() {
    let config = server_config("key_pkcs8_b64", &[]);
    let server = Server::start(config, Vec::new()).await;

    let outcome = connect(&server).await;

    assert!(matches!(outcome, Err(ClientError::NoAlpn)));
    let received = server.received().await.expect("handshake completes");
    assert_eq!(received.bytes, 0);
}

#[tokio::test]
async fn a_hello_without_version_1_makes_the_client_close_without_a_frame() {
    let t2_hello = &transcript("t2-auth-ok")[0];
    let newer_versions = with_member(t2_hello, "protocol", json!({"min": 2, "max": 3}));
    let server = Server::start(good_config(), vec![Step::Send(newer_versions)]).await;

    let outcome = connect(&server).await;

    assert!(matches!(
        outcome,
        Err(ClientError::NoCommonVersion { min: 2, max: 3 })
    ));
    let received = server.received().await.expect("handshake completes");
    assert_eq!(received.bytes, 0, "spec 5.6 rule 3: no frame is sent");
}

#[tokio::test]
async fn t1_pairing_emits_the_vector_frame_and_gives_the_profile_only_after_confirmation() {
    let t1 = transcript("t1-pair-ok");
    let (owner_says_yes, owner_answer) = oneshot::channel();
    let script = vec![
        Step::Send(t1[0].clone()),
        Step::Receive,
        Step::Send(t1[2].clone()),
        Step::WaitFor(owner_answer),
        Step::Send(t1[3].clone()),
    ];
    let server = Server::start(good_config(), script).await;

    let pending = pairing_connector()
        .pair(&pairing_uri(server.port), "Pixel 9", Some("android"))
        .await
        .expect("T1 handshake");
    assert_eq!(pending.confirm_code(), "261988");

    let mut confirmed = Box::pin(pending.confirmed());
    let before_the_answer = tokio::time::timeout(Duration::from_millis(100), &mut confirmed).await;
    assert!(
        before_the_answer.is_err(),
        "no profile before `pairing.confirmed`"
    );
    owner_says_yes.send(()).expect("server waits");
    let stored = confirmed.await.expect("pairing confirmed");

    assert_eq!(stored, profile(server.port, vector_fp()));
    let received = server.received().await.expect("handshake completes");
    assert_eq!(received.frames, vec![parse(&t1[1])]);
}

#[tokio::test]
async fn t7_bye_revoked_ends_the_pairing_without_a_profile() {
    let t7 = transcript("t7-pair-refused");
    let script = vec![
        Step::Send(t7[0].clone()),
        Step::Receive,
        Step::Send(t7[2].clone()),
        Step::Send(t7[3].clone()),
    ];
    let server = Server::start(good_config(), script).await;

    let pending = pairing_connector()
        .pair(&pairing_uri(server.port), "Pixel 9", Some("android"))
        .await
        .expect("T7 handshake");
    let outcome = pending.confirmed().await;

    assert!(matches!(
        outcome,
        Err(ClientError::Bye {
            code: ByeCode::Revoked,
            ..
        })
    ));
    assert_eq!(
        server
            .received()
            .await
            .expect("handshake completes")
            .frames
            .len(),
        1
    );
}

#[tokio::test]
async fn pairing_with_a_wrong_server_proof_closes_and_gives_nothing() {
    let t1 = transcript("t1-pair-ok");
    let later_proof = parse(&transcript("t2-auth-ok")[2])["server_proof"].clone();
    let forged_welcome = with_member(&t1[2], "server_proof", later_proof);
    let script = vec![
        Step::Send(t1[0].clone()),
        Step::Receive,
        Step::Send(forged_welcome),
    ];
    let server = Server::start(good_config(), script).await;

    let outcome = pairing_connector()
        .pair(&pairing_uri(server.port), "Pixel 9", Some("android"))
        .await;

    assert!(matches!(
        outcome,
        Err(ClientError::BadWelcome("server_proof"))
    ));
    assert_eq!(
        server
            .received()
            .await
            .expect("handshake completes")
            .frames
            .len(),
        1
    );
}

/// Plays T2 with one changed `welcome` member and returns the client's error.
async fn auth_against_changed_welcome(name: &str, value: Value) -> ClientError {
    let t2 = transcript("t2-auth-ok");
    let changed_welcome = with_member(&t2[2], name, value);
    let script = vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(changed_welcome),
    ];
    let server = Server::start(good_config(), script).await;

    let outcome = connect(&server).await;

    let received = server.received().await.expect("handshake completes");
    assert_eq!(
        received.frames,
        vec![parse(&t2[1])],
        "only the T2 `auth` frame is sent"
    );
    outcome
        .err()
        .expect("the client must refuse this `welcome`")
}

#[tokio::test]
async fn auth_with_a_wrong_server_proof_closes() {
    let pairing_proof = parse(&transcript("t1-pair-ok")[2])["server_proof"].clone();
    let error = auth_against_changed_welcome("server_proof", pairing_proof).await;
    assert!(matches!(error, ClientError::BadWelcome("server_proof")));
}

#[tokio::test]
async fn auth_with_another_device_id_in_welcome_closes() {
    let other_id = DeviceId::from_bytes([9; 16]);
    let error = auth_against_changed_welcome("device_id", other_id.as_str().into()).await;
    assert!(matches!(error, ClientError::BadWelcome("device_id")));
}

#[tokio::test]
async fn auth_with_another_protocol_in_welcome_closes() {
    let error = auth_against_changed_welcome("protocol", 2.into()).await;
    assert!(matches!(error, ClientError::BadWelcome("protocol")));
}
