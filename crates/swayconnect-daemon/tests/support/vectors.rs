//! The files of `protocol/vectors/v1`, and a fake sway that shows the desktop of transcript T3
//! (`docs/PROTOCOL.md` 15.5).

use std::fs;

use serde_json::{Value, json};
use swayconnect_protocol::JsonObject;
use swayconnect_testkit::{FakeSway, Request};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1");

pub fn vector_file(name: &str) -> Value {
    let text = fs::read_to_string(format!("{VECTORS}/{name}")).expect("vector file");
    serde_json::from_str(&text).expect("vector JSON")
}

/// The frames of one transcript, each with `from` and `payload`.
pub fn transcript_frames(name: &str) -> Vec<Value> {
    let transcript = vector_file(&format!("transcripts/{name}.json"));
    transcript["frames"].as_array().expect("frames").clone()
}

/// The exact payload text of a transcript frame or of a vector case.
pub fn payload_text(frame: &Value) -> &str {
    frame["payload"].as_str().expect("payload")
}

pub fn payload_of(frame: &Value) -> JsonObject {
    serde_json::from_str(payload_text(frame)).expect("a JSON object")
}

fn sway_workspace(id: u64, number: u32, is_focused: bool, window: &Value) -> (Value, Value) {
    let name = number.to_string();
    let listed = json!({
        "id": id, "type": "workspace", "num": number, "name": name, "output": "DP-1",
        "focused": is_focused, "visible": is_focused, "urgent": false
    });
    let node = json!({
        "id": id, "type": "workspace", "num": number, "name": name, "output": "DP-1",
        "focused": false, "urgent": false, "fullscreen_mode": 1,
        "nodes": [window], "floating_nodes": []
    });
    (listed, node)
}

fn sway_window(id: u64, app_id: &str, title: &str, is_focused: bool) -> Value {
    json!({
        "id": id, "type": "con", "name": title, "app_id": app_id, "pid": 1000 + id,
        "focused": is_focused, "urgent": false, "fullscreen_mode": 0,
        "nodes": [], "floating_nodes": []
    })
}

/// Makes the fake sway show the desktop of transcript T3: workspaces 4 and 9, windows 5 and
/// 12. The focus is on the workspace with `focused_number`: 1 gives the `rev` 7 state, 2 the
/// `rev` 8 state. The tests compare the daemon's state with the vector afterwards.
pub fn show_the_t3_desktop(sway: &FakeSway, focused_number: u32) {
    let terminal = sway_window(5, "Alacritty", "zsh", focused_number == 1);
    let browser = sway_window(
        12,
        "firefox",
        "Example Domain \u{2014} Mozilla Firefox",
        focused_number == 2,
    );
    let (first_listed, first_node) = sway_workspace(4, 1, focused_number == 1, &terminal);
    let (second_listed, second_node) = sway_workspace(9, 2, focused_number == 2, &browser);
    let tree = json!({
        "id": 1, "type": "root", "name": "root", "floating_nodes": [],
        "nodes": [{
            "id": 2, "type": "output", "name": "DP-1", "floating_nodes": [],
            "nodes": [first_node, second_node]
        }]
    });
    let version = json!({
        "human_readable": "1.12", "variant": "sway", "major": 1, "minor": 12, "patch": 0,
        "loaded_config_file_name": "/nonexistent/sway/config"
    });
    let workspaces = json!([first_listed, second_listed]);
    sway.set_reply(Request::GetVersion, version.to_string());
    sway.set_reply(Request::GetWorkspaces, workspaces.to_string());
    sway.set_reply(Request::GetTree, tree.to_string());
}
