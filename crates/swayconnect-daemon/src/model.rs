//! Pure function: sway tree, workspaces and version to `Desktop`. Written by slice S6b.
//!
//! Every string in the input is set by an application or by sway and is untrusted
//! (`docs/DESIGN.md` 4.5, `docs/PROTOCOL.md` 9.3).

use std::collections::HashSet;

use swayconnect_protocol::limits::{
    MAX_DESKTOP_BYTES, MAX_LABEL_SCALARS, MAX_TITLE_SCALARS, MAX_WINDOWS, MAX_WORKSPACE_NUMBER,
    MAX_WORKSPACES,
};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{Desktop, SwayStatus, Window, Workspace};
use swayconnect_sway::{Node, Version, WorkspaceInfo};

const INTERNAL_OUTPUT: &str = "__i3";
const SCRATCHPAD_WORKSPACE: &str = "__i3_scratch";
const FLOATING_TYPE: &str = "floating_con";
const TILED_TYPE: &str = "con";
const WORKSPACE_TYPE: &str = "workspace";

/// The state while sway is away.
pub fn offline_desktop() -> Desktop {
    Desktop {
        sway: SwayStatus {
            online: false,
            version: None,
        },
        workspaces: Vec::new(),
        windows: Vec::new(),
        truncated: false,
    }
}

/// The state of a running sway, cut to the limits of `docs/PROTOCOL.md` section 14.
pub fn online_desktop(tree: &Node, listed: &[WorkspaceInfo], version: &Version) -> Desktop {
    let mut workspaces: Vec<Workspace> = listed
        .iter()
        .filter(|info| info.name.as_deref() != Some(SCRATCHPAD_WORKSPACE))
        .map(workspace)
        .collect();
    let mut windows = windows_of_listed_workspaces(tree, &workspaces);

    let truncated = workspaces.len() > MAX_WORKSPACES || windows.len() > MAX_WINDOWS;
    workspaces.truncate(MAX_WORKSPACES);
    windows.truncate(MAX_WINDOWS);

    let mut desktop = Desktop {
        sway: SwayStatus {
            online: true,
            version: Some(label(&version.human_readable)),
        },
        workspaces,
        windows,
        truncated,
    };
    cut_to_size(&mut desktop);
    desktop
}

fn workspace(info: &WorkspaceInfo) -> Workspace {
    Workspace {
        id: info.id,
        number: info.num.and_then(workspace_number),
        name: label(info.name.as_deref().unwrap_or_default()),
        output: info.output.as_deref().map(label),
        focused: info.focused,
        visible: info.visible,
        urgent: info.urgent,
    }
}

/// Sway reports -1 for a workspace with no number (`man 7 sway-ipc`).
fn workspace_number(num: i64) -> Option<u32> {
    u64::try_from(num)
        .ok()
        .filter(|number| *number <= MAX_WORKSPACE_NUMBER)
        .and_then(|number| u32::try_from(number).ok())
}

/// Windows in layout tree order. The tree and the workspace list come from two requests;
/// a window of a workspace the list lacks is left out, so every `workspace_id` has its entry.
fn windows_of_listed_workspaces(tree: &Node, workspaces: &[Workspace]) -> Vec<Window> {
    let listed_ids: HashSet<u64> = workspaces.iter().map(|workspace| workspace.id).collect();
    let mut windows = Vec::new();
    let outputs = tree
        .nodes
        .iter()
        .filter(|output| output.name.as_deref() != Some(INTERNAL_OUTPUT));
    for workspace_node in outputs.flat_map(|output| &output.nodes) {
        if workspace_node.node_type == WORKSPACE_TYPE && listed_ids.contains(&workspace_node.id) {
            let on_workspace = Placement {
                workspace_id: workspace_node.id,
                is_floating: false,
            };
            collect_windows(workspace_node, on_workspace, &mut windows);
        }
    }
    windows
}

/// Where a container sits. Its descendants inherit it.
#[derive(Clone, Copy)]
struct Placement {
    workspace_id: u64,
    /// Sway gives the type `floating_con` only to the top container of a floating group; the
    /// windows of a floating split have the type `con` and float with it.
    is_floating: bool,
}

/// Tiled children first, then floating ones. The depth is bounded by the JSON parser.
fn collect_windows(container: &Node, placement: Placement, windows: &mut Vec<Window>) {
    let floating = Placement {
        is_floating: true,
        ..placement
    };
    let tiled_children = container.nodes.iter().map(|child| (child, placement));
    let floating_children = container
        .floating_nodes
        .iter()
        .map(|child| (child, floating));
    for (child, placement) in tiled_children.chain(floating_children) {
        if is_window(child) {
            windows.push(window(child, placement));
        } else {
            collect_windows(child, placement, windows);
        }
    }
}

fn is_window(node: &Node) -> bool {
    let is_container_type = node.node_type == TILED_TYPE || node.node_type == FLOATING_TYPE;
    let has_client = node.pid.is_some() || node.app_id.is_some() || node.window.is_some();
    let is_leaf = node.nodes.is_empty() && node.floating_nodes.is_empty();
    is_container_type && has_client && is_leaf
}

fn window(node: &Node, placement: Placement) -> Window {
    let xwayland_class = || node.window_properties.as_ref()?.class.as_deref();
    Window {
        id: node.id,
        workspace_id: placement.workspace_id,
        app_id: node.app_id.as_deref().or_else(xwayland_class).map(label),
        title: node.name.as_deref().map(title),
        focused: node.focused,
        urgent: node.urgent,
        floating: placement.is_floating || node.node_type == FLOATING_TYPE,
        fullscreen: node.fullscreen_mode.is_some_and(|mode| mode != 0),
    }
}

fn title(text: &str) -> String {
    safe_prefix(text, MAX_TITLE_SCALARS)
}

fn label(text: &str) -> String {
    safe_prefix(text, MAX_LABEL_SCALARS)
}

/// Replaces first, then cuts (`docs/PROTOCOL.md` 9.3 rule 3).
fn safe_prefix(text: &str, max_scalars: usize) -> String {
    replace_unsafe(text).chars().take(max_scalars).collect()
}

/// Drops windows from the end, then workspaces from the end, until the encoded state fits.
fn cut_to_size(desktop: &mut Desktop) {
    if fits(desktop) {
        return;
    }
    desktop.truncated = true;
    keep_fitting_prefix(desktop, |desktop| &mut desktop.windows);
    if !fits(desktop) {
        keep_fitting_prefix(desktop, |desktop| &mut desktop.workspaces);
    }
}

fn fits(desktop: &Desktop) -> bool {
    // A state that cannot be encoded cannot be sent either, so it does not fit.
    serde_json::to_vec(&desktop.encode()).is_ok_and(|encoded| encoded.len() <= MAX_DESKTOP_BYTES)
}

/// Cuts one list to its longest prefix with which `desktop` fits; to nothing if none does.
fn keep_fitting_prefix<T: Clone>(desktop: &mut Desktop, list: fn(&mut Desktop) -> &mut Vec<T>) {
    let mut entries = std::mem::take(list(desktop));
    // Invariant: a prefix of `fitting` entries fits or is empty; no prefix over `limit` fits.
    let (mut fitting, mut limit) = (0, entries.len());
    while fitting < limit {
        let candidate = fitting + (limit - fitting).div_ceil(2);
        *list(desktop) = entries[..candidate].to_vec();
        if fits(desktop) {
            fitting = candidate;
        } else {
            limit = candidate - 1;
        }
    }
    entries.truncate(fitting);
    *list(desktop) = entries;
}

#[cfg(test)]
mod tests {
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
        let floating: Vec<(u64, bool)> =
            windows.map(|window| (window.id, window.floating)).collect();
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
}
