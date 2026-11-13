//! Default replies of the fake sway: one output, two workspaces, two windows.
//! [`layout_tree`] and its siblings add geometry for the v2 desktop additions.
//! Every name and title is invented.

mod layout;

use serde_json::{Value, json};

pub use crate::xdg_data::{NOTES_APP_ID, NOTES_DESKTOP_ENTRY, NOTES_ICON_PNG};
pub use layout::{binding_event, layout_tree, layout_tree_resized, layout_workspaces};

/// Default `GET_VERSION` reply.
pub fn version() -> String {
    json!({
        "human_readable": "sway version 1.12",
        "variant": "sway",
        "major": 1,
        "minor": 12,
        "patch": 0,
        "loaded_config_file_name": "/nonexistent/sway/config"
    })
    .to_string()
}

/// Default `GET_WORKSPACES` reply.
pub fn workspaces() -> String {
    json!([
        {
            "id": 4, "type": "workspace", "num": 1, "name": "1", "output": "TEST-1",
            "focused": true, "visible": true, "urgent": false
        },
        {
            "id": 5, "type": "workspace", "num": 2, "name": "2: web", "output": "TEST-1",
            "focused": false, "visible": false, "urgent": false
        }
    ])
    .to_string()
}

/// Default `GET_TREE` reply. Window ids are 6 and 7, workspace ids 4 and 5.
pub fn tree() -> String {
    let notes_window = json!({
        "id": 6, "type": "con", "name": "Packing list for the lake trip", "app_id": "notes",
        "pid": 1001, "focused": true, "urgent": false, "fullscreen_mode": 0,
        "nodes": [], "floating_nodes": []
    });
    let browser_window = json!({
        "id": 7, "type": "con", "name": "Sourdough for beginners", "app_id": null,
        "window": 4194311, "window_properties": { "class": "Browser" },
        "pid": 1002, "focused": false, "urgent": false, "fullscreen_mode": 0,
        "nodes": [], "floating_nodes": []
    });
    json!({
        "id": 1, "type": "root", "name": "root",
        "nodes": [{
            "id": 2, "type": "output", "name": "TEST-1",
            "nodes": [
                workspace_node(4, 1, "1", notes_window),
                workspace_node(5, 2, "2: web", browser_window)
            ],
            "floating_nodes": []
        }],
        "floating_nodes": []
    })
    .to_string()
}

fn workspace_node(id: u64, num: i32, name: &str, window: Value) -> Value {
    json!({
        "id": id, "type": "workspace", "num": num, "name": name, "output": "TEST-1",
        "focused": false, "urgent": false, "fullscreen_mode": 1,
        "nodes": [window], "floating_nodes": []
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is JSON")
    }

    fn collect_nodes<'a>(node: &'a Value, node_type: &str, found: &mut Vec<&'a Value>) {
        if node["type"] == node_type {
            found.push(node);
        }
        for child in node["nodes"].as_array().into_iter().flatten() {
            collect_nodes(child, node_type, found);
        }
    }

    #[test]
    fn the_default_tree_has_two_workspaces_and_two_windows() {
        let tree = parse(&tree());
        let (mut workspaces, mut windows) = (Vec::new(), Vec::new());
        collect_nodes(&tree, "workspace", &mut workspaces);
        collect_nodes(&tree, "con", &mut windows);

        assert_eq!(workspaces.len(), 2);
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().all(|window| window["name"].is_string()));
    }

    #[test]
    fn the_workspace_list_matches_the_tree() {
        let tree = parse(&tree());
        let mut tree_workspaces = Vec::new();
        collect_nodes(&tree, "workspace", &mut tree_workspaces);
        let listed = parse(&workspaces());
        let listed = listed.as_array().expect("array");

        assert_eq!(listed.len(), 2);
        for (listed, in_tree) in listed.iter().zip(tree_workspaces) {
            assert_eq!(listed["id"], in_tree["id"]);
            assert_eq!(listed["name"], in_tree["name"]);
            assert_eq!(listed["num"], in_tree["num"]);
        }
    }

    #[test]
    fn the_version_reply_names_sway() {
        assert_eq!(parse(&version())["variant"], "sway");
    }
}
