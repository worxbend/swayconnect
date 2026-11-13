use super::*;
use crate::command::WorkspaceNumber;
use crate::test_socket::TestSocket;

const VERSION_CODE: u32 = 7;

#[tokio::test]
async fn two_pipelined_requests_are_answered_in_order() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        // Both requests are read before the first reply is written.
        let first = peer.read_request().await;
        let second = peer.read_request().await;
        peer.send(first.0, br#"{"human_readable":"first"}"#).await;
        peer.send(second.0, br#"{"human_readable":"second"}"#).await;
        (first, second)
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        tokio::join!(connection.version(), connection.version())
    };
    let ((first_request, second_request), (first_reply, second_reply)) = tokio::join!(sway, client);

    assert_eq!(first_request, (VERSION_CODE, Vec::new()));
    assert_eq!(second_request, (VERSION_CODE, Vec::new()));
    assert_eq!(first_reply.expect("first reply").human_readable, "first");
    assert_eq!(second_reply.expect("second reply").human_readable, "second");
}

#[tokio::test]
async fn each_query_sends_its_message_type_and_parses_the_reply() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        let mut codes = Vec::new();
        for reply in [
            &br#"[{"id":4,"num":1,"name":"1"}]"#[..],
            br#"{"id":1,"type":"root"}"#,
        ] {
            let (code, payload) = peer.read_request().await;
            assert!(payload.is_empty());
            peer.send(code, reply).await;
            codes.push(code);
        }
        codes
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        let workspaces = connection.workspaces().await.expect("workspaces");
        let tree = connection.tree().await.expect("tree");
        (workspaces, tree)
    };
    let (codes, (workspaces, tree)) = tokio::join!(sway, client);

    assert_eq!(codes, [1, 4]);
    assert_eq!((workspaces[0].id, workspaces[0].num), (4, Some(1)));
    assert_eq!(tree.node_type, "root");
}

#[tokio::test]
async fn an_event_between_request_and_reply_goes_to_the_event_side() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        let (code, _) = peer.read_request().await;
        peer.send(0x8000_0003, br#"{"change":"title"}"#).await;
        peer.send(code, br#"{"human_readable":"1.12"}"#).await;
        peer
    };
    let client = async {
        let mut connection = Connection::connect(socket.path()).await.expect("connect");
        let version = connection.version().await;
        let event = connection.next_event().await;
        (version, event)
    };
    let (_peer, (version, event)) = tokio::join!(sway, client);

    assert_eq!(
        version.expect("the reply, not the event").human_readable,
        "1.12"
    );
    assert_eq!(event.expect("the event"), Event::Window);
}

#[tokio::test(start_paused = true)]
async fn no_reply_is_a_timeout_after_two_seconds_and_closes_the_connection() {
    let socket = TestSocket::bind();
    let connection = Connection::connect(socket.path()).await.expect("connect");
    let mut peer = socket.accept().await;
    let started = tokio::time::Instant::now();

    let (error, _request) = tokio::join!(connection.version(), peer.read_request());

    let error = error.expect_err("sway never replies");
    assert!(matches!(error, Error::Timeout), "{error}");
    assert_eq!(started.elapsed(), REQUEST_TIMEOUT);
    assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(2));

    let error = connection
        .version()
        .await
        .expect_err("out of step, so closed");
    assert!(matches!(error, Error::Closed), "{error}");
}

#[tokio::test]
async fn bad_magic_is_an_error_and_closes_the_connection() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        peer.read_request().await;
        peer.send_raw(b"i3-ipx\0\0\0\0\x07\0\0\0").await;
        peer
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        let first = connection.version().await;
        let second = connection.version().await;
        (first, second)
    };
    let (_peer, (first, second)) = tokio::join!(sway, client);

    let error = first.expect_err("bad magic");
    assert!(matches!(error, Error::BadMagic), "{error}");
    let error = second.expect_err("closed");
    assert!(matches!(error, Error::Closed), "{error}");
}

#[tokio::test]
async fn a_closed_socket_is_an_io_error() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        peer.read_request().await;
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        connection.version().await
    };
    let ((), reply) = tokio::join!(sway, client);

    let error = reply.expect_err("end of stream");
    assert!(matches!(error, Error::Io(_)), "{error}");
}

#[tokio::test]
async fn a_reply_of_another_type_is_an_error() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        peer.read_request().await;
        peer.send(4, b"{}").await;
        peer
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        connection.version().await
    };
    let (_peer, reply) = tokio::join!(sway, client);

    let error = reply.expect_err("wrong type");
    assert!(
        matches!(
            error,
            Error::UnexpectedReply {
                expected: 7,
                got: 4
            }
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_reply_that_is_not_json_is_an_error() {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        let (code, _) = peer.read_request().await;
        peer.send(code, b"not json").await;
        peer
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        connection.version().await
    };
    let (_peer, reply) = tokio::join!(sway, client);

    let error = reply.expect_err("not JSON");
    assert!(matches!(error, Error::Json(_)), "{error}");
}

#[tokio::test]
async fn connecting_to_a_missing_socket_is_an_io_error() {
    let dir = tempfile::tempdir().expect("temp directory");
    let error = Connection::connect(&dir.path().join("none.sock"))
        .await
        .expect_err("no socket");
    assert!(matches!(error, Error::Io(_)), "{error}");
}

/// Sends `command` to an in-test socket that answers `reply`.
/// Returns the request the socket saw and what the client got.
async fn run_against(command: Command, reply: &[u8]) -> ((u32, Vec<u8>), Result<(), Error>) {
    let socket = TestSocket::bind();
    let sway = async {
        let mut peer = socket.accept().await;
        let request = peer.read_request().await;
        peer.send(request.0, reply).await;
        (request, peer)
    };
    let client = async {
        let connection = Connection::connect(socket.path()).await.expect("connect");
        connection.run(command).await
    };
    let ((request, _peer), outcome) = tokio::join!(sway, client);
    (request, outcome)
}

#[tokio::test]
async fn run_sends_the_rendered_command_as_type_0() {
    let number = WorkspaceNumber::new(3).expect("3 is in range");
    let command = Command::FocusWorkspace(number);
    let (request, outcome) = run_against(command, br#"[{"success":true}]"#).await;

    assert_eq!(request.0, 0);
    // The text itself is pinned in `command.rs`, the only file that may spell it.
    assert_eq!(request.1, command.render().into_bytes());
    outcome.expect("one success");
}

#[tokio::test]
async fn run_fails_unless_the_reply_is_exactly_one_success() {
    let command = Command::FocusWindow { con_id: 9 };
    let table: [(&[u8], &str); 4] = [
        (
            br#"[{"success":false,"error":"no such con"}]"#,
            "no such con",
        ),
        (br#"[{"success":false}]"#, ""),
        (br#"[]"#, "0 results for one command"),
        (
            br#"[{"success":true},{"success":true}]"#,
            "2 results for one command",
        ),
    ];
    for (reply, expected) in table {
        let (_request, outcome) = run_against(command, reply).await;
        match outcome {
            Err(Error::CommandFailed(text)) => assert_eq!(text, expected),
            other => panic!("{other:?} for {}", String::from_utf8_lossy(reply)),
        }
    }
}

#[tokio::test]
async fn a_run_reply_of_another_shape_is_an_error() {
    let command = Command::EnableFullscreen { con_id: 9 };
    let (_request, outcome) = run_against(command, br#"{"success":true}"#).await;
    let error = outcome.expect_err("an object, not an array");
    assert!(matches!(error, Error::Json(_)), "{error}");
}
