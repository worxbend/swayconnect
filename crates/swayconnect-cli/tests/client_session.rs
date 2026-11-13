//! The message layer of the client library after transcript T2, against the in-test server.

#[allow(dead_code, reason = "this file uses a part of the shared support")]
mod client_support;

use std::time::Duration;

use client_support::{Server, Step, connect, good_config, transcript, with_member};
use serde_json::{Value, json};
use swayconnect_cli::client::{ClientError, Event, Session};
use swayconnect_protocol::limits::{CLIENT_KEEPALIVE_INTERVAL_MS, CLIENT_REQUEST_TIMEOUT_MS};
use swayconnect_protocol::ops::Args;
use swayconnect_protocol::{ByeCode, ErrorCode};

/// Transcript T2, then `after_welcome`.
async fn session(after_welcome: Vec<Step>) -> (Session, Server) {
    let t2 = transcript("t2-auth-ok");
    let mut script = vec![
        Step::Send(t2[0].clone()),
        Step::Receive,
        Step::Send(t2[2].clone()),
    ];
    script.extend(after_welcome);
    let server = Server::start(good_config(), script).await;
    let session = connect(&server).await.expect("T2 handshake");
    (session, server)
}

fn res(id: u64, result: Value) -> Step {
    Step::Send(json!({"type": "res", "id": id, "ok": true, "result": result}).to_string())
}

fn desktop_state(rev: u64) -> Value {
    json!({"topic": "desktop", "rev": rev, "data": {"marker": rev}})
}

fn state_changed(rev: u64) -> Step {
    let event = json!({"type": "evt", "event": "state.changed", "data": desktop_state(rev)});
    Step::Send(event.to_string())
}

fn rev_of(event: &Event) -> u64 {
    match event {
        Event::State(state) => state.rev,
        Event::PairingConfirmed => panic!("not a state event"),
    }
}

#[tokio::test]
async fn responses_are_matched_by_id_after_an_event_and_in_swapped_order() {
    let script = vec![
        Step::Receive,
        res(1, json!({"states": [desktop_state(7)]})),
        Step::Receive,
        Step::Receive,
        state_changed(8),
        res(3, json!({"answer_to": 3})),
        res(2, json!({"answer_to": 2})),
    ];
    let (mut session, server) = session(script).await;
    session.subscribe(&["desktop"]).await.expect("subscribe");

    let second = session.send(&Args::Ping).await.expect("request 2");
    let third = session.send(&Args::Ping).await.expect("request 3");
    let second_result = session.response(second).await.expect("response 2");
    let third_result = session.response(third).await.expect("response 3");

    assert_eq!(Value::Object(second_result), json!({"answer_to": 2}));
    assert_eq!(Value::Object(third_result), json!({"answer_to": 3}));
    assert_eq!(rev_of(&session.next_event().await.expect("event")), 8);
    session.close().await.expect("close");
    assert_eq!(
        server
            .received()
            .await
            .expect("handshake completes")
            .frames
            .len(),
        4
    );
}

#[tokio::test]
async fn a_state_with_a_lower_or_equal_rev_is_dropped() {
    let script = vec![
        Step::Receive,
        res(1, json!({"states": [desktop_state(7)]})),
        state_changed(7),
        state_changed(6),
        state_changed(8),
        Step::Receive,
        res(2, json!({"states": [desktop_state(8)]})),
        Step::Receive,
        res(3, json!({"states": [desktop_state(9)]})),
        state_changed(9),
        state_changed(10),
    ];
    let (mut session, _server) = session(script).await;

    let first = session.subscribe(&["desktop"]).await.expect("subscribe");
    assert_eq!(first.iter().map(|state| state.rev).collect::<Vec<_>>(), [7]);
    assert_eq!(rev_of(&session.next_event().await.expect("event")), 8);

    let repeated = session
        .subscribe(&["desktop"])
        .await
        .expect("subscribe again");
    assert!(repeated.is_empty(), "rev 8 is applied already");
    let newer = session
        .subscribe(&["desktop"])
        .await
        .expect("subscribe a third time");
    assert_eq!(newer.iter().map(|state| state.rev).collect::<Vec<_>>(), [9]);
    assert_eq!(rev_of(&session.next_event().await.expect("event")), 10);
}

#[tokio::test]
async fn a_state_of_a_topic_that_was_not_requested_and_unknown_frames_are_ignored() {
    let unknown_type = Step::Send(json!({"type": "future", "x": 1}).to_string());
    let unknown_event = json!({"type": "evt", "event": "future.event", "data": {}});
    let script = vec![
        state_changed(3),
        unknown_type,
        Step::Send(unknown_event.to_string()),
        Step::Receive,
        res(1, json!({"states": [desktop_state(1)]})),
        Step::Send(transcript("t3-session").pop().expect("bye")),
    ];
    let (mut session, _server) = session(script).await;

    let states = session.subscribe(&["desktop"]).await.expect("subscribe");
    assert_eq!(
        states.len(),
        1,
        "the early event with rev 3 was not applied"
    );
    let end = session.next_event().await;
    assert!(matches!(
        end,
        Err(ClientError::Bye {
            code: ByeCode::Shutdown,
            ..
        })
    ));
}

#[tokio::test]
async fn an_error_response_and_an_operation_that_is_not_offered() {
    let t3 = transcript("t3-session");
    let ping_only = with_member(&transcript("t2-auth-ok")[2], "ops", json!(["ping"]));
    let script = vec![
        Step::Send(transcript("t2-auth-ok")[0].clone()),
        Step::Receive,
        Step::Send(ping_only),
        Step::Receive,
        Step::Send(with_member(&t3[6], "id", 1.into())),
    ];
    let server = Server::start(good_config(), script).await;
    let mut session = connect(&server).await.expect("handshake");

    let refused_locally = session.request(&Args::WindowFocus { window_id: 999 }).await;
    assert!(matches!(
        refused_locally,
        Err(ClientError::NotOffered("window.focus"))
    ));
    let failed = session.request(&Args::Ping).await;
    let Err(ClientError::Failed(error)) = failed else {
        panic!("expected the error of the response");
    };
    assert_eq!(error.code, ErrorCode::NotFound);
    drop(session);
    assert_eq!(
        server
            .received()
            .await
            .expect("handshake completes")
            .frames
            .len(),
        2
    );
}

#[tokio::test(start_paused = true)]
async fn a_ping_is_sent_while_the_client_only_waits_for_events() {
    let script = vec![
        Step::Receive,
        res(1, json!({"states": [desktop_state(7)]})),
        Step::Receive,
        res(2, json!({})),
        state_changed(8),
    ];
    let (mut session, server) = session(script).await;
    session.subscribe(&["desktop"]).await.expect("subscribe");

    assert_eq!(rev_of(&session.next_event().await.expect("event")), 8);

    drop(session);
    let frames = server.received().await.expect("handshake completes").frames;
    assert_eq!(frames[2], json!({"type": "req", "id": 2, "op": "ping"}));
}

#[tokio::test(start_paused = true)]
async fn a_keepalive_ping_without_a_response_ends_the_wait_for_events() {
    // The server reads the `ping` and stays silent, as a dead network path would.
    let script = vec![
        Step::Receive,
        res(1, json!({"states": [desktop_state(7)]})),
        Step::Receive,
    ];
    let (mut session, server) = session(script).await;
    session.subscribe(&["desktop"]).await.expect("subscribe");
    let waiting_since = tokio::time::Instant::now();

    // Without the answer deadline the client would ping a dead connection for ever.
    let outcome = tokio::time::timeout(Duration::from_secs(600), session.next_event()).await;
    let outcome = outcome.expect("the client gives up on a dead connection");

    assert!(matches!(outcome, Err(ClientError::NoAnswer)), "{outcome:?}");
    let ping_and_wait =
        Duration::from_millis(CLIENT_KEEPALIVE_INTERVAL_MS + CLIENT_REQUEST_TIMEOUT_MS);
    let waited = waiting_since.elapsed();
    assert!(
        (ping_and_wait..ping_and_wait + Duration::from_secs(1)).contains(&waited),
        "gave up after {waited:?}"
    );
    drop(session);
    let frames = server.received().await.expect("handshake completes").frames;
    assert_eq!(frames.len(), 3, "one `ping`, not one every 30 seconds");
}

#[tokio::test]
async fn raw_frames_go_out_unchecked_and_answers_come_back_undecoded() {
    let answer = json!({"type": "res", "id": 77, "ok": true, "result": {}});
    let script = vec![Step::Receive, Step::Send(answer.to_string())];
    let (mut session, server) = session(script).await;
    let forbidden_request = br#"{"type":"req","id":77,"op":"no.such.op"}"#;

    session
        .send_raw_frame(forbidden_request)
        .await
        .expect("raw send");
    let reply = session.receive_raw_frame().await.expect("raw receive");

    assert_eq!(Value::Object(reply), answer);
    drop(session);
    let frames = server.received().await.expect("handshake completes").frames;
    assert_eq!(
        frames[1],
        json!({"type": "req", "id": 77, "op": "no.such.op"})
    );
}
