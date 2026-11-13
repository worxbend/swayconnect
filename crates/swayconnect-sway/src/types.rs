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
///
/// Output-only fields (`active`, `power`, `scale`, `transform`, `make`, `model`,
/// `current_workspace`) are `None` on other node types.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
    /// App id that a sandbox engine (flatpak) set through `security-context-v1`; sway 1.10+.
    pub sandbox_app_id: Option<String>,
    pub fullscreen_mode: Option<i64>,
    pub focused: bool,
    pub urgent: bool,
    /// Area in global logical pixels. A workspace's rect excludes the bar.
    pub rect: Option<Rect>,
    /// `splith`, `splitv`, `tabbed`, `stacked`, `output`, `none` or a value sway adds later.
    pub layout: Option<String>,
    /// Ids of child nodes (tiled and floating), most recently focused first.
    pub focus: Vec<u64>,
    /// `none`, `fresh` or `changed`; anything but `none` marks a scratchpad window.
    pub scratchpad_state: Option<String>,
    /// A view that currently inhibits idle.
    pub inhibit_idle: Option<bool>,
    pub active: Option<bool>,
    pub power: Option<bool>,
    pub scale: Option<f64>,
    /// `normal`, `90`, `180`, `270`, `flipped`, `flipped-90`, ... or a value sway adds later.
    pub transform: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    /// Name of the workspace an output shows.
    pub current_workspace: Option<String>,
    pub nodes: Vec<Node>,
    pub floating_nodes: Vec<Node>,
}

/// A rectangle in global logical pixels, as sway reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// Xwayland properties of a [`Node`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WindowProperties {
    pub class: Option<String>,
    pub instance: Option<String>,
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
                class: Some("XTerm".to_owned()),
                instance: Some("xterm".to_owned()),
            })
        );
    }

    #[test]
    fn layout_and_output_fields_parse() {
        let output: Node = serde_json::from_str(
            r#"{"id":3,"type":"output","name":"DP-1","active":true,"power":false,"dpms":false,
                "scale":1.25,"transform":"90","make":"Dell Inc.","model":"P3223QE",
                "current_workspace":"2","layout":"output","focus":[4,7],
                "rect":{"x":-1920,"y":0,"width":3072,"height":1728}}"#,
        )
        .expect("output parses");
        assert_eq!(
            (output.active, output.power, output.scale),
            (Some(true), Some(false), Some(1.25))
        );
        assert_eq!(output.transform.as_deref(), Some("90"));
        assert_eq!(output.make.as_deref(), Some("Dell Inc."));
        assert_eq!(output.model.as_deref(), Some("P3223QE"));
        assert_eq!(output.current_workspace.as_deref(), Some("2"));
        assert_eq!(output.layout.as_deref(), Some("output"));
        assert_eq!(output.focus, [4, 7]);
        assert_eq!(
            output.rect,
            Some(Rect {
                x: -1920,
                y: 0,
                width: 3072,
                height: 1728
            })
        );

        let view: Node = serde_json::from_str(
            r#"{"id":9,"type":"floating_con","layout":"none","scratchpad_state":"fresh",
                "inhibit_idle":true,"focus":[]}"#,
        )
        .expect("view parses");
        assert_eq!(view.scratchpad_state.as_deref(), Some("fresh"));
        assert_eq!(view.inhibit_idle, Some(true));
        assert_eq!((view.power, view.scale, view.rect), (None, None, None));
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
