//! `Layout` and `Tile` of topic `desktop` (`docs/PROTOCOL.md` 17.4.3).
//!
//! Decoding applies the client rules that need no other part of the state: an unknown
//! container kind is `h`, an `r` value above 10000 is 10000, an absent or out-of-range `a` of a
//! tabbed or stacked container is 0. [`Layout::retain_windows`] applies the rule that needs
//! `windows`. Nesting depth is bounded by the JSON parser's recursion limit (serde_json: 128).

use std::collections::HashSet;

use serde_json::Value;

use crate::code::wire_names;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::MAX_BASIS_POINTS;
use crate::message::JsonObject;

/// The tiling layout of one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Workspace width and height in logical pixels.
    pub size: [u64; 2],
    /// The window that is fullscreen on this workspace.
    pub fullscreen: Option<u64>,
    pub tiled: Option<Tile>,
    /// Bottom first, top last.
    pub floating: Vec<Tile>,
}

/// `[x, y, w, h]` in basis points of the workspace, each 0 to 10000.
pub type BasisRect = [u16; 4];

/// A window tile (member `w`) or a container tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tile {
    Window { id: u64, r: BasisRect },
    Container(Container),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub kind: ContainerKind,
    pub r: BasisRect,
    /// One or more, in sway order.
    pub children: Vec<Tile>,
    /// Member `a`: index into `children` of the child shown. Always within range after decode.
    pub active: Option<usize>,
}

/// `Container.k`. Decoding maps an unknown value to [`ContainerKind::SplitH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    SplitH,
    SplitV,
    Tabbed,
    Stacked,
}

wire_names!(ContainerKind {
    SplitH => "h",
    SplitV => "v",
    Tabbed => "t",
    Stacked => "s",
});

impl ContainerKind {
    /// Tabbed and stacked containers show one child.
    pub const fn shows_one_child(self) -> bool {
        matches!(self, ContainerKind::Tabbed | ContainerKind::Stacked)
    }
}

impl Layout {
    pub(crate) fn encode(&self) -> Value {
        let size: Vec<Value> = self.size.iter().map(|side| (*side).into()).collect();
        let floating: Vec<Value> = self.floating.iter().map(Tile::encode).collect();
        Value::Object(object_of([
            ("size", size.into()),
            ("fullscreen", self.fullscreen.into()),
            (
                "tiled",
                self.tiled.as_ref().map_or(Value::Null, Tile::encode),
            ),
            ("floating", floating.into()),
        ]))
    }

    pub(crate) fn decode_value(value: &Value, name: &'static str) -> Member<Layout> {
        let layout = value.as_object().ok_or(InvalidMember(name))?;
        Ok(Layout {
            size: json::int_array(layout, "size")?,
            fullscreen: json::nullable_int(layout, "fullscreen")?,
            tiled: json::nullable(layout, "tiled", |tile| Tile::decode_value(tile, "tiled"))?,
            floating: json::array(layout, "floating")?
                .iter()
                .map(|tile| Tile::decode_value(tile, "floating"))
                .collect::<Member<Vec<Tile>>>()?,
        })
    }

    /// Client rule 1 of 17.4.3: skips each window tile whose id `is_known` refuses. A container
    /// left without children is skipped too, and `a` keeps pointing at the same child (or 0).
    pub fn retain_windows(&mut self, is_known: &impl Fn(u64) -> bool) {
        self.tiled = self
            .tiled
            .take()
            .and_then(|tile| tile.retain_windows(is_known));
        self.floating = std::mem::take(&mut self.floating)
            .into_iter()
            .filter_map(|tile| tile.retain_windows(is_known))
            .collect();
    }
}

impl Tile {
    fn encode(&self) -> Value {
        match self {
            Tile::Window { id, r } => {
                Value::Object(object_of([("w", (*id).into()), ("r", encode_r(r))]))
            }
            Tile::Container(container) => {
                let children: Vec<Value> = container.children.iter().map(Tile::encode).collect();
                let mut object = object_of([
                    ("k", container.kind.as_str().into()),
                    ("r", encode_r(&container.r)),
                    ("c", children.into()),
                ]);
                json::insert_present(&mut object, "a", container.active.map(Value::from));
                Value::Object(object)
            }
        }
    }

    fn decode_value(value: &Value, name: &'static str) -> Member<Tile> {
        let tile = value.as_object().ok_or(InvalidMember(name))?;
        let r = decode_r(tile)?;
        if tile.contains_key("w") {
            return Ok(Tile::Window {
                id: json::int(tile, "w")?,
                r,
            });
        }
        let kind =
            ContainerKind::from_wire(json::string(tile, "k")?).unwrap_or(ContainerKind::SplitH);
        let children = json::array(tile, "c")?
            .iter()
            .map(|child| Tile::decode_value(child, "c"))
            .collect::<Member<Vec<Tile>>>()?;
        if children.is_empty() {
            return Err(InvalidMember("c"));
        }
        let active = decode_active(tile, kind, children.len())?;
        Ok(Tile::Container(Container {
            kind,
            r,
            children,
            active,
        }))
    }

    fn retain_windows(self, is_known: &impl Fn(u64) -> bool) -> Option<Tile> {
        match self {
            Tile::Window { id, .. } => is_known(id).then_some(self),
            Tile::Container(container) => {
                let shown = container.active;
                let mut active = shown.map(|_| 0);
                let mut children = Vec::with_capacity(container.children.len());
                for (index, child) in container.children.into_iter().enumerate() {
                    if let Some(child) = child.retain_windows(is_known) {
                        if shown == Some(index) {
                            active = Some(children.len());
                        }
                        children.push(child);
                    }
                }
                (!children.is_empty()).then_some(Tile::Container(Container {
                    children,
                    active,
                    ..container
                }))
            }
        }
    }
}

/// `a`: an `int` when present. Absent on a tabbed or stacked container, or out of range: 0.
fn decode_active(tile: &JsonObject, kind: ContainerKind, len: usize) -> Member<Option<usize>> {
    const NAME: &str = "a";
    let active = match tile.get(NAME) {
        None => return Ok(kind.shows_one_child().then_some(0)),
        Some(value) => json::as_int(value).ok_or(InvalidMember(NAME))?,
    };
    Ok(Some(
        usize::try_from(active)
            .ok()
            .filter(|index| *index < len)
            .unwrap_or(0),
    ))
}

fn decode_r(tile: &JsonObject) -> Member<BasisRect> {
    let clamp = |value: u64| {
        u16::try_from(value).map_or(MAX_BASIS_POINTS, |value| value.min(MAX_BASIS_POINTS))
    };
    Ok(json::int_array::<4>(tile, "r")?.map(clamp))
}

fn encode_r(r: &BasisRect) -> Value {
    r.iter().map(|value| Value::from(*value)).collect()
}

/// The ids of `windows`, as the predicate of [`Layout::retain_windows`].
pub(crate) fn known_ids(ids: impl Iterator<Item = u64>) -> impl Fn(u64) -> bool {
    let ids: HashSet<u64> = ids.collect();
    move |id| ids.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(text: &str) -> Layout {
        let value: Value = serde_json::from_str(text).expect("test JSON");
        Layout::decode_value(&value, "layout").expect("valid layout")
    }

    #[test]
    fn skipping_a_window_keeps_the_shown_child() {
        let mut tabbed = layout(
            r#"{"size":[10,10],"fullscreen":null,"floating":[],"tiled":{"k":"t","r":[0,0,1,1],"a":2,
            "c":[{"w":1,"r":[0,0,1,1]},{"w":2,"r":[0,0,1,1]},{"w":3,"r":[0,0,1,1]}]}}"#,
        );
        tabbed.retain_windows(&known_ids([2, 3].into_iter()));
        let expected = layout(
            r#"{"size":[10,10],"fullscreen":null,"floating":[],"tiled":{"k":"t","r":[0,0,1,1],"a":1,
            "c":[{"w":2,"r":[0,0,1,1]},{"w":3,"r":[0,0,1,1]}]}}"#,
        );
        assert_eq!(tabbed, expected);
    }

    #[test]
    fn a_container_left_empty_is_skipped() {
        let mut nested = layout(
            r#"{"size":[10,10],"fullscreen":null,"floating":[{"k":"v","r":[0,0,1,1],"c":[{"w":9,"r":[0,0,1,1]}]}],
            "tiled":{"k":"h","r":[0,0,1,1],"c":[{"k":"s","r":[0,0,1,1],"c":[{"w":9,"r":[0,0,1,1]}]}]}}"#,
        );
        nested.retain_windows(&known_ids([1].into_iter()));
        assert_eq!(nested.tiled, None);
        assert!(nested.floating.is_empty());
    }

    #[test]
    fn negative_or_missing_values_are_refused() {
        let value: Value = serde_json::from_str(r#"{"w":1,"r":[0,0,-1,1]}"#).expect("test JSON");
        assert!(Tile::decode_value(&value, "tile").is_err());
        let value: Value =
            serde_json::from_str(r#"{"k":"t","r":[0,0,1,1],"a":-1,"c":[{"w":1,"r":[0,0,1,1]}]}"#)
                .expect("test JSON");
        assert!(Tile::decode_value(&value, "tile").is_err());
    }
}
