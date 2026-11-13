//! A tree with geometry for the v2 desktop additions (`docs/PROTOCOL.md` 17.4): two
//! outputs, rects on every node, nested splits, a tabbed and a stacked container, a
//! floating window, a fullscreen window and a scratchpad window. Shaped like a read-only
//! `swaymsg -t get_tree` of sway 1.12, with invented names and titles.
//!
//! Ids: root 1, outputs 2 (`TEST-1`), 19 (`TEST-2`), 3 (`__i3`); workspaces 4 (`1`),
//! 5 (`2: web`), 14 (`3`), 20 (`__i3_scratch`); containers 10 (splitv), 11 (tabbed),
//! 17 (stacked); windows 6, 7, 8, 9, 12, 13, 15, 16, 18.
//!
//! | Window | Workspace | Place |
//! |---|---|---|
//! | 6 `notes` | 4 | left half of the splith root |
//! | 8 `terminal` | 4 | top of splitv 10 |
//! | 9 `docs`, 12 `mail` | 4 | tabbed 11 under splitv 10; 12 is shown and focused |
//! | 13 `calc` | 4 | floating |
//! | 7 X11 `Browser` | 5 | fullscreen |
//! | 15 `notes`, 18 `docs` | 14 | stacked 17, the only child of the splitv root; 15 is shown |
//! | 16 `music` | 20 | scratchpad, hidden |

use serde_json::{Value, json};

/// `TEST-1`: 2560 x 1440 at scale 1. The bar takes the top 30 px of its workspaces.
const BAR: i64 = 30;
const OUT1: Rect = Rect::new(0, 0, 2560, 1440);
const WS1: Rect = Rect::new(0, BAR, 2560, 1440 - BAR);
/// `TEST-2`: a 1080p panel at scale 1.25, turned 90 degrees: 864 x 1536 logical px.
const OUT2: Rect = Rect::new(2560, 0, 864, 1536);
const WS3: Rect = Rect::new(2560, BAR, 864, 1536 - BAR);
/// Width of window 6 in [`layout_tree`]; [`layout_tree_resized`] makes it narrower.
const NOTES_WIDTH: i64 = 1280;
const NOTES_WIDTH_RESIZED: i64 = 1024;

#[derive(Clone, Copy)]
struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

impl Rect {
    const fn new(x: i64, y: i64, w: i64, h: i64) -> Self {
        Self { x, y, w, h }
    }

    fn json(self) -> Value {
        json!({ "x": self.x, "y": self.y, "width": self.w, "height": self.h })
    }
}

/// `GET_TREE` reply with geometry. Window 6 takes the left 1280 px of workspace 4.
pub fn layout_tree() -> String {
    build_tree(NOTES_WIDTH)
}

/// [`layout_tree`] after the owner resized window 6 to 1024 px; the splitv container 10
/// beside it grows. Nothing else changes, so a test can swap the two replies and expect
/// exactly one new state.
pub fn layout_tree_resized() -> String {
    build_tree(NOTES_WIDTH_RESIZED)
}

/// `GET_WORKSPACES` reply that matches [`layout_tree`]. Workspace 4 is focused.
pub fn layout_workspaces() -> String {
    json!([
        workspace_info(4, 1, "1", "TEST-1", WS1, true, true),
        workspace_info(5, 2, "2: web", "TEST-1", WS1, false, false),
        workspace_info(14, 3, "3", "TEST-2", WS3, false, true),
    ])
    .to_string()
}

/// A `binding` event payload, as sway sends it after a `resize` key binding ran.
pub fn binding_event() -> String {
    json!({
        "change": "run",
        "binding": {
            "command": "resize shrink width 10px",
            "event_state_mask": ["Mod4"],
            "input_code": 0,
            "mods": ["Mod4"],
            "symbol": "h",
            "symbols": ["h"],
            "input_type": "keyboard"
        }
    })
    .to_string()
}

fn workspace_info(
    id: u64,
    num: i64,
    name: &str,
    output: &str,
    rect: Rect,
    focused: bool,
    visible: bool,
) -> Value {
    json!({
        "id": id, "type": "workspace", "num": num, "name": name, "output": output,
        "focused": focused, "visible": visible, "urgent": false, "rect": rect.json(),
        "layout": "splith", "representation": null
    })
}

fn build_tree(notes_width: i64) -> String {
    json!({
        "id": 1, "type": "root", "name": "root", "layout": "splith",
        "rect": Rect::new(0, 0, 3424, 1536).json(), "focus": [2, 19, 3],
        "nodes": [scratch_output(), output_1(notes_width), output_2()],
        "floating_nodes": []
    })
    .to_string()
}

fn scratch_output() -> Value {
    let mut music = window(16, "music", "Radio stream", Rect::new(1080, 345, 400, 300));
    music["type"] = json!("floating_con");
    music["scratchpad_state"] = json!("fresh");
    let music = hidden(music);
    let mut scratch = workspace(20, -1, "__i3_scratch", "__i3", WS1, vec![], vec![16]);
    scratch["floating_nodes"] = json!([music]);
    let mut output = output(
        3,
        "__i3",
        Rect::new(0, 0, 1920, 1080),
        vec![scratch],
        vec![20],
    );
    output["active"] = json!(false);
    output
}

fn output_1(notes_width: i64) -> Value {
    let right = Rect::new(notes_width, BAR, WS1.w - notes_width, WS1.h);
    let half = right.h / 2;
    let top = Rect::new(right.x, right.y, right.w, half);
    let bottom = Rect::new(right.x, right.y + half, right.w, right.h - half);

    let tabbed = container(
        11,
        "tabbed",
        bottom,
        vec![
            hidden(window(9, "docs", "Meeting minutes", bottom)),
            focused(window(12, "mail", "Inbox (3)", bottom)),
        ],
        vec![12, 9],
    );
    let splitv = container(
        10,
        "splitv",
        right,
        vec![window(8, "terminal", "~/projects", top), tabbed],
        vec![11, 8],
    );
    let notes = window(
        6,
        "notes",
        "Packing list for the lake trip",
        Rect::new(0, BAR, notes_width, WS1.h),
    );
    let mut calc = window(13, "calc", "Calculator", Rect::new(980, 420, 600, 600));
    calc["type"] = json!("floating_con");
    let mut ws1 = workspace(
        4,
        1,
        "1",
        "TEST-1",
        WS1,
        vec![notes, splitv],
        vec![10, 6, 13],
    );
    ws1["floating_nodes"] = json!([calc]);

    let mut browser = x11_window(7, "Browser", "Sourdough for beginners", OUT1);
    browser["fullscreen_mode"] = json!(1);
    let browser = hidden(browser);
    let ws2 = workspace(5, 2, "2: web", "TEST-1", WS1, vec![browser], vec![7]);

    output(2, "TEST-1", OUT1, vec![ws1, ws2], vec![4, 5])
}

fn output_2() -> Value {
    let stacked = container(
        17,
        "stacked",
        WS3,
        vec![
            window(15, "notes", "Shopping list", WS3),
            hidden(window(18, "docs", "Recipe draft", WS3)),
        ],
        vec![15, 18],
    );
    let ws3 = workspace(14, 3, "3", "TEST-2", WS3, vec![stacked], vec![17]);
    let mut output = output(19, "TEST-2", OUT2, vec![ws3], vec![14]);
    output["scale"] = json!(1.25);
    output["transform"] = json!("90");
    output["model"] = json!("Side Panel 24");
    output
}

fn output(id: u64, name: &str, rect: Rect, workspaces: Vec<Value>, focus: Vec<u64>) -> Value {
    let current = workspaces
        .first()
        .map_or(Value::Null, |workspace| workspace["name"].clone());
    json!({
        "id": id, "type": "output", "name": name, "rect": rect.json(), "layout": "output",
        "active": true, "power": true, "dpms": true, "scale": 1.0, "transform": "normal",
        "make": "Example Displays", "model": "Desk Panel 27", "serial": "0000",
        "current_workspace": current, "focused": false, "focus": focus,
        "nodes": workspaces, "floating_nodes": []
    })
}

fn workspace(
    id: u64,
    num: i64,
    name: &str,
    output: &str,
    rect: Rect,
    nodes: Vec<Value>,
    focus: Vec<u64>,
) -> Value {
    json!({
        "id": id, "type": "workspace", "num": num, "name": name, "output": output,
        "rect": rect.json(), "layout": "splith", "focused": false, "urgent": false,
        "fullscreen_mode": 1, "focus": focus, "nodes": nodes, "floating_nodes": []
    })
}

fn container(id: u64, layout: &str, rect: Rect, nodes: Vec<Value>, focus: Vec<u64>) -> Value {
    json!({
        "id": id, "type": "con", "name": null, "layout": layout, "rect": rect.json(),
        "focused": false, "urgent": false, "fullscreen_mode": 0, "focus": focus,
        "nodes": nodes, "floating_nodes": []
    })
}

fn window(id: u64, app_id: &str, title: &str, rect: Rect) -> Value {
    json!({
        "id": id, "type": "con", "name": title, "app_id": app_id, "pid": 2000 + id,
        "layout": "none", "rect": rect.json(), "focused": false, "urgent": false,
        "visible": true, "fullscreen_mode": 0, "scratchpad_state": "none",
        "inhibit_idle": false, "focus": [], "nodes": [], "floating_nodes": []
    })
}

fn x11_window(id: u64, class: &str, title: &str, rect: Rect) -> Value {
    let mut node = window(id, class, title, rect);
    node["app_id"] = Value::Null;
    node["window"] = json!(4_194_304 + id);
    node["window_properties"] = json!({ "class": class });
    node
}

fn focused(mut node: Value) -> Value {
    node["focused"] = json!(true);
    node
}

/// A window on a workspace that is not shown, behind a tab, or in the scratchpad.
fn hidden(mut node: Value) -> Value {
    node["visible"] = json!(false);
    node
}
