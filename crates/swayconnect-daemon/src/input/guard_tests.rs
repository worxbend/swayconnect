use std::path::PathBuf;

use swayconnect_testkit::{FakeSway, TestDirs};

use super::*;

fn node(id: u64, node_type: &str, focused: bool, children: Vec<Node>) -> Node {
    Node {
        id,
        node_type: node_type.to_owned(),
        focused,
        app_id: (node_type == "con" && children.is_empty()).then(|| "app".to_owned()),
        nodes: children,
        ..Node::default()
    }
}

#[test]
fn only_a_focused_window_counts() {
    let window = |id, focused| node(id, "con", focused, Vec::new());
    let tree = |workspace_focused, children| {
        node(
            1,
            "root",
            false,
            vec![node(3, "workspace", workspace_focused, children)],
        )
    };
    assert_eq!(
        focused_window(&tree(false, vec![window(6, false), window(7, true)])),
        Some(7)
    );
    assert_eq!(focused_window(&tree(true, vec![window(6, false)])), None);
    let split = node(9, "con", true, vec![window(6, false), window(7, false)]);
    assert_eq!(
        focused_window(&tree(false, vec![split])),
        None,
        "a focused split container is not a window"
    );

    let mut floating = tree(false, vec![window(6, false)]);
    floating.nodes[0].floating_nodes = vec![node(8, "floating_con", true, Vec::new())];
    floating.nodes[0].floating_nodes[0].app_id = Some("app".to_owned());
    assert_eq!(focused_window(&floating), Some(8));
}

#[tokio::test]
async fn every_read_asks_sway_again() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let focus = SwayFocus::new(SwayTarget::Explicit(sway.socket_path().to_owned()));
    assert_eq!(focus.focused_window().await.expect("read"), Some(6));
    sway.focus_without_event(7);
    assert_eq!(focus.focused_window().await.expect("read"), Some(7));
    sway.focus_without_event(4);
    assert_eq!(focus.focused_window().await.expect("read"), None);
}

#[tokio::test]
async fn a_lost_connection_is_replaced_once() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let focus = SwayFocus::new(SwayTarget::Explicit(sway.socket_path().to_owned()));
    focus.focused_window().await.expect("first read");
    sway.close_connections();
    sway.focus_without_event(7);
    assert_eq!(focus.focused_window().await.expect("reconnected"), Some(7));
}

#[tokio::test]
async fn no_sway_is_an_error() {
    let focus = SwayFocus::new(SwayTarget::Explicit(PathBuf::from(
        "/nonexistent/sway-ipc.sock",
    )));
    assert!(focus.focused_window().await.is_err());
}
