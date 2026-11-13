//! The geometry fixtures hold the cases the v2 layout export needs (`docs/PROTOCOL.md` 17.4).

use serde_json::Value;
use swayconnect_testkit::fixtures::{
    NOTES_APP_ID, binding_event, layout_tree, layout_tree_resized, layout_workspaces,
};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture is JSON")
}

/// Every node of `node`, depth first, tiled children before floating ones.
fn all_nodes(node: &Value) -> Vec<&Value> {
    let mut found = vec![node];
    for key in ["nodes", "floating_nodes"] {
        for child in node[key].as_array().into_iter().flatten() {
            found.extend(all_nodes(child));
        }
    }
    found
}

fn find(tree: &Value, id: u64) -> &Value {
    all_nodes(tree)
        .into_iter()
        .find(|node| node["id"] == id)
        .unwrap_or_else(|| panic!("node {id} in the tree"))
}

fn is_window(node: &Value) -> bool {
    node["layout"] == "none"
}

fn rect(node: &Value) -> [i64; 4] {
    ["x", "y", "width", "height"].map(|key| node["rect"][key].as_i64().expect("rect member"))
}

#[test]
fn every_node_has_a_unique_id_and_a_rect() {
    let tree = parse(&layout_tree());
    let nodes = all_nodes(&tree);
    let mut ids: Vec<u64> = nodes
        .iter()
        .map(|node| node["id"].as_u64().expect("id"))
        .collect();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(ids, (1..=20).collect::<Vec<_>>());
    assert!(nodes.iter().all(|node| node["rect"]["width"].is_i64()));
}

#[test]
fn a_tiled_window_lies_inside_its_workspace_rect() {
    let tree = parse(&layout_tree());

    for workspace_id in [4, 14] {
        let [wx, wy, ww, wh] = rect(find(&tree, workspace_id));
        let tiled = find(&tree, workspace_id)["nodes"]
            .as_array()
            .expect("nodes");
        for node in tiled.iter().flat_map(all_nodes) {
            let [x, y, w, h] = rect(node);
            assert!(x >= wx && y >= wy && x + w <= wx + ww && y + h <= wy + wh);
        }
    }
}

#[test]
fn the_tree_has_tabbed_stacked_floating_fullscreen_and_scratchpad_cases() {
    let tree = parse(&layout_tree());

    let tabbed = find(&tree, 11);
    assert_eq!(tabbed["layout"], "tabbed");
    assert_eq!(tabbed["focus"][0], 12);
    assert_eq!(tabbed["nodes"][1]["id"], 12);
    assert_eq!(find(&tree, 17)["layout"], "stacked");
    assert_eq!(find(&tree, 4)["floating_nodes"][0]["type"], "floating_con");
    assert_eq!(find(&tree, 7)["fullscreen_mode"], 1);
    assert_eq!(find(&tree, 7)["app_id"], Value::Null);
    let scratch = find(&tree, 20);
    assert_eq!(scratch["name"], "__i3_scratch");
    assert_eq!(scratch["floating_nodes"][0]["scratchpad_state"], "fresh");
    assert_eq!(find(&tree, 6)["app_id"], NOTES_APP_ID);
    let focused: Vec<&Value> = all_nodes(&tree)
        .into_iter()
        .filter(|node| node["focused"] == true)
        .collect();
    assert_eq!(focused.len(), 1);
    assert_eq!(focused[0]["id"], 12);
}

#[test]
fn the_outputs_carry_power_scale_transform_make_and_model() {
    let tree = parse(&layout_tree());

    let side = find(&tree, 19);
    assert_eq!(side["type"], "output");
    assert_eq!(side["scale"], 1.25);
    assert_eq!(side["transform"], "90");
    assert_eq!(side["power"], true);
    assert!(side["make"].is_string() && side["model"].is_string());
    assert_eq!(find(&tree, 2)["current_workspace"], "1");
    assert_eq!(find(&tree, 3)["name"], "__i3");
}

#[test]
fn the_workspace_list_matches_the_tree() {
    let tree = parse(&layout_tree());
    let listed = parse(&layout_workspaces());

    let listed = listed.as_array().expect("array");
    assert_eq!(listed.len(), 3);
    for workspace in listed {
        let in_tree = find(&tree, workspace["id"].as_u64().expect("id"));
        assert_eq!(workspace["name"], in_tree["name"]);
        assert_eq!(workspace["output"], in_tree["output"]);
        assert_eq!(workspace["rect"], in_tree["rect"]);
    }
}

#[test]
fn the_resized_tree_differs_only_in_rects_of_workspace_4() {
    let mut before = parse(&layout_tree());
    let mut after = parse(&layout_tree_resized());

    assert_ne!(rect(find(&before, 6)), rect(find(&after, 6)));
    assert_eq!(rect(find(&before, 4)), rect(find(&after, 4)));
    for tree in [&mut before, &mut after] {
        strip_rects(&mut tree["nodes"][1]["nodes"][0]);
    }
    assert_eq!(before, after);
}

fn strip_rects(node: &mut Value) {
    if let Some(object) = node.as_object_mut() {
        object.remove("rect");
    }
    for key in ["nodes", "floating_nodes"] {
        for child in node[key].as_array_mut().into_iter().flatten() {
            strip_rects(child);
        }
    }
}

#[test]
fn every_window_is_a_leaf() {
    let tree = parse(&layout_tree());

    for node in all_nodes(&tree).into_iter().filter(|node| is_window(node)) {
        assert!(node["nodes"].as_array().expect("nodes").is_empty());
        assert!(node["app_id"].is_string() || node["window_properties"]["class"].is_string());
    }
}

#[test]
fn the_binding_event_is_a_run_change() {
    let event = parse(&binding_event());

    assert_eq!(event["change"], "run");
    assert!(event["binding"]["command"].is_string());
}
