//! State object of topic `desktop` (`docs/PROTOCOL.md` section 9 and 17.4).
//!
//! A member that 17.4 adds is optional: an older server leaves it out. Such a member is an
//! `Option`, and `None` means absent. A member of type `T or null` that 17.4 adds is an
//! `Option<Option<T>>`: `None` is absent, `Some(None)` is `null`. Both keep a decoded state
//! re-encoding to the same JSON value.

mod layout;
mod output;

use serde_json::Value;

use crate::icon::IconHash;
use crate::json::{self, InvalidMember, object_of};
use crate::limits::MAX_WORKSPACE_NUMBER;
use crate::message::JsonObject;

pub use layout::{BasisRect, Container, ContainerKind, Layout, Tile};
pub use output::{Output, Rect, ScratchpadWindow, Transform};

/// Spec 9.1 and 17.4.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    pub sway: SwayStatus,
    /// Sway's tree order, at most 16.
    pub outputs: Option<Vec<Output>>,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub scratchpad: Option<Vec<ScratchpadWindow>>,
    /// The server set every `Workspace.layout` to `null` for the size limit (17.4.5).
    pub layouts_cut: Option<bool>,
    pub truncated: bool,
}

/// Member `sway` of [`Desktop`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwayStatus {
    pub online: bool,
    /// `null` on the wire when offline.
    pub version: Option<String>,
}

/// Spec 9.2 and 17.4.1.
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
    /// Global logical pixels, without the bar. `Some(None)` when unknown.
    pub rect: Option<Option<Rect>>,
    /// `Some(None)` when the server dropped it.
    pub layout: Option<Option<Layout>>,
}

/// Spec 9.3 and 17.4.1.
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
    /// `Some(None)` when the server has no icon.
    pub icon: Option<Option<IconHash>>,
}

impl Desktop {
    pub fn encode(&self) -> JsonObject {
        let sway = object_of([
            ("online", self.sway.online.into()),
            ("version", self.sway.version.clone().into()),
        ]);
        let workspaces: Vec<Value> = self.workspaces.iter().map(encode_workspace).collect();
        let windows: Vec<Value> = self.windows.iter().map(encode_window).collect();
        let mut object = object_of([
            ("sway", sway.into()),
            ("workspaces", workspaces.into()),
            ("windows", windows.into()),
            ("truncated", self.truncated.into()),
        ]);
        let outputs = self
            .outputs
            .as_ref()
            .map(|outputs| Value::Array(outputs.iter().map(output::encode_output).collect()));
        let scratchpad = self.scratchpad.as_ref().map(|windows| {
            Value::Array(
                windows
                    .iter()
                    .map(output::encode_scratchpad_window)
                    .collect(),
            )
        });
        json::insert_present(&mut object, "outputs", outputs);
        json::insert_present(&mut object, "scratchpad", scratchpad);
        json::insert_present(
            &mut object,
            "layouts_cut",
            self.layouts_cut.map(Value::from),
        );
        object
    }

    /// Checks presence and types, and maps unknown enum values to their fallbacks. The text is
    /// still untrusted: a client passes it through `replace_unsafe` or `escape_for_terminal`
    /// before it shows it (spec 9.3 rule 4). A client then calls [`Desktop::normalize`].
    pub fn decode(object: &JsonObject) -> Result<Desktop, InvalidMember> {
        let sway = json::object(object, "sway")?;
        Ok(Desktop {
            sway: SwayStatus {
                online: json::boolean(sway, "online")?,
                version: json::nullable_string(sway, "version")?,
            },
            outputs: json::optional_object_array(object, "outputs", output::decode_output)?,
            workspaces: json::object_array(object, "workspaces", decode_workspace)?,
            windows: json::object_array(object, "windows", decode_window)?,
            scratchpad: json::optional_object_array(
                object,
                "scratchpad",
                output::decode_scratchpad_window,
            )?,
            layouts_cut: json::optional_boolean(object, "layouts_cut")?,
            truncated: json::boolean(object, "truncated")?,
        })
    }

    /// The client rule of 17.4.3 that needs the whole state: a layout tile whose window is
    /// not in `windows` is skipped.
    pub fn normalize(&mut self) {
        let is_known = layout::known_ids(self.windows.iter().map(|window| window.id));
        for workspace in &mut self.workspaces {
            if let Some(Some(layout)) = &mut workspace.layout {
                layout.retain_windows(&is_known);
            }
        }
    }
}

fn encode_workspace(workspace: &Workspace) -> Value {
    let mut object = object_of([
        ("id", workspace.id.into()),
        ("number", workspace.number.into()),
        ("name", workspace.name.as_str().into()),
        ("output", workspace.output.clone().into()),
        ("focused", workspace.focused.into()),
        ("visible", workspace.visible.into()),
        ("urgent", workspace.urgent.into()),
    ]);
    let rect = workspace
        .rect
        .map(|rect| rect.as_ref().map_or(Value::Null, Rect::encode));
    let layout = workspace
        .layout
        .as_ref()
        .map(|layout| layout.as_ref().map_or(Value::Null, Layout::encode));
    json::insert_present(&mut object, "rect", rect);
    json::insert_present(&mut object, "layout", layout);
    Value::Object(object)
}

fn encode_window(window: &Window) -> Value {
    let mut object = object_of([
        ("id", window.id.into()),
        ("workspace_id", window.workspace_id.into()),
        ("app_id", window.app_id.clone().into()),
        ("title", window.title.clone().into()),
        ("focused", window.focused.into()),
        ("urgent", window.urgent.into()),
        ("floating", window.floating.into()),
        ("fullscreen", window.fullscreen.into()),
    ]);
    json::insert_present(&mut object, "icon", window.icon.map(output::encode_icon));
    Value::Object(object)
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
        rect: json::optional_nullable(workspace, "rect", |rect| Rect::decode_value(rect, "rect"))?,
        layout: json::optional_nullable(workspace, "layout", |layout| {
            Layout::decode_value(layout, "layout")
        })?,
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
        icon: json::optional_nullable(window, "icon", |hash| IconHash::decode_value(hash, "icon"))?,
    })
}
