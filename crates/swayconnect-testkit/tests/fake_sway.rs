//! The fake sway seen from a raw Unix socket, and the path guard of `FakeSway::start`.

use std::time::Duration;

use swayconnect_testkit::{Event, FakeSway, Request, TestDirs, fixtures};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const GET_OUTPUTS: u32 = 3;
const WINDOW_EVENT: u32 = 0x8000_0003;
const BINDING_EVENT: u32 = 0x8000_0005;

async fn connect(sway: &FakeSway) -> UnixStream {
    UnixStream::connect(sway.socket_path())
        .await
        .expect("connect to fake sway")
}

async fn send(stream: &mut UnixStream, message_type: u32, payload: &str) {
    let mut frame = b"i3-ipc".to_vec();
    frame.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    frame.extend_from_slice(&message_type.to_ne_bytes());
    frame.extend_from_slice(payload.as_bytes());
    stream.write_all(&frame).await.expect("write frame");
}

async fn receive(stream: &mut UnixStream) -> (u32, String) {
    let mut header = [0u8; 14];
    stream.read_exact(&mut header).await.expect("read header");
    assert_eq!(&header[..6], b"i3-ipc");
    let length = u32::from_ne_bytes(header[6..10].try_into().expect("4 bytes"));
    let message_type = u32::from_ne_bytes(header[10..14].try_into().expect("4 bytes"));
    let mut payload = vec![0u8; length as usize];
    stream.read_exact(&mut payload).await.expect("read payload");
    (
        message_type,
        String::from_utf8(payload).expect("UTF-8 payload"),
    )
}

#[tokio::test]
async fn get_version_reply_has_the_i3_ipc_header_in_native_byte_order() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;
    let expected_payload = fixtures::version();

    send(&mut stream, 7, "").await;
    let mut header = [0u8; 14];
    stream.read_exact(&mut header).await.expect("read header");
    let mut payload = vec![0u8; expected_payload.len()];
    stream.read_exact(&mut payload).await.expect("read payload");

    assert_eq!(header[..6], [0x69, 0x33, 0x2d, 0x69, 0x70, 0x63]);
    assert_eq!(header[6..10], (expected_payload.len() as u32).to_ne_bytes());
    assert_eq!(header[10..14], 7u32.to_ne_bytes());
    assert_eq!(payload, expected_payload.as_bytes());
}

#[tokio::test]
async fn an_unknown_message_type_gets_no_reply() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    send(&mut stream, GET_OUTPUTS, "").await;
    send(&mut stream, 100, "").await;
    send(&mut stream, 7, "").await;

    // Requests are answered in order, so a reply to type 3 or 100 would come first.
    assert_eq!(receive(&mut stream).await, (7, fixtures::version()));
}

#[tokio::test]
async fn default_replies_are_the_fixtures() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    send(&mut stream, 1, "").await;
    send(&mut stream, 4, "").await;

    assert_eq!(receive(&mut stream).await, (1, fixtures::workspaces()));
    assert_eq!(receive(&mut stream).await, (4, fixtures::tree()));
}

#[tokio::test]
async fn run_command_payloads_are_recorded_and_the_reply_can_be_scripted() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    send(&mut stream, 0, "workspace number 2").await;
    assert_eq!(
        receive(&mut stream).await,
        (0, r#"[{"success":true}]"#.to_owned())
    );

    let refusal = r#"[{"success":false,"error":"No matching node."}]"#;
    sway.set_reply(Request::RunCommand, refusal);
    send(&mut stream, 0, "[con_id=99] focus").await;
    assert_eq!(receive(&mut stream).await, (0, refusal.to_owned()));

    assert_eq!(sway.commands(), ["workspace number 2", "[con_id=99] focus"]);
}

#[tokio::test]
async fn an_event_reaches_only_a_connection_subscribed_to_it() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut subscriber = connect(&sway).await;
    let mut bystander = connect(&sway).await;

    send(&mut subscriber, 2, r#"["window"]"#).await;
    assert_eq!(
        receive(&mut subscriber).await,
        (2, r#"{"success":true}"#.to_owned())
    );
    send(&mut bystander, 7, "").await;
    receive(&mut bystander).await;

    sway.send_event(Event::Output, r#"{"change":"unspecified"}"#);
    sway.send_event(Event::Window, r#"{"change":"focus"}"#);

    assert_eq!(
        receive(&mut subscriber).await,
        (WINDOW_EVENT, r#"{"change":"focus"}"#.to_owned())
    );
    send(&mut bystander, 7, "").await;
    assert_eq!(receive(&mut bystander).await, (7, fixtures::version()));
}

#[tokio::test]
async fn a_binding_event_reaches_a_binding_subscriber_with_type_5() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    send(&mut stream, 2, r#"["window","binding"]"#).await;
    receive(&mut stream).await;
    sway.send_event(Event::Binding, fixtures::binding_event());

    assert_eq!(
        receive(&mut stream).await,
        (BINDING_EVENT, fixtures::binding_event())
    );
}

#[tokio::test]
async fn a_swapped_tree_reply_is_served_from_the_next_request_on() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    sway.set_reply(Request::GetTree, fixtures::layout_tree());
    let mut stream = connect(&sway).await;

    send(&mut stream, 4, "").await;
    assert_eq!(receive(&mut stream).await, (4, fixtures::layout_tree()));
    sway.set_reply(Request::GetTree, fixtures::layout_tree_resized());
    send(&mut stream, 4, "").await;

    assert_eq!(
        receive(&mut stream).await,
        (4, fixtures::layout_tree_resized())
    );
}

#[tokio::test]
async fn a_malformed_subscribe_is_refused_and_subscribes_to_nothing() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    send(&mut stream, 2, "window").await;
    assert_eq!(
        receive(&mut stream).await,
        (2, r#"{"success":false}"#.to_owned())
    );

    sway.send_event(Event::Window, "{}");
    send(&mut stream, 7, "").await;
    assert_eq!(receive(&mut stream).await.0, 7);
}

#[tokio::test]
async fn a_stopped_fake_records_commands_but_does_not_answer() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    sway.stop_answering();
    send(&mut stream, 1, "").await;
    send(&mut stream, 0, "kill").await;
    // The record of `kill` is the only sign that both requests were read while stopped.
    let both_read = async {
        while sway.commands().is_empty() {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), both_read)
        .await
        .expect("fake sway read the requests");
    sway.resume_answering();
    send(&mut stream, 7, "").await;

    assert_eq!(receive(&mut stream).await, (7, fixtures::version()));
    assert_eq!(sway.commands(), ["kill"]);
}

#[tokio::test]
async fn close_connections_hangs_up_and_keeps_listening() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;
    send(&mut stream, 7, "").await;
    receive(&mut stream).await;

    sway.close_connections();

    let mut rest = Vec::new();
    assert_eq!(stream.read_to_end(&mut rest).await.expect("read to end"), 0);
    let mut reconnected = connect(&sway).await;
    send(&mut reconnected, 7, "").await;
    assert_eq!(receive(&mut reconnected).await.0, 7);
}

#[tokio::test]
async fn bad_magic_closes_the_connection() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    stream
        .write_all(b"i3-ipx\0\0\0\0\x07\0\0\0")
        .await
        .expect("write");

    let mut rest = Vec::new();
    assert_eq!(stream.read_to_end(&mut rest).await.expect("read to end"), 0);
}

#[tokio::test]
async fn dropping_the_fake_removes_the_socket_so_it_can_start_again() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let socket_path = sway.socket_path().to_owned();
    assert_eq!(
        socket_path,
        dirs.sway_socket().canonicalize().expect("canonical")
    );

    drop(sway);

    assert!(!socket_path.exists());
    let restarted = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&restarted).await;
    send(&mut stream, 7, "").await;
    assert_eq!(receive(&mut stream).await.0, 7);
}

#[tokio::test]
#[should_panic(expected = "is outside")]
async fn start_panics_when_the_socket_path_is_outside_the_directory() {
    let dirs = TestDirs::new();

    FakeSway::start_at(&dirs.root().join("sway-ipc.sock"), &dirs.runtime_dir(), &[]);
}

#[tokio::test]
#[should_panic(expected = "is outside")]
async fn start_panics_when_a_symlink_leads_out_of_the_directory() {
    let dirs = TestDirs::new();
    let link = dirs.runtime_dir().join("link");
    std::os::unix::fs::symlink(dirs.data_home(), &link).expect("symlink");

    FakeSway::start_at(&link.join("sway-ipc.sock"), &dirs.runtime_dir(), &[]);
}

#[tokio::test]
#[should_panic(expected = "is forbidden")]
async fn start_panics_when_the_socket_path_is_forbidden() {
    let dirs = TestDirs::new();

    FakeSway::start(&dirs.runtime_dir(), &[dirs.sway_socket()]);
}

#[tokio::test]
#[should_panic(expected = "is forbidden")]
async fn start_panics_when_the_forbidden_path_is_an_existing_socket() {
    let dirs = TestDirs::new();
    let _real_sway = std::os::unix::net::UnixListener::bind(dirs.sway_socket()).expect("bind");

    FakeSway::start(&dirs.runtime_dir(), &[dirs.sway_socket()]);
}

#[tokio::test]
#[should_panic(expected = "already exists")]
async fn start_panics_instead_of_replacing_an_existing_file() {
    let dirs = TestDirs::new();
    std::fs::write(dirs.sway_socket(), "").expect("write");

    FakeSway::start(&dirs.runtime_dir(), &[]);
}

#[test]
fn the_guard_panics_before_anything_is_bound() {
    let dirs = TestDirs::new();
    let outside = dirs.root().join("sway-ipc.sock");
    let runtime_dir = dirs.runtime_dir();

    // No runtime here: a panic from the guard, not from `tokio::spawn`, proves the order.
    let panic = std::panic::catch_unwind(|| FakeSway::start_at(&outside, &runtime_dir, &[]))
        .expect_err("start must panic");

    let message = panic.downcast_ref::<String>().expect("panic message");
    assert!(message.contains("is outside"), "{message}");
    assert!(!outside.exists());
}

/// One command per `docs/DESIGN.md` 11.4 variant added in M2a.
const M2A_COMMANDS: [&str; 9] = [
    "[con_id=6] move container to workspace number 2",
    "[con_id=6] floating enable",
    "[con_id=6] floating disable",
    "[con_id=6] layout tabbed",
    "[con_id=6] move scratchpad",
    "[con_id=6] scratchpad show",
    "[con_id=6] kill",
    "output * power on",
    "workspace --no-auto-back-and-forth number 2",
];

#[tokio::test]
async fn every_m2a_command_is_recorded_and_answered_with_success() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;

    for command in M2A_COMMANDS {
        send(&mut stream, 0, command).await;
        assert_eq!(
            receive(&mut stream).await,
            (0, r#"[{"success":true}]"#.to_owned()),
            "{command}"
        );
    }

    assert_eq!(sway.commands(), M2A_COMMANDS);
}

#[tokio::test]
async fn a_reply_scripted_for_one_command_leaves_the_others_alone() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let refusal = r#"[{"success":false,"error":"No matching node."}]"#;
    sway.set_command_reply("[con_id=6] kill", refusal);
    let mut stream = connect(&sway).await;

    send(&mut stream, 0, "[con_id=6] kill").await;
    assert_eq!(receive(&mut stream).await, (0, refusal.to_owned()));
    send(&mut stream, 0, "[con_id=7] kill").await;
    assert_eq!(
        receive(&mut stream).await,
        (0, r#"[{"success":true}]"#.to_owned())
    );
    send(&mut stream, 0, "[con_id=6] kill").await;
    assert_eq!(receive(&mut stream).await, (0, refusal.to_owned()));

    assert_eq!(sway.commands().len(), 3);
}

fn focused_ids(tree: &serde_json::Value, found: &mut Vec<u64>) {
    if tree["focused"] == true {
        found.push(tree["id"].as_u64().expect("id"));
    }
    for key in ["nodes", "floating_nodes"] {
        for child in tree[key].as_array().into_iter().flatten() {
            focused_ids(child, found);
        }
    }
}

async fn focused_in_tree(stream: &mut UnixStream) -> Vec<u64> {
    send(stream, 4, "").await;
    let (message_type, payload) = receive(stream).await;
    assert_eq!(message_type, 4);
    let mut found = Vec::new();
    focused_ids(
        &serde_json::from_str(&payload).expect("tree JSON"),
        &mut found,
    );
    found
}

#[tokio::test]
async fn focus_moves_in_the_tree_without_an_event() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut stream = connect(&sway).await;
    send(&mut stream, 2, r#"["window","workspace"]"#).await;
    receive(&mut stream).await;
    assert_eq!(focused_in_tree(&mut stream).await, [6]);

    sway.focus_without_event(7);

    // The subscribed connection gets only replies: no event frame comes before them.
    assert_eq!(focused_in_tree(&mut stream).await, [7]);
    send(&mut stream, 7, "").await;
    assert_eq!(receive(&mut stream).await, (7, fixtures::version()));
}

#[tokio::test]
async fn focus_moves_in_a_scripted_tree_too() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    sway.set_reply(Request::GetTree, fixtures::layout_tree());
    let mut stream = connect(&sway).await;
    let mut before = Vec::new();
    focused_ids(
        &serde_json::from_str(&fixtures::layout_tree()).expect("tree JSON"),
        &mut before,
    );
    let other = layout_window_ids()
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("an unfocused window");

    sway.focus_without_event(other);

    assert_eq!(focused_in_tree(&mut stream).await, [other]);
}

fn layout_window_ids() -> Vec<u64> {
    fn collect(node: &serde_json::Value, ids: &mut Vec<u64>) {
        let children: Vec<&serde_json::Value> = ["nodes", "floating_nodes"]
            .iter()
            .flat_map(|key| node[*key].as_array().into_iter().flatten())
            .collect();
        if node["type"] == "con" && children.is_empty() {
            ids.push(node["id"].as_u64().expect("id"));
        }
        for child in children {
            collect(child, ids);
        }
    }
    let mut ids = Vec::new();
    collect(
        &serde_json::from_str(&fixtures::layout_tree()).expect("tree JSON"),
        &mut ids,
    );
    ids
}

#[tokio::test]
#[should_panic(expected = "no node 99")]
async fn focus_on_a_missing_node_panics() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);

    sway.focus_without_event(99);
}
