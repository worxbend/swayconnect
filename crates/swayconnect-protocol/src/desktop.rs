//! State object of topic `desktop` (`docs/PROTOCOL.md` section 9).

use serde_json::Value;

use crate::json::{self, InvalidMember, object_of};
use crate::limits::MAX_WORKSPACE_NUMBER;
use crate::message::JsonObject;

/// Spec 9.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    pub sway: SwayStatus,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub truncated: bool,
}

/// Member `sway` of [`Desktop`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwayStatus {
    pub online: bool,
    /// `null` on the wire when offline.
    pub version: Option<String>,
}

/// Spec 9.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: u64,
    /// `null` on the wire for a workspace with no number.
    pub number: Option<u32>,
    pub name: String,
    pub output: Option<String>,
    pub focused: bool,
    pub visible: bool,
    pub urgent: bool,
}

/// Spec 9.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub id: u64,
    pub workspace_id: u64,
    pub app_id: Option<String>,
    /// `null` on the wire without scope `state.titles` or when the window has no title.
    pub title: Option<String>,
    pub focused: bool,
    pub urgent: bool,
    pub floating: bool,
    pub fullscreen: bool,
}

impl Desktop {
    pub fn encode(&self) -> JsonObject {
        let sway = object_of([
            ("online", self.sway.online.into()),
            ("version", self.sway.version.clone().into()),
        ]);
        let workspaces: Vec<Value> = self.workspaces.iter().map(encode_workspace).collect();
        let windows: Vec<Value> = self.windows.iter().map(encode_window).collect();
        object_of([
            ("sway", sway.into()),
            ("workspaces", workspaces.into()),
            ("windows", windows.into()),
            ("truncated", self.truncated.into()),
        ])
    }

    /// Checks presence and types. The text is still untrusted: a client passes it through
    /// `replace_unsafe` or `escape_for_terminal` before it shows it (spec 9.3 rule 4).
    pub fn decode(object: &JsonObject) -> Result<Desktop, InvalidMember> {
        let sway = json::object(object, "sway")?;
        Ok(Desktop {
            sway: SwayStatus {
                online: json::boolean(sway, "online")?,
                version: json::nullable_string(sway, "version")?,
            },
            workspaces: json::object_array(object, "workspaces", decode_workspace)?,
            windows: json::object_array(object, "windows", decode_window)?,
            truncated: json::boolean(object, "truncated")?,
        })
    }
}

fn encode_workspace(workspace: &Workspace) -> Value {
    Value::Object(object_of([
        ("id", workspace.id.into()),
        ("number", workspace.number.into()),
        ("name", workspace.name.as_str().into()),
        ("output", workspace.output.clone().into()),
        ("focused", workspace.focused.into()),
        ("visible", workspace.visible.into()),
        ("urgent", workspace.urgent.into()),
    ]))
}

fn encode_window(window: &Window) -> Value {
    Value::Object(object_of([
        ("id", window.id.into()),
        ("workspace_id", window.workspace_id.into()),
        ("app_id", window.app_id.clone().into()),
        ("title", window.title.clone().into()),
        ("focused", window.focused.into()),
        ("urgent", window.urgent.into()),
        ("floating", window.floating.into()),
        ("fullscreen", window.fullscreen.into()),
    ]))
}

fn decode_workspace(workspace: &JsonObject) -> Result<Workspace, InvalidMember> {
    Ok(Workspace {
        id: json::int(workspace, "id")?,
        number: decode_number(workspace)?,
        name: json::string(workspace, "name")?.to_owned(),
        output: json::nullable_string(workspace, "output")?,
        focused: json::boolean(workspace, "focused")?,
        visible: json::boolean(workspace, "visible")?,
        urgent: json::boolean(workspace, "urgent")?,
    })
}

/// `int or null`, 0 to 2147483647.
fn decode_number(workspace: &JsonObject) -> Result<Option<u32>, InvalidMember> {
    const NAME: &str = "number";
    match workspace.get(NAME) {
        Some(Value::Null) => Ok(None),
        Some(value) => json::as_int(value)
            .filter(|number| *number <= MAX_WORKSPACE_NUMBER)
            .and_then(|number| u32::try_from(number).ok())
            .map(Some)
            .ok_or(InvalidMember(NAME)),
        None => Err(InvalidMember(NAME)),
    }
}

fn decode_window(window: &JsonObject) -> Result<Window, InvalidMember> {
    Ok(Window {
        id: json::int(window, "id")?,
        workspace_id: json::int(window, "workspace_id")?,
        app_id: json::nullable_string(window, "app_id")?,
        title: json::nullable_string(window, "title")?,
        focused: json::boolean(window, "focused")?,
        urgent: json::boolean(window, "urgent")?,
        floating: json::boolean(window, "floating")?,
        fullscreen: json::boolean(window, "fullscreen")?,
    })
}
