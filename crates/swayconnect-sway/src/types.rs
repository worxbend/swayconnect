//! Lenient sway reply types (`docs/DESIGN.md` 4.3 rule 2).
//!
//! Only the fields the daemon uses. Every field has a default and enum-like values stay
//! strings, so a new sway value degrades and never fails the whole message.

use serde::Deserialize;

/// Reply to `GET_VERSION`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Version {
    pub human_readable: String,
}

/// One element of the reply to `GET_WORKSPACES`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WorkspaceInfo {
    pub id: u64,
    /// -1 for a workspace with no number (`man 7 sway-ipc`).
    pub num: Option<i64>,
    pub name: Option<String>,
    pub output: Option<String>,
    pub focused: bool,
    pub visible: bool,
    pub urgent: bool,
}

/// A node of the reply to `GET_TREE`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Node {
    pub id: u64,
    pub name: Option<String>,
    /// `root`, `output`, `workspace`, `con`, `floating_con` or a value sway adds later.
    #[serde(rename = "type")]
    pub node_type: String,
    pub app_id: Option<String>,
    pub pid: Option<i64>,
    /// X11 window id of an Xwayland view.
    pub window: Option<i64>,
    pub window_properties: Option<WindowProperties>,
    pub fullscreen_mode: Option<i64>,
    pub focused: bool,
    pub urgent: bool,
    pub nodes: Vec<Node>,
    pub floating_nodes: Vec<Node>,
}

/// Xwayland properties of a [`Node`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WindowProperties {
    pub class: Option<String>,
}

/// Reply to `SUBSCRIBE`, and one element of the reply to `RUN_COMMAND`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Outcome {
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_keeps_the_used_fields_and_ignores_the_rest() {
        let tree: Node = serde_json::from_str(
            r#"{"id":1,"type":"root","name":"root","rect":{"x":0},"nodes":[
                {"id":4,"type":"workspace","name":"1","num":1,"window":null,"nodes":[
                    {"id":9,"type":"con","name":"term","app_id":"foot","pid":77,
                     "fullscreen_mode":1,"focused":true,"nodes":[],"floating_nodes":[]}],
                 "floating_nodes":[
                    {"id":10,"type":"floating_con","name":null,"app_id":null,"window":123,
                     "window_properties":{"class":"XTerm","instance":"xterm"}}]}]}"#,
        )
        .expect("tree parses");
        let workspace = &tree.nodes[0];
        assert_eq!(workspace.node_type, "workspace");
        assert_eq!(workspace.window, None);
        let tiled = &workspace.nodes[0];
        assert_eq!(
            (tiled.id, tiled.pid, tiled.fullscreen_mode),
            (9, Some(77), Some(1))
        );
        assert_eq!(tiled.app_id.as_deref(), Some("foot"));
        assert!(tiled.focused && !tiled.urgent);
        let floating = &workspace.floating_nodes[0];
        assert_eq!(floating.name, None);
        assert_eq!(
            floating.window_properties,
            Some(WindowProperties {
                class: Some("XTerm".to_owned())
            })
        );
    }

    #[test]
    fn an_empty_object_is_a_default_value() {
        assert_eq!(
            serde_json::from_str::<Node>("{}").expect("node"),
            Node::default()
        );
        assert_eq!(
            serde_json::from_str::<WorkspaceInfo>("{}").expect("workspace"),
            WorkspaceInfo::default()
        );
        assert_eq!(
            serde_json::from_str::<Version>("{}").expect("version"),
            Version::default()
        );
    }

    #[test]
    fn a_new_enum_like_value_does_not_fail_the_message() {
        let node: Node = serde_json::from_str(r#"{"id":3,"type":"hologram"}"#).expect("node");
        assert_eq!(node.node_type, "hologram");
    }

    #[test]
    fn a_workspace_without_a_number_reports_minus_one() {
        let workspaces: Vec<WorkspaceInfo> = serde_json::from_str(
            r#"[{"id":5,"num":-1,"name":"mail","output":"DP-1","focused":false,"visible":true,"urgent":false}]"#,
        )
        .expect("workspaces");
        assert_eq!(workspaces[0].num, Some(-1));
        assert_eq!(workspaces[0].output.as_deref(), Some("DP-1"));
        assert!(workspaces[0].visible);
    }
}
