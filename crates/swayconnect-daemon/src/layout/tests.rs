use swayconnect_testkit::fixtures;

use super::*;

const WORKSPACE_TYPE: &str = "workspace";

fn parse_tree(text: &str) -> Node {
    serde_json::from_str(text).expect("tree fixture")
}

fn live_tree() -> Node {
    parse_tree(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/layout/live_tree.json"
    )))
}

fn find(node: &Node, id: u64) -> Option<&Node> {
    if node.id == id {
        return Some(node);
    }
    let mut children = node.nodes.iter().chain(&node.floating_nodes);
    children.find_map(|child| find(child, id))
}

fn layout_of(tree: &Node, workspace_id: u64) -> Layout {
    let workspace = find(tree, workspace_id).expect("workspace in tree");
    workspace_layout(workspace).expect("workspace has a layout")
}

/// `12` for a window; `h[..]`, `v[..]`, `t1[..]`, `s0[..]` for containers, with `a` after
/// the kind of a tabbed or stacked one.
fn shape(tile: &Tile) -> String {
    match tile {
        Tile::Window { id, .. } => id.to_string(),
        Tile::Container(container) => {
            let children: Vec<String> = container.children.iter().map(shape).collect();
            let active = container.active.map(|a| a.to_string()).unwrap_or_default();
            format!(
                "{}{active}[{}]",
                container.kind.as_str(),
                children.join(",")
            )
        }
    }
}

fn tiled_shape(layout: &Layout) -> String {
    layout.tiled.as_ref().map(shape).unwrap_or_default()
}

fn rect(x: i64, y: i64, width: i64, height: i64) -> Option<SwayRect> {
    Some(SwayRect {
        x,
        y,
        width,
        height,
    })
}

fn window(id: u64) -> Node {
    Node {
        id,
        node_type: "con".to_owned(),
        app_id: Some("app".to_owned()),
        rect: rect(0, 0, 100, 100),
        ..Node::default()
    }
}

fn split(id: u64, layout: &str, nodes: Vec<Node>) -> Node {
    Node {
        id,
        node_type: "con".to_owned(),
        layout: Some(layout.to_owned()),
        rect: rect(0, 0, 100, 100),
        nodes,
        ..Node::default()
    }
}

fn workspace(nodes: Vec<Node>) -> Node {
    Node {
        id: 1,
        node_type: WORKSPACE_TYPE.to_owned(),
        layout: Some("splith".to_owned()),
        rect: rect(0, 0, 100, 100),
        nodes,
        ..Node::default()
    }
}

fn depth(tile: &Tile) -> usize {
    match tile {
        Tile::Window { .. } => 0,
        Tile::Container(container) => 1 + container.children.iter().map(depth).max().unwrap_or(0),
    }
}

fn window_ids(tile: &Tile, ids: &mut Vec<u64>) {
    match tile {
        Tile::Window { id, .. } => ids.push(*id),
        Tile::Container(container) => {
            for child in &container.children {
                window_ids(child, ids);
            }
        }
    }
}

#[test]
fn the_live_tree_loses_its_one_child_wrappers() {
    let tree = live_tree();

    assert_eq!(
        tiled_shape(&layout_of(&tree, 4)),
        "h[v[5,h[13,15]],v[h[6,33],h[v[8,49],v[10,47]]]]"
    );
    assert_eq!(tiled_shape(&layout_of(&tree, 20)), "h[24,v[29,53]]");
    assert_eq!(
        tiled_shape(&layout_of(&tree, 36)),
        "h[37,v[38,h[40,v[42,44]]]]"
    );
    assert_eq!(tiled_shape(&layout_of(&tree, 56)), "h[57,v[59,62]]");
}

#[test]
fn the_live_tree_gives_rects_relative_to_the_workspace() {
    let layout = layout_of(&live_tree(), 20);

    assert_eq!(layout.size, [3068, 1698]);
    let Some(Tile::Container(root)) = &layout.tiled else {
        panic!("root is a container");
    };
    assert_eq!(root.r, [0, 0, 10000, 10000]);
    assert_eq!(
        root.children[0],
        Tile::Window {
            id: 24,
            r: [0, 0, 4997, 10000]
        }
    );
    let floating_ids: Vec<String> = layout.floating.iter().map(shape).collect();
    assert_eq!(floating_ids, ["23"]);
    assert_eq!(
        layout.fullscreen, None,
        "workspace fullscreen_mode 1 is ignored"
    );
}

#[test]
fn the_fixture_tree_gives_tabs_stacks_floating_and_fullscreen() {
    let tree = parse_tree(&fixtures::layout_tree());

    let first = layout_of(&tree, 4);
    assert_eq!(
        tiled_shape(&first),
        "h[6,v[8,t1[9,12]]]",
        "tab 12 is focused"
    );
    assert_eq!(first.floating.iter().map(shape).collect::<Vec<_>>(), ["13"]);
    assert_eq!(
        first.fullscreen, None,
        "workspace fullscreen_mode 1 is ignored"
    );
    assert_eq!(first.size, [2560, 1410]);

    let web = layout_of(&tree, 5);
    assert_eq!(tiled_shape(&web), "h[7]", "the root stays a container");
    assert_eq!(web.fullscreen, Some(7));

    let side = layout_of(&tree, 14);
    assert_eq!(
        tiled_shape(&side),
        "s0[15,18]",
        "the only container takes the root"
    );
}

#[test]
fn a_window_outside_the_workspace_is_clamped() {
    let tree = parse_tree(&fixtures::layout_tree());

    let web = layout_of(&tree, 5);

    // Window 7 covers the whole output, bar included: y starts 30 px above the workspace.
    let Some(Tile::Container(root)) = &web.tiled else {
        panic!("root is a container");
    };
    assert_eq!(
        root.children[0],
        Tile::Window {
            id: 7,
            r: [0, 0, 10000, 10000]
        }
    );
}

#[test]
fn a_workspace_without_a_window_has_no_tiled_root() {
    let mut empty_split = split(2, "splitv", vec![]);
    empty_split.app_id = None;

    let layout = workspace_layout(&workspace(vec![empty_split])).expect("layout");

    assert_eq!(layout.tiled, None);
    assert!(layout.floating.is_empty());
}

#[test]
fn a_workspace_without_a_usable_rect_has_no_layout() {
    let mut no_rect = workspace(vec![window(2)]);
    no_rect.rect = None;
    let mut zero_width = workspace(vec![window(2)]);
    zero_width.rect = rect(0, 0, 0, 100);

    assert_eq!(workspace_layout(&no_rect), None);
    assert_eq!(workspace_layout(&zero_width), None);
    assert_eq!(protocol_rect(&no_rect), None);
}

#[test]
fn the_active_tab_skips_a_focused_child_without_windows() {
    let empty = split(5, "splith", vec![]);
    let tabs = split(4, "tabbed", vec![window(2), window(3), empty]);
    let tabs = Node {
        focus: vec![5, 3, 2],
        ..tabs
    };

    let layout = workspace_layout(&workspace(vec![window(9), tabs])).expect("layout");

    assert_eq!(tiled_shape(&layout), "h[9,t1[2,3]]");
}

#[test]
fn a_tab_whose_focus_list_names_no_child_has_no_active_index() {
    let tabs = split(4, "stacked", vec![window(2), window(3)]);

    let layout = workspace_layout(&workspace(vec![window(9), tabs])).expect("layout");

    assert_eq!(tiled_shape(&layout), "h[9,s[2,3]]");
}

#[test]
fn a_tabbed_container_with_one_child_stays() {
    let tabs = split(4, "tabbed", vec![window(2)]);
    let tabs = Node {
        focus: vec![2],
        ..tabs
    };

    let layout = workspace_layout(&workspace(vec![window(9), tabs])).expect("layout");

    assert_eq!(tiled_shape(&layout), "h[9,t0[2]]");
}

#[test]
fn floating_tiles_keep_sway_order_and_a_floating_split_stays_a_container() {
    let mut top = window(7);
    top.node_type = "floating_con".to_owned();
    let mut group = split(8, "splitv", vec![window(5), window(6)]);
    group.node_type = "floating_con".to_owned();
    let mut ws = workspace(vec![window(2)]);
    ws.floating_nodes = vec![group, top];

    let layout = workspace_layout(&ws).expect("layout");

    let floating: Vec<String> = layout.floating.iter().map(shape).collect();
    assert_eq!(floating, ["v[5,6]", "7"]);
}

#[test]
fn only_a_window_counts_as_fullscreen() {
    let mut container = split(4, "splitv", vec![window(2), window(3)]);
    container.fullscreen_mode = Some(1);
    let mut floating = window(6);
    floating.node_type = "floating_con".to_owned();
    floating.fullscreen_mode = Some(2);
    let mut ws = workspace(vec![container]);
    ws.fullscreen_mode = Some(1);

    assert_eq!(workspace_layout(&ws).expect("layout").fullscreen, None);

    ws.floating_nodes = vec![floating];
    assert_eq!(workspace_layout(&ws).expect("layout").fullscreen, Some(6));
}

/// A chain of 20 splits, each with one window and the next split: 21 windows.
fn deep_workspace() -> Node {
    let mut inner = split(100, "splith", vec![window(200), window(201)]);
    for level in 1..20 {
        let kind = if level % 2 == 0 { "splith" } else { "splitv" };
        let mut tabs = split(100 + level, kind, vec![window(200 + 2 * level), inner]);
        if level == 5 {
            tabs.layout = Some("tabbed".to_owned());
            tabs.focus = vec![100 + level - 1];
        }
        inner = tabs;
    }
    workspace(vec![window(1000), inner])
}

#[test]
fn depth_is_capped_at_16_container_levels_and_no_window_is_lost() {
    let layout = workspace_layout(&deep_workspace()).expect("layout");
    let root = layout.tiled.as_ref().expect("tiled root");

    assert_eq!(depth(root), MAX_LAYOUT_DEPTH);
    let mut ids = Vec::new();
    window_ids(root, &mut ids);
    let mut expected: Vec<u64> = vec![1000];
    expected.extend((1..20).rev().map(|level| 200 + 2 * level));
    expected.extend([200, 201]);
    assert_eq!(ids, expected, "every window once, in sway order");
}

#[test]
fn a_flattened_tab_points_at_the_first_window_of_its_shown_child() {
    // Split 105 is tabbed and shows split 104. The root is level 1 and split 119 level 2, so
    // split 105 is level 16: the deepest one, which takes every window below it.
    let layout = workspace_layout(&deep_workspace()).expect("layout");
    let mut container = match &layout.tiled {
        Some(Tile::Container(root)) => root,
        _ => panic!("root is a container"),
    };
    while let Some(Tile::Container(inner)) = container.children.last() {
        container = inner;
    }

    assert_eq!(container.kind, ContainerKind::Tabbed);
    let ids: Vec<String> = container.children.iter().map(shape).collect();
    assert_eq!(ids, ["210", "208", "206", "204", "202", "200", "201"]);
    assert_eq!(container.active, Some(1));
}

#[test]
fn basis_points_round_half_up_and_clamp() {
    assert_eq!(basis_points(1, 3), 3333);
    assert_eq!(basis_points(1, 20000), 1, "0.5 rounds up");
    assert_eq!(basis_points(-5, 100), 0);
    assert_eq!(basis_points(500, 100), 10000);
    assert_eq!(basis_points(i128::from(i64::MAX), u64::MAX), 5000);
}

#[test]
fn a_node_without_a_rect_gets_zeros() {
    let area = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };

    assert_eq!(basis_rect(None, area), [0; 4]);
}

#[test]
fn a_rect_corner_outside_sint_is_unknown() {
    let mut far = workspace(vec![]);
    far.rect = rect(i64::MAX, 0, 10, 10);

    assert_eq!(protocol_rect(&far), None);
}
