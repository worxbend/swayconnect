//! Deadlines, caps, the source filter at the listener and the action bucket
//! (`docs/PROTOCOL.md` 14, `docs/DESIGN.md` 4.6 steps 1 to 3).

mod support;

use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

use serde_json::json;
use support::logs::CapturedLog;
use support::{EVERY_SCOPE, Harness, soon, wire};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::security::{Refused, Security};
use swayconnect_protocol::limits::{
    ACTION_BURST, ACTION_REFILL_PER_SECOND, CONNECTION_BURST, CONNECTION_REFILL_INTERVAL_MS,
    FIRST_FRAME_DEADLINE_MS, MAX_CONNECTIONS_BEFORE_WELCOME, TLS_HANDSHAKE_DEADLINE_MS,
};
use swayconnect_protocol::ops::Args;
use swayconnect_testkit::TestDirs;
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::net::TcpStream;
use tokio::time::Instant;

const FLOOD_SOURCE: ([u8; 4], u16) = ([127, 0, 0, 2], 0);
const CLOCK_MARGIN: Duration = Duration::from_millis(500);
const FLOOD_CONNECTIONS: usize = 4;
const ACTIONS_PER_CONNECTION: u64 = 5;
const NO_LAN_VARIABLE: &str = "SWAYCONNECT_TEST_NO_LAN";
/// Far more than one log interval may hold, and done long before the interval is over.
const LOG_FLOOD: u64 = 50;

/// A non-loopback IPv4 address of this machine, when it has one. Connecting a UDP socket
/// only selects a route; no packet is sent.
fn routed_address() -> Option<IpAddr> {
    let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    let address = probe.local_addr().ok()?.ip();
    (!address.is_loopback() && !address.is_unspecified()).then_some(address)
}

/// The address the `allow_from` tests connect from. Loopback is always allowed, so these
/// tests need another address of this machine. Without one the test fails; only a run
/// that sets `SWAYCONNECT_TEST_NO_LAN=1` may skip it, and then says so.
fn lan_address(test: &str) -> Option<IpAddr> {
    let address = routed_address();
    if address.is_none() {
        let is_skip_allowed = std::env::var_os(NO_LAN_VARIABLE).is_some_and(|set| set == "1");
        assert!(
            is_skip_allowed,
            "{test} needs a non-loopback IPv4 address and this machine has none; \
             set {NO_LAN_VARIABLE}=1 to skip it"
        );
        eprintln!("SKIPPED {test}: {NO_LAN_VARIABLE}=1 and no non-loopback IPv4 address");
    }
    address
}

/// How long the server takes to close `stream` when the client stays silent, on a paused
/// clock. Panics when the server sends anything.
async fn silence_until_close(stream: &mut (impl AsyncRead + Unpin)) -> Duration {
    tokio::time::pause();
    let silent_since = Instant::now();
    let mut received = [0u8; 64];
    let read = tokio::time::timeout(Duration::from_secs(60), stream.read(&mut received)).await;
    let waited = silent_since.elapsed();
    tokio::time::resume();
    match read.expect("the server closes a silent connection") {
        // Without `close_notify` the TLS layer reports the close as an error.
        Ok(0) | Err(_) => waited,
        Ok(length) => panic!("the server sent {length} bytes before closing"),
    }
}

fn assert_close_to(waited: Duration, deadline_ms: u64) {
    let deadline = Duration::from_millis(deadline_ms);
    assert!(
        (deadline - CLOCK_MARGIN..deadline + CLOCK_MARGIN).contains(&waited),
        "closed after {waited:?}, deadline {deadline:?}"
    );
}

/// `true` when the server lets a connection from `source` reach `hello`.
async fn reaches_hello(harness: &Harness, source: IpAddr) -> bool {
    let from = SocketAddr::new(source, 0);
    let stream = soon("TCP connect", wire::tcp(harness.address(), Some(from))).await;
    let stream = stream.expect("TCP connection from a local address");
    match soon("TLS handshake", wire::tls(stream)).await {
        Ok(mut client) => soon("hello", client.next()).await.is_some(),
        Err(_closed_before_tls) => false,
    }
}

#[tokio::test]
async fn a_client_that_sends_no_first_frame_is_closed_after_2_s_with_no_frame() {
    let harness = Harness::start().await;
    let mut client = wire::connect(harness.address()).await;
    client.hello(harness.daemon.fingerprint()).await;

    let waited = silence_until_close(client.stream_mut()).await;

    assert_close_to(waited, FIRST_FRAME_DEADLINE_MS);
}

#[tokio::test]
async fn a_connection_that_never_starts_tls_is_closed_after_5_s() {
    let harness = Harness::start().await;
    let mut stream = wire::tcp(harness.address(), None).await.expect("TCP");

    let waited = silence_until_close(&mut stream).await;

    assert_close_to(waited, TLS_HANDSHAKE_DEADLINE_MS);
}

#[tokio::test]
async fn a_flood_of_silent_connections_evicts_the_oldest_and_does_not_stop_an_auth() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut silent: Vec<TcpStream> = Vec::new();
    for _connection in 0..MAX_CONNECTIONS_BEFORE_WELCOME {
        let stream = wire::tcp(harness.address(), Some(FLOOD_SOURCE.into())).await;
        silent.push(stream.expect("TCP connection"));
    }

    let mut session = harness.connect(&profile).await;
    soon("ping", session.request(&Args::Ping))
        .await
        .expect("pong");

    let mut received = [0u8; 1];
    let oldest = soon("eviction", silent[0].read(&mut received)).await;
    assert!(
        matches!(oldest, Ok(0) | Err(_)),
        "the oldest is closed with no byte"
    );
    for younger in &silent[1..] {
        let still_open = younger.try_read(&mut received);
        assert_eq!(
            still_open.expect_err("no byte, no close").kind(),
            ErrorKind::WouldBlock
        );
    }
}

#[tokio::test]
async fn only_loopback_and_allow_from_sources_get_past_the_accept() {
    let Some(lan) = lan_address("the allow_from test") else {
        return;
    };
    let closed = Harness::start().await;
    assert!(!reaches_hello(&closed, lan).await);
    assert!(reaches_hello(&closed, IpAddr::from([127, 0, 0, 1])).await);

    let open = Harness::start_with(&format!("allow_from = [\"{lan}/32\"]\n")).await;
    assert!(reaches_hello(&open, lan).await);
}

#[tokio::test]
async fn the_connection_bucket_refuses_a_burst_and_refills_without_lockout() {
    let Some(lan) = lan_address("the connection bucket test") else {
        return;
    };
    let harness = Harness::start_with(&format!("allow_from = [\"{lan}/32\"]\n")).await;

    for _connection in 0..CONNECTION_BURST {
        assert!(reaches_hello(&harness, lan).await);
    }
    for _connection in 0..CONNECTION_BURST {
        assert!(!reaches_hello(&harness, lan).await, "the bucket is empty");
    }
    // Loopback has no bucket (spec 14 note 6).
    assert!(reaches_hello(&harness, IpAddr::from([127, 0, 0, 1])).await);

    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(CONNECTION_REFILL_INTERVAL_MS)).await;
    tokio::time::resume();

    assert!(reaches_hello(&harness, lan).await, "one token came back");
    assert!(!reaches_hello(&harness, lan).await);
}

#[tokio::test]
async fn twenty_actions_at_once_on_four_connections_get_the_burst_through_and_no_more() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut sessions = Vec::new();
    for _connection in 0..FLOOD_CONNECTIONS {
        sessions.push(harness.connect(&profile).await);
    }
    // The snapshot must hold window 6 before it can be focused.
    support::online_desktop(&mut sessions[0]).await;
    let action_refill_ms = u64::from(1000 / ACTION_REFILL_PER_SECOND);

    // Every request is on the wire before the first answer is read.
    let flood_started = std::time::Instant::now();
    for id in 1..=ACTIONS_PER_CONNECTION {
        let focus = support::request_frame(100 + id, "window.focus", &json!({"window_id": 6}));
        for session in &mut sessions {
            let sent = soon("request", session.send_raw_frame(&focus)).await;
            sent.expect("frame sent");
        }
    }
    let (mut succeeded, mut limited) = (0u64, 0u64);
    for session in &mut sessions {
        for _answer in 0..ACTIONS_PER_CONNECTION {
            let answer = soon("answer", session.receive_raw_frame()).await;
            let answer = answer.expect("an answer frame");
            if answer["ok"] == true {
                succeeded += 1;
                continue;
            }
            assert_eq!(answer["error"]["code"], "rate_limited", "{answer:?}");
            let retry_after_ms = answer["error"]["detail"]["retry_after_ms"].as_u64();
            let retry_after_ms = retry_after_ms.expect("retry_after_ms");
            assert!(
                (1..=action_refill_ms).contains(&retry_after_ms),
                "retry after {retry_after_ms} ms"
            );
            limited += 1;
        }
    }
    let flood_ms = u64::try_from(flood_started.elapsed().as_millis()).expect("a short test");

    // The bucket is per device, not per connection. It refills while the flood runs, so a
    // run that takes longer than one refill interval may see one more success for each.
    let burst = u64::from(ACTION_BURST);
    let allowed = burst + flood_ms / action_refill_ms;
    assert!(
        (burst..=allowed).contains(&succeeded),
        "{succeeded} successes in {flood_ms} ms"
    );
    assert_eq!(succeeded + limited, 20);
    let reached_sway = harness.sway().commands();
    assert_eq!(u64::try_from(reached_sway.len()), Ok(succeeded));
    assert!(reached_sway.iter().all(|sent| sent == "[con_id=6] focus"));
}

#[tokio::test]
async fn a_flood_of_refused_connections_writes_one_log_line() {
    let dirs = TestDirs::new();
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let config = Config::from_toml("listen = [\"127.0.0.1:0\"]").expect("config");
    let security = Security::open(&config, &paths).expect("security");
    let outsider = IpAddr::from([203, 0, 113, 7]);
    let log = CapturedLog::start();

    for _connection in 0..LOG_FLOOD {
        assert_eq!(security.admit(outsider).err(), Some(Refused::NotAllowed));
    }

    assert_eq!(log.lines_with("connection refused"), 1);
    assert_eq!(log.lines_with("not in allow_from"), 1);
}

#[tokio::test]
async fn a_flood_of_refused_handshakes_writes_one_log_line() {
    let harness = Harness::start().await;
    let log = CapturedLog::start();

    for _connection in 0..LOG_FLOOD {
        let mut client = wire::connect(harness.address()).await;
        soon("hello", client.receive()).await;
        client.send(&json!({"type": "login"})).await;
        let answer = soon("reject", client.receive()).await;
        assert_eq!(answer["code"], "bad_message");
    }

    assert_eq!(log.lines_with("handshake refused"), 1);
}

#[tokio::test]
async fn a_flood_of_rate_limited_actions_writes_one_log_line() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    let log = CapturedLog::start();

    let mut limited = 0;
    for id in 0..LOG_FLOOD {
        // No target: the request takes a token and reaches sway with nothing.
        let no_target = support::request_frame(id, "workspace.focus", &json!({}));
        let answer = support::raw_exchange(&mut session, &no_target).await;
        limited += u64::from(answer["error"]["code"] == "rate_limited");
    }

    assert!(limited > LOG_FLOOD / 2, "{limited} requests were limited");
    assert_eq!(log.lines_with("action refused: rate limit"), 1);
}
