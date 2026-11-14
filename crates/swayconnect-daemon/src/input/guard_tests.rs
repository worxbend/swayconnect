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

/// Real sway 1.12 while fuzzel holds the keyboard as a layer surface: no node is focused, and
/// each node's `focus` list still leads to the window that gets the keys back.
#[test]
fn under_a_layer_surface_the_launcher_window_is_the_focus_chain_leaf() {
    let window = |id| node(id, "con", false, Vec::new());
    let workspace = |id, focused, children: Vec<Node>| {
        let focus = children.iter().rev().map(|child| child.id).collect();
        Node {
            focus,
            ..node(id, "workspace", focused, children)
        }
    };
    let root = |workspaces: Vec<Node>| Node {
        focus: workspaces.iter().map(|workspace| workspace.id).collect(),
        ..node(1, "root", false, workspaces)
    };
    let layer = root(vec![
        workspace(4, false, vec![window(5), window(7)]),
        workspace(9, false, vec![window(10)]),
    ]);
    assert_eq!(focused_window(&layer), None, "no window has the keyboard");
    assert_eq!(launcher_focus(&layer), LauncherFocus::Held(Some(7)));

    let empty = root(vec![workspace(4, false, Vec::new())]);
    assert_eq!(
        launcher_focus(&empty),
        LauncherFocus::Held(None),
        "the chain ends at a workspace"
    );
}

/// A focused node means that no layer surface holds the keyboard: the keys go to that node,
/// also when it is the window recorded at `launcher.open` (fuzzel has not mapped yet).
#[test]
fn a_focused_node_means_the_launcher_does_not_hold_the_keyboard() {
    let window = |id, focused| node(id, "con", focused, Vec::new());
    let tree = |workspace_focused, children| {
        let workspace = Node {
            focus: vec![5],
            ..node(4, "workspace", workspace_focused, children)
        };
        Node {
            focus: vec![4],
            ..node(1, "root", false, vec![workspace])
        }
    };

    let recorded_window_focused = tree(false, vec![window(5, true), window(7, false)]);
    assert_eq!(
        launcher_focus(&recorded_window_focused),
        LauncherFocus::NotHeld(Some(5))
    );
    let workspace_focused = tree(true, vec![window(5, false)]);
    assert_eq!(
        launcher_focus(&workspace_focused),
        LauncherFocus::NotHeld(None),
        "a focused workspace is not a held keyboard either"
    );
    let split = node(9, "con", true, vec![window(6, false), window(7, false)]);
    assert_eq!(
        launcher_focus(&tree(false, vec![split])),
        LauncherFocus::NotHeld(None),
        "a focused split container is not a window"
    );
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
