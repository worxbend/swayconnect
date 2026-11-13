//! `Rect`, `Output` and `ScratchpadWindow` of topic `desktop` (`docs/PROTOCOL.md` 17.4.2).

use serde_json::Value;

use crate::code::wire_names;
use crate::icon::IconHash;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::message::JsonObject;

/// An area in global logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// `sint`.
    pub x: i64,
    /// `sint`.
    pub y: i64,
    pub w: u64,
    pub h: u64,
}

/// `Output.transform`. Decoding maps an unknown value to [`Transform::Normal`] (17.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

wire_names!(Transform {
    Normal => "normal",
    Rotate90 => "90",
    Rotate180 => "180",
    Rotate270 => "270",
    Flipped => "flipped",
    Flipped90 => "flipped-90",
    Flipped180 => "flipped-180",
    Flipped270 => "flipped-270",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    /// Make and model, for display only.
    pub model: Option<String>,
    pub rect: Rect,
    /// Scale times 1000.
    pub scale_milli: u64,
    pub transform: Transform,
    pub active: bool,
    pub power: bool,
    pub current_workspace_id: Option<u64>,
}

/// A window hidden in the scratchpad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchpadWindow {
    pub id: u64,
    pub app_id: Option<String>,
    /// `null` without scope `state.titles`.
    pub title: Option<String>,
    pub icon: Option<IconHash>,
}

impl Rect {
    pub(crate) fn encode(&self) -> Value {
        Value::Object(object_of([
            ("x", self.x.into()),
            ("y", self.y.into()),
            ("w", self.w.into()),
            ("h", self.h.into()),
        ]))
    }

    pub(crate) fn decode_value(value: &Value, name: &'static str) -> Member<Rect> {
        let rect = value.as_object().ok_or(InvalidMember(name))?;
        Ok(Rect {
            x: json::sint(rect, "x")?,
            y: json::sint(rect, "y")?,
            w: json::int(rect, "w")?,
            h: json::int(rect, "h")?,
        })
    }
}

pub(crate) fn encode_output(output: &Output) -> Value {
    Value::Object(object_of([
        ("name", output.name.as_str().into()),
        ("model", output.model.clone().into()),
        ("rect", output.rect.encode()),
        ("scale_milli", output.scale_milli.into()),
        ("transform", output.transform.as_str().into()),
        ("active", output.active.into()),
        ("power", output.power.into()),
        ("current_workspace_id", output.current_workspace_id.into()),
    ]))
}

pub(crate) fn decode_output(output: &JsonObject) -> Member<Output> {
    const RECT: &str = "rect";
    Ok(Output {
        name: json::string(output, "name")?.to_owned(),
        model: json::nullable_string(output, "model")?,
        rect: Rect::decode_value(output.get(RECT).ok_or(InvalidMember(RECT))?, RECT)?,
        scale_milli: json::int(output, "scale_milli")?,
        transform: Transform::from_wire(json::string(output, "transform")?)
            .unwrap_or(Transform::Normal),
        active: json::boolean(output, "active")?,
        power: json::boolean(output, "power")?,
        current_workspace_id: json::nullable_int(output, "current_workspace_id")?,
    })
}

pub(crate) fn encode_scratchpad_window(window: &ScratchpadWindow) -> Value {
    Value::Object(object_of([
        ("id", window.id.into()),
        ("app_id", window.app_id.clone().into()),
        ("title", window.title.clone().into()),
        ("icon", encode_icon(window.icon)),
    ]))
}

pub(crate) fn decode_scratchpad_window(window: &JsonObject) -> Member<ScratchpadWindow> {
    Ok(ScratchpadWindow {
        id: json::int(window, "id")?,
        app_id: json::nullable_string(window, "app_id")?,
        title: json::nullable_string(window, "title")?,
        icon: json::nullable(window, "icon", |hash| IconHash::decode_value(hash, "icon"))?,
    })
}

pub(crate) fn encode_icon(icon: Option<IconHash>) -> Value {
    icon.map_or(Value::Null, IconHash::encode_value)
}
