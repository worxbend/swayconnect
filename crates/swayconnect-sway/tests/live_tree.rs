//! A `GET_TREE` reply of the owner's sway 1.12 desktop (window titles, marks and the monitor
//! serial removed) parses with every field the layout export reads (`docs/DESIGN.md` 11.4).

use swayconnect_sway::{Node, Rect};

const LIVE_TREE: &str = include_str!("fixtures/live_tree.json");

fn tree() -> Node {
    serde_json::from_str(LIVE_TREE).expect("the live tree parses")
}

fn views<'a>(node: &'a Node, found: &mut Vec<&'a Node>) {
    if node.pid.is_some() {
        found.push(node);
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        views(child, found);
    }
}

#[test]
fn outputs_carry_power_scale_transform_and_model() {
    let tree = tree();
    let output = tree
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("DP-1"))
        .expect("output DP-1");
    assert_eq!(output.node_type, "output");
    assert_eq!(
        (output.active, output.power, output.scale),
        (Some(true), Some(true), Some(1.25))
    );
    assert_eq!(output.transform.as_deref(), Some("normal"));
    assert_eq!(output.make.as_deref(), Some("Dell Inc."));
    assert_eq!(output.model.as_deref(), Some("DELL P3223QE"));
    assert_eq!(output.current_workspace.as_deref(), Some("1"));
    assert_eq!(
        output.rect,
        Some(Rect {
            x: 0,
            y: 0,
            width: 3072,
            height: 1728
        })
    );
}

#[test]
fn workspaces_carry_rect_layout_and_focus() {
    let tree = tree();
    let output = &tree.nodes[1];
    let names: Vec<_> = output
        .nodes
        .iter()
        .map(|workspace| workspace.name.as_deref())
        .collect();
    assert_eq!(names, [Some("1"), Some("2"), Some("3"), Some("4")]);
    let first = &output.nodes[0];
    assert_eq!(
        first.rect,
        Some(Rect {
            x: 2,
            y: 28,
            width: 3068,
            height: 1698
        })
    );
    assert_eq!(first.layout.as_deref(), Some("splith"));
    assert_eq!(first.focus, [12, 7]);
    assert_eq!(first.nodes[0].layout.as_deref(), Some("splitv"));
    assert_eq!(output.nodes[1].floating_nodes.len(), 1);
}

#[test]
fn views_carry_scratchpad_state_and_idle_inhibit() {
    let tree = tree();
    let mut found = Vec::new();
    views(&tree, &mut found);
    assert_eq!(found.len(), 21);
    for view in &found {
        assert_eq!(view.name, None, "titles are removed from the fixture");
        assert_eq!(view.scratchpad_state.as_deref(), Some("none"));
        assert_eq!(view.inhibit_idle, Some(false));
        assert_eq!(view.layout.as_deref(), Some("none"));
        assert!(
            view.rect
                .is_some_and(|rect| rect.width > 0 && rect.height > 0)
        );
        assert_eq!((view.power, view.scale), (None, None));
    }
    let xwayland = found
        .iter()
        .filter(|view| view.window_properties.is_some())
        .count();
    assert_eq!(xwayland, 2);
}

#[test]
fn the_internal_scratchpad_output_comes_first() {
    let tree = tree();
    let internal = &tree.nodes[0];
    assert_eq!(internal.name.as_deref(), Some("__i3"));
    assert_eq!(internal.power, None);
    assert_eq!(internal.nodes[0].name.as_deref(), Some("__i3_scratch"));
}
