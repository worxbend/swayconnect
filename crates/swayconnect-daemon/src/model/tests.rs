use swayconnect_protocol::JsonObject;

use super::*;

const FOUR_BYTE_SCALAR: &str = "\u{1F600}";

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $name
        ))
    };
}

fn version() -> Version {
    Version {
        human_readable: "1.12".to_owned(),
    }
}

fn desktop_from_fixtures(tree: &str, workspaces: &str) -> Desktop {
    let tree: Node = serde_json::from_str(tree).expect("tree fixture");
    let workspaces: Vec<WorkspaceInfo> =
        serde_json::from_str(workspaces).expect("workspaces fixture");
    online_desktop(&tree, &workspaces, &version())
}

fn window_node(id: u64, title: &str) -> Node {
    Node {
        id,
        node_type: TILED_TYPE.to_owned(),
        name: Some(title.to_owned()),
        app_id: Some("app".to_owned()),
        ..Node::default()
    }
}

fn workspace_info(id: u64, name: &str) -> WorkspaceInfo {
    WorkspaceInfo {
        id,
        num: Some(1),
        name: Some(name.to_owned()),
        output: Some(name.to_owned()),
        ..WorkspaceInfo::default()
    }
}

/// One output whose workspaces have the ids of `listed`; the first holds `windows`.
fn tree_of(listed: &[WorkspaceInfo], windows: Vec<Node>) -> Node {
    let mut workspace_nodes: Vec<Node> = listed
        .iter()
        .map(|info| Node {
            id: info.id,
            node_type: WORKSPACE_TYPE.to_owned(),
            ..Node::default()
        })
        .collect();
    if let Some(first) = workspace_nodes.first_mut() {
        first.nodes = windows;
    }
    let output = Node {
        node_type: "output".to_owned(),
        nodes: workspace_nodes,
        ..Node::default()
    };
    Node {
        node_type: "root".to_owned(),
        nodes: vec![output],
        ..Node::default()
    }
}

fn desktop_with_windows(windows: Vec<Node>) -> Desktop {
    let listed = [workspace_info(1, "1")];
    online_desktop(&tree_of(&listed, windows), &listed, &version())
}

fn encoded_len(desktop: &Desktop) -> usize {
    serde_json::to_vec(&desktop.encode())
        .expect("encodable")
        .len()
}

#[test]
fn the_fixture_desktop_gives_the_expected_json() {
    let desktop = desktop_from_fixtures(
        fixture!("desktop_tree.json"),
        fixture!("desktop_workspaces.json"),
    );

    let expected: JsonObject =
        serde_json::from_str(fixture!("desktop_expected.json")).expect("expected fixture");
    assert_eq!(desktop.encode(), expected);
}

#[test]
fn the_fixture_desktop_covers_each_special_case() {
    let desktop = desktop_from_fixtures(
        fixture!("desktop_tree.json"),
        fixture!("desktop_workspaces.json"),
    );
    let window = |id: u64| {
        desktop
            .windows
            .iter()
            .find(|window| window.id == id)
            .expect("window is listed")
    };

    let ids: Vec<u64> = desktop.windows.iter().map(|window| window.id).collect();
    assert_eq!(ids, [13, 14, 15, 17], "scratchpad 30 and empty 18 skipped");
    assert_eq!(window(14).app_id.as_deref(), Some("Painter"));
    assert!(window(14).fullscreen);
    assert!(window(15).floating);
    assert!(!window(13).floating && !window(13).fullscreen);
    assert_eq!(desktop.workspaces[1].number, None);
    assert!(!desktop.truncated);
}

#[test]
fn the_windows_of_a_floating_split_container_float() {
    let floating_split = Node {
        id: 20,
        node_type: FLOATING_TYPE.to_owned(),
        nodes: vec![window_node(21, "left"), window_node(22, "right")],
        ..Node::default()
    };
    let listed = [workspace_info(1, "1")];
    let mut tree = tree_of(&listed, vec![window_node(10, "tiled")]);
    tree.nodes[0].nodes[0].floating_nodes = vec![floating_split];

    let desktop = online_desktop(&tree, &listed, &version());

    let windows = desktop.windows.iter();
    let floating: Vec<(u64, bool)> = windows.map(|window| (window.id, window.floating)).collect();
    assert_eq!(floating, [(10, false), (21, true), (22, true)]);
}

#[test]
fn hostile_characters_become_the_replacement_character() {
    let desktop = desktop_from_fixtures(
        fixture!("hostile_tree.json"),
        fixture!("hostile_workspaces.json"),
    );

    let workspace = &desktop.workspaces[0];
    assert_eq!(workspace.name, "1\u{FFFD}[2J\u{FFFD}\u{FFFD}evil");
    assert_eq!(workspace.output.as_deref(), Some("TEST\u{FFFD}-1"));
    let window = &desktop.windows[0];
    assert_eq!(
        window.title.as_deref(),
        Some("a\u{FFFD}[31mb\u{FFFD}c\u{FFFD}d")
    );
    assert_eq!(
        window.app_id.as_deref(),
        Some("app\u{FFFD}\u{FFFD}\u{FFFD}")
    );
    let xwayland = &desktop.windows[1];
    assert_eq!(xwayland.app_id.as_deref(), Some("\u{FFFD}Class\u{FFFD}"));
}

#[test]
fn a_hostile_character_is_replaced_before_the_length_cut() {
    let long_title = format!("{}\u{1b}tail", "a".repeat(MAX_TITLE_SCALARS - 1));
    let long_class = format!("{}\u{202e}tail", "b".repeat(MAX_LABEL_SCALARS - 1));
    let mut node = window_node(2, &long_title);
    node.app_id = Some(long_class);

    let desktop = desktop_with_windows(vec![node]);

    let title = desktop.windows[0].title.as_deref().expect("title");
    assert_eq!(title.chars().count(), MAX_TITLE_SCALARS);
    assert!(title.ends_with("a\u{FFFD}"));
    let app_id = desktop.windows[0].app_id.as_deref().expect("app_id");
    assert_eq!(app_id.chars().count(), MAX_LABEL_SCALARS);
    assert!(app_id.ends_with("b\u{FFFD}"));
}

#[test]
fn a_title_is_cut_to_200_scalars_and_a_name_to_128() {
    let listed = [workspace_info(1, &FOUR_BYTE_SCALAR.repeat(300))];
    let windows = vec![window_node(2, &FOUR_BYTE_SCALAR.repeat(300))];

    let desktop = online_desktop(&tree_of(&listed, windows), &listed, &version());

    let title = desktop.windows[0].title.as_deref().expect("title");
    assert_eq!(title, FOUR_BYTE_SCALAR.repeat(200));
    assert_eq!(desktop.workspaces[0].name, FOUR_BYTE_SCALAR.repeat(128));
    assert!(!desktop.truncated, "a cut string is not a cut list");
}

#[test]
fn the_window_list_is_cut_to_512_and_marked_truncated() {
    let nodes = |count: u64| (0..count).map(|id| window_node(100 + id, "t")).collect();

    let full = desktop_with_windows(nodes(512));
    let cut = desktop_with_windows(nodes(513));

    assert_eq!(full.windows.len(), 512);
    assert!(!full.truncated);
    assert_eq!(cut.windows, full.windows);
    assert!(cut.truncated);
}

#[test]
fn the_workspace_list_is_cut_to_256_and_marked_truncated() {
    let listed: Vec<WorkspaceInfo> = (0..257).map(|id| workspace_info(id, "w")).collect();

    let full = online_desktop(&tree_of(&listed[..256], vec![]), &listed[..256], &version());
    let cut = online_desktop(&tree_of(&listed, vec![]), &listed, &version());

    assert_eq!(full.workspaces.len(), 256);
    assert!(!full.truncated);
    assert_eq!(cut.workspaces, full.workspaces);
    assert!(cut.truncated);
}

#[test]
fn worst_case_titles_are_cut_to_the_size_limit_and_keep_a_prefix() {
    let worst_title = FOUR_BYTE_SCALAR.repeat(MAX_TITLE_SCALARS);
    let nodes: Vec<Node> = (0..512)
        .map(|id| window_node(100 + id, &worst_title))
        .collect();

    let desktop = desktop_with_windows(nodes);

    assert!(encoded_len(&desktop) <= MAX_DESKTOP_BYTES);
    assert!(desktop.truncated);
    let kept_ids: Vec<u64> = desktop.windows.iter().map(|window| window.id).collect();
    let prefix_ids: Vec<u64> = (100..).take(kept_ids.len()).collect();
    assert_eq!(kept_ids, prefix_ids);
    assert!(!kept_ids.is_empty() && kept_ids.len() < 512);
    assert_eq!(desktop.workspaces.len(), 1, "workspaces are cut last");

    let mut one_more = desktop.clone();
    one_more.windows.push(desktop.windows[0].clone());
    assert!(
        encoded_len(&one_more) > MAX_DESKTOP_BYTES,
        "the kept prefix is the longest one that fits"
    );
}

#[test]
fn workspaces_are_cut_from_the_end_when_no_window_is_left() {
    let worst_label = FOUR_BYTE_SCALAR.repeat(MAX_LABEL_SCALARS);
    let listed: Vec<WorkspaceInfo> = (0..256)
        .map(|id| workspace_info(id, &worst_label))
        .collect();
    let windows = vec![window_node(1000, "t")];

    let desktop = online_desktop(&tree_of(&listed, windows), &listed, &version());

    assert!(encoded_len(&desktop) <= MAX_DESKTOP_BYTES);
    assert!(desktop.truncated);
    assert!(desktop.windows.is_empty());
    let kept_ids: Vec<u64> = desktop.workspaces.iter().map(|entry| entry.id).collect();
    let prefix_ids: Vec<u64> = (0..).take(kept_ids.len()).collect();
    assert_eq!(kept_ids, prefix_ids);
    assert!(!kept_ids.is_empty() && kept_ids.len() < 256);
}

#[test]
fn a_workspace_number_outside_the_protocol_range_is_null() {
    assert_eq!(workspace_number(-1), None);
    assert_eq!(workspace_number(0), Some(0));
    assert_eq!(workspace_number(2_147_483_647), Some(2_147_483_647));
    assert_eq!(workspace_number(2_147_483_648), None);
}

#[test]
fn a_window_of_an_unlisted_workspace_is_left_out() {
    let in_tree = [workspace_info(1, "1")];
    let tree = tree_of(&in_tree, vec![window_node(2, "t")]);

    let desktop = online_desktop(&tree, &[], &version());

    assert!(desktop.windows.is_empty());
    assert!(!desktop.truncated);
}

#[test]
fn the_offline_desktop_is_empty() {
    let desktop = offline_desktop();

    assert!(!desktop.sway.online);
    assert_eq!(desktop.sway.version, None);
    assert!(desktop.workspaces.is_empty() && desktop.windows.is_empty());
    assert!(!desktop.truncated);
}

fn layout_fixture_desktop(icon_of: IconLookup<'_>) -> Desktop {
    let tree: Node = serde_json::from_str(fixture!("layout/example_tree.json")).expect("tree");
    let workspaces: Vec<WorkspaceInfo> =
        serde_json::from_str(fixture!("layout/example_workspaces.json")).expect("workspaces");
    online_desktop_with_icons(&tree, &workspaces, &version(), icon_of)
}

#[test]
fn the_live_example_fixture_gives_the_protocol_example() {
    let chrome = IconHash::parse("q83vEjRWeJCrze8SNFZ4kA").expect("hash");
    let icon_of = |node: &Node| (app_id_of(node) == Some("google-chrome")).then_some(chrome);

    let desktop = layout_fixture_desktop(&icon_of);

    let expected: JsonObject =
        serde_json::from_str(fixture!("layout/example_expected.json")).expect("expected");
    assert_eq!(desktop.encode(), expected);
}

#[test]
fn the_whole_live_tree_keeps_every_window_in_one_layout() {
    let tree: Node = serde_json::from_str(fixture!("layout/live_tree.json")).expect("tree");
    let workspaces: Vec<WorkspaceInfo> =
        serde_json::from_str(fixture!("layout/live_workspaces.json")).expect("workspaces");

    let desktop = online_desktop(&tree, &workspaces, &version());

    let mut in_layouts = Vec::new();
    for workspace in &desktop.workspaces {
        let layout = workspace.layout.clone().flatten().expect("layout");
        let tiles = layout.tiled.iter().chain(&layout.floating);
        for tile in tiles {
            tile_window_ids(tile, &mut in_layouts);
        }
    }
    let mut listed: Vec<u64> = desktop.windows.iter().map(|window| window.id).collect();
    in_layouts.sort_unstable();
    listed.sort_unstable();
    assert_eq!(in_layouts, listed);
    assert_eq!(listed.len(), 21);
    assert_eq!(desktop.layouts_cut, Some(false));
    assert!(encoded_len(&desktop) < 16_384, "the live desktop is small");
}

fn tile_window_ids(tile: &swayconnect_protocol::Tile, ids: &mut Vec<u64>) {
    match tile {
        swayconnect_protocol::Tile::Window { id, .. } => ids.push(*id),
        swayconnect_protocol::Tile::Container(container) => {
            for child in &container.children {
                tile_window_ids(child, ids);
            }
        }
    }
}

fn sway_rect(x: i64, y: i64, width: i64, height: i64) -> Option<swayconnect_sway::Rect> {
    Some(swayconnect_sway::Rect {
        x,
        y,
        width,
        height,
    })
}

/// One 3840 x 2160 workspace: a splith root of `count / 8` splitv columns of 8 windows.
fn synthetic_desktop(count: u64, title: &str, hidden: u64) -> Desktop {
    let columns = count.div_ceil(8);
    let column_width = 3840 / i64::try_from(columns.max(1)).expect("small");
    let column = |index: u64| {
        let x = i64::try_from(index).expect("small") * column_width;
        let windows = (index * 8..(index * 8 + 8).min(count))
            .map(|id| {
                let row = i64::try_from(id % 8).expect("small");
                Node {
                    rect: sway_rect(x, row * 270, column_width, 270),
                    ..window_node(10_000 + id, title)
                }
            })
            .collect();
        Node {
            id: 100_000 + index,
            node_type: TILED_TYPE.to_owned(),
            layout: Some("splitv".to_owned()),
            rect: sway_rect(x, 0, column_width, 2160),
            nodes: windows,
            ..Node::default()
        }
    };
    let listed = [workspace_info(1, "1")];
    let mut tree = tree_of(&listed, (0..columns).map(column).collect());
    let workspace = &mut tree.nodes[0].nodes[0];
    workspace.rect = sway_rect(0, 0, 3840, 2160);
    workspace.layout = Some("splith".to_owned());
    let scratch = Node {
        node_type: WORKSPACE_TYPE.to_owned(),
        name: Some(SCRATCHPAD_WORKSPACE.to_owned()),
        floating_nodes: (0..hidden)
            .map(|id| window_node(50_000 + id, title))
            .collect(),
        ..Node::default()
    };
    tree.nodes.insert(
        0,
        Node {
            node_type: "output".to_owned(),
            name: Some(INTERNAL_OUTPUT.to_owned()),
            nodes: vec![scratch],
            ..Node::default()
        },
    );
    online_desktop(&tree, &listed, &version())
}

fn layout_window_count(desktop: &Desktop) -> usize {
    let mut ids = Vec::new();
    for workspace in &desktop.workspaces {
        if let Some(Some(layout)) = &workspace.layout {
            for tile in layout.tiled.iter().chain(&layout.floating) {
                tile_window_ids(tile, &mut ids);
            }
        }
    }
    ids.len()
}

#[test]
fn a_512_window_desktop_with_its_layout_stays_under_the_size_limit() {
    let desktop = synthetic_desktop(512, &"t".repeat(64), 0);

    assert!(encoded_len(&desktop) <= MAX_DESKTOP_BYTES);
    assert_eq!(desktop.windows.len(), 512);
    assert_eq!(layout_window_count(&desktop), 512);
    assert_eq!(desktop.layouts_cut, Some(false));
    assert!(!desktop.truncated);
}

#[test]
fn a_tile_of_a_window_cut_by_count_leaves_the_layout() {
    let desktop = synthetic_desktop(520, "t", 0);

    assert!(desktop.truncated);
    assert_eq!(desktop.windows.len(), 512);
    assert_eq!(layout_window_count(&desktop), 512);
}

/// The shortest title, in 4-byte scalars, whose 512-window desktop needs the layout cut.
fn shortest_title_that_cuts_layouts() -> usize {
    let (mut fits, mut cuts) = (0, MAX_TITLE_SCALARS);
    assert_eq!(
        synthetic_desktop(512, &FOUR_BYTE_SCALAR.repeat(cuts), 0).layouts_cut,
        Some(true)
    );
    while cuts - fits > 1 {
        let middle = fits + (cuts - fits) / 2;
        if synthetic_desktop(512, &FOUR_BYTE_SCALAR.repeat(middle), 0).layouts_cut == Some(true) {
            cuts = middle;
        } else {
            fits = middle;
        }
    }
    cuts
}

#[test]
fn layouts_are_cut_before_any_window() {
    let length = shortest_title_that_cuts_layouts();

    let desktop = synthetic_desktop(512, &FOUR_BYTE_SCALAR.repeat(length), 0);

    assert_eq!(desktop.layouts_cut, Some(true));
    assert!(
        desktop
            .workspaces
            .iter()
            .all(|workspace| workspace.layout == Some(None))
    );
    assert_eq!(desktop.windows.len(), 512, "no window was dropped");
    assert!(
        !desktop.truncated,
        "dropping layouts alone is not a list cut"
    );
    assert!(encoded_len(&desktop) <= MAX_DESKTOP_BYTES);
}

#[test]
fn scratchpad_entries_are_cut_before_windows() {
    let length = shortest_title_that_cuts_layouts();

    let desktop = synthetic_desktop(512, &FOUR_BYTE_SCALAR.repeat(length), 200);

    assert!(desktop.truncated);
    assert_eq!(desktop.layouts_cut, Some(true));
    assert_eq!(
        desktop.windows.len(),
        512,
        "windows are cut after the scratchpad"
    );
    let scratchpad = desktop.scratchpad.as_ref().expect("scratchpad");
    assert!(scratchpad.len() < 200);
    let kept: Vec<u64> = scratchpad.iter().map(|window| window.id).collect();
    let prefix: Vec<u64> = (50_000..).take(kept.len()).collect();
    assert_eq!(kept, prefix, "the scratchpad keeps a prefix");
}

#[test]
fn the_offline_desktop_has_empty_v2_members() {
    let desktop = offline_desktop();

    assert_eq!(desktop.outputs, Some(Vec::new()));
    assert_eq!(desktop.scratchpad, Some(Vec::new()));
    assert_eq!(desktop.layouts_cut, Some(false));
}
