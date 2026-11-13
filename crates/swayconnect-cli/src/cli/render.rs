//! Text and JSON forms of what the commands print. Pure: nothing here writes.

use serde_json::{Value, json};
use swayconnect_protocol::text::{escape_for_terminal, is_control_or_format};
use swayconnect_protocol::{Desktop, Scope, Welcome, Window, Workspace};

use super::state::DesktopState;

const ABSENT: &str = "-";
const COLUMN_GAP: &str = "  ";

pub fn workspaces_table(desktop: &Desktop) -> String {
    let rows = desktop.workspaces.iter().map(workspace_row).collect();
    table(&["ID", "NUM", "NAME", "OUTPUT", "FLAGS"], rows)
}

fn workspace_row(workspace: &Workspace) -> Vec<String> {
    let flags = [
        (workspace.focused, "focused"),
        (workspace.visible, "visible"),
        (workspace.urgent, "urgent"),
    ];
    vec![
        workspace.id.to_string(),
        workspace
            .number
            .map_or_else(|| ABSENT.to_owned(), |number| number.to_string()),
        workspace.name.clone(),
        or_absent(workspace.output.as_ref()),
        set_flags(&flags),
    ]
}

/// The title is the last column because it is the long one.
pub fn windows_table(desktop: &Desktop) -> String {
    let rows = desktop
        .windows
        .iter()
        .map(|window| window_row(window, desktop))
        .collect();
    table(&["ID", "WORKSPACE", "APP", "FLAGS", "TITLE"], rows)
}

fn window_row(window: &Window, desktop: &Desktop) -> Vec<String> {
    let flags = [
        (window.focused, "focused"),
        (window.urgent, "urgent"),
        (window.floating, "floating"),
        (window.fullscreen, "fullscreen"),
    ];
    let workspace_name = desktop
        .workspaces
        .iter()
        .find(|workspace| workspace.id == window.workspace_id)
        .map(|workspace| &workspace.name);
    vec![
        window.id.to_string(),
        or_absent(workspace_name),
        or_absent(window.app_id.as_ref()),
        set_flags(&flags),
        or_absent(window.title.as_ref()),
    ]
}

/// What a table view says after its rows when the rows are not the whole truth.
pub fn desktop_notes(desktop: &Desktop) -> Vec<&'static str> {
    let notes = [
        (!desktop.sway.online, "note: sway is offline"),
        (desktop.truncated, "note: the server cut the lists short"),
    ];
    notes
        .into_iter()
        .filter_map(|(applies, note)| applies.then_some(note))
        .collect()
}

pub fn watch_line(state: &DesktopState) -> String {
    let desktop = state.desktop();
    let focused_app = desktop
        .windows
        .iter()
        .find(|window| window.focused)
        .and_then(|window| window.app_id.as_ref());
    format!(
        "rev {}  sway {}  workspaces {}  windows {}  focused {}",
        state.rev(),
        if desktop.sway.online {
            "online"
        } else {
            "offline"
        },
        desktop.workspaces.len(),
        desktop.windows.len(),
        or_absent(focused_app),
    )
}

pub fn info_text(welcome: &Welcome) -> String {
    let limits = &welcome.limits;
    let lines = [
        format!(
            "server:     {} ({})",
            escape_for_terminal(&welcome.server.name),
            escape_for_terminal(&welcome.server.version)
        ),
        format!("protocol:   {}", welcome.protocol),
        format!("device:     {}", welcome.device_id.as_str()),
        format!("scopes:     {}", scope_names(&welcome.scopes).join(", ")),
        format!("ops:        {}", escaped(&welcome.ops).join(", ")),
        format!("topics:     {}", escaped(&welcome.topics).join(", ")),
        format!(
            "read-only:  {}",
            if welcome.read_only { "yes" } else { "no" }
        ),
        format!(
            "limits:     max_frame_c2s {}, max_frame_s2c {}, idle_timeout_ms {}",
            limits.max_frame_c2s, limits.max_frame_s2c, limits.idle_timeout_ms
        ),
    ];
    lines.join("\n")
}

/// The `welcome` members a user can act on, with their wire names. The proof is left out.
pub fn info_json(welcome: &Welcome) -> Value {
    json!({
        "protocol": welcome.protocol,
        "device_id": welcome.device_id.as_str(),
        "scopes": scope_names(&welcome.scopes),
        "ops": escaped(&welcome.ops),
        "topics": escaped(&welcome.topics),
        "read_only": welcome.read_only,
        "server": {
            "name": escape_for_terminal(&welcome.server.name),
            "version": escape_for_terminal(&welcome.server.version),
        },
        "limits": {
            "max_frame_c2s": welcome.limits.max_frame_c2s,
            "max_frame_s2c": welcome.limits.max_frame_s2c,
            "idle_timeout_ms": welcome.limits.idle_timeout_ms,
        },
    })
}

/// One line of JSON for a value whose text the server chose. Each control or format
/// character becomes a JSON `\u` escape: the line is safe for a terminal and decodes to the
/// same value. Such a character can only stand inside a JSON string.
pub fn server_json(value: &Value) -> String {
    let mut line = String::new();
    for character in value.to_string().chars() {
        if is_control_or_format(character) {
            for unit in character.encode_utf16(&mut [0; 2]) {
                line.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            line.push(character);
        }
    }
    line
}

fn scope_names(scopes: &[Scope]) -> Vec<&'static str> {
    scopes.iter().map(|scope| scope.as_str()).collect()
}

fn escaped(server_texts: &[String]) -> Vec<String> {
    server_texts
        .iter()
        .map(|text| escape_for_terminal(text))
        .collect()
}

fn or_absent(text: Option<&String>) -> String {
    text.map_or_else(|| ABSENT.to_owned(), Clone::clone)
}

fn set_flags(flags: &[(bool, &str)]) -> String {
    let names: Vec<&str> = flags
        .iter()
        .filter_map(|&(is_set, name)| is_set.then_some(name))
        .collect();
    if names.is_empty() {
        ABSENT.to_owned()
    } else {
        names.join(",")
    }
}

/// Left-aligned columns, padded by scalar count. The last column is not padded.
fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let header_row = headers.iter().map(|&header| header.to_owned()).collect();
    let all_rows: Vec<Vec<String>> = std::iter::once(header_row).chain(rows).collect();
    let widths: Vec<usize> = (0..headers.len())
        .map(|column| {
            let cells = all_rows.iter().filter_map(|row| row.get(column));
            cells.map(|cell| cell.chars().count()).max().unwrap_or(0)
        })
        .collect();
    let lines: Vec<String> = all_rows
        .iter()
        .map(|row| table_line(row, &widths))
        .collect();
    lines.join("\n")
}

fn table_line(row: &[String], widths: &[usize]) -> String {
    let last_column = row.len().saturating_sub(1);
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(column, (cell, &width))| {
            if column == last_column {
                cell.clone()
            } else {
                format!("{cell:<width$}")
            }
        })
        .collect();
    cells.join(COLUMN_GAP)
}

#[cfg(test)]
mod tests {
    use swayconnect_protocol::ops::TopicState;
    use swayconnect_protocol::{DeviceId, Limits, ServerInfo, SwayStatus};

    use super::*;

    fn workspace(id: u64, number: Option<u32>, name: &str) -> Workspace {
        Workspace {
            id,
            number,
            name: name.to_owned(),
            output: number.map(|_| "DP-1".to_owned()),
            focused: id == 4,
            visible: id == 4,
            urgent: false,
        }
    }

    fn window(id: u64, app_id: Option<&str>, title: Option<&str>) -> Window {
        Window {
            id,
            workspace_id: 4,
            app_id: app_id.map(str::to_owned),
            title: title.map(str::to_owned),
            focused: id == 9,
            urgent: false,
            floating: id == 10,
            fullscreen: id == 10,
        }
    }

    fn desktop() -> Desktop {
        Desktop {
            sway: SwayStatus {
                online: true,
                version: Some("1.11".to_owned()),
            },
            workspaces: vec![
                workspace(4, Some(1), "1:web"),
                workspace(12, None, "scratch"),
            ],
            windows: vec![
                window(9, Some("foot"), Some("~/src — vim")),
                window(10, None, None),
            ],
            truncated: false,
        }
    }

    fn welcome() -> Welcome {
        Welcome {
            protocol: 1,
            device_id: DeviceId::from_bytes([2; 16]),
            server_proof: [9; 32],
            confirm_code: None,
            scopes: vec![Scope::StateRead, Scope::StateTitles],
            ops: vec!["ping".to_owned(), "state.subscribe".to_owned()],
            topics: vec!["desktop".to_owned()],
            read_only: true,
            server: ServerInfo {
                name: "desk\u{1b}[31m".to_owned(),
                version: "0.1.0\u{202e}".to_owned(),
            },
            limits: Limits {
                max_frame_c2s: 16384,
                max_frame_s2c: 1_048_576,
                idle_timeout_ms: 90000,
            },
        }
    }

    #[test]
    fn workspaces_are_aligned_columns_with_a_dash_for_absent_values() {
        let expected = "\
ID  NUM  NAME     OUTPUT  FLAGS
4   1    1:web    DP-1    focused,visible
12  -    scratch  -       -";

        assert_eq!(workspaces_table(&desktop()), expected);
    }

    #[test]
    fn windows_show_the_workspace_name_and_end_with_the_title() {
        let expected = "\
ID  WORKSPACE  APP   FLAGS                TITLE
9   1:web      foot  focused              ~/src — vim
10  1:web      -     floating,fullscreen  -";

        assert_eq!(windows_table(&desktop()), expected);
    }

    #[test]
    fn a_window_of_an_unknown_workspace_and_an_empty_list_still_render() {
        let mut desktop = desktop();
        desktop.workspaces.clear();
        assert!(windows_table(&desktop).contains("9   -          foot"));

        desktop.windows.clear();
        assert_eq!(windows_table(&desktop), "ID  WORKSPACE  APP  FLAGS  TITLE");
    }

    #[test]
    fn padding_counts_scalars_and_not_bytes() {
        let mut desktop = desktop();
        desktop.workspaces[0].name = "ёж".to_owned();

        let table = workspaces_table(&desktop);

        assert!(table.contains("4   1    ёж       DP-1"), "{table}");
    }

    #[test]
    fn notes_tell_of_an_offline_sway_and_of_cut_lists() {
        let mut desktop = desktop();
        assert!(desktop_notes(&desktop).is_empty());

        desktop.sway.online = false;
        desktop.truncated = true;

        assert_eq!(
            desktop_notes(&desktop),
            [
                "note: sway is offline",
                "note: the server cut the lists short"
            ]
        );
    }

    #[test]
    fn a_watch_line_counts_the_lists_and_names_the_focused_app() {
        let topic_state = TopicState {
            topic: "desktop".to_owned(),
            rev: 31,
            data: desktop().encode(),
        };
        let state = DesktopState::from_topic_state(&topic_state)
            .expect("desktop topic")
            .expect("valid state");

        assert_eq!(
            watch_line(&state),
            "rev 31  sway online  workspaces 2  windows 2  focused foot"
        );
    }

    #[test]
    fn info_text_lists_the_welcome_data_with_escaped_server_text() {
        let expected = "\
server:     desk\\u{1b}[31m (0.1.0\\u{202e})
protocol:   1
device:     AgICAgICAgICAgICAgICAg
scopes:     state.read, state.titles
ops:        ping, state.subscribe
topics:     desktop
read-only:  yes
limits:     max_frame_c2s 16384, max_frame_s2c 1048576, idle_timeout_ms 90000";

        assert_eq!(info_text(&welcome()), expected);
    }

    #[test]
    fn info_json_uses_wire_names_and_holds_no_proof_and_no_unsafe_character() {
        let info = info_json(&welcome());

        assert_eq!(info["server"]["name"], "desk\\u{1b}[31m");
        assert_eq!(info["scopes"], json!(["state.read", "state.titles"]));
        assert_eq!(info["limits"]["idle_timeout_ms"], 90000);
        assert_eq!(info["read_only"], true);
        assert!(info.get("server_proof").is_none());
        let line = info.to_string();
        assert!(!line.chars().any(is_control_or_format));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn server_json_escapes_unsafe_text_and_keeps_the_value() {
        let hostile = "a\u{1b}[2Jb\u{7f}c\u{9b}d\u{202e}e\u{e0001}f \u{e9}\u{1f600}";
        let value = json!({ hostile: [hostile, 1, null], "plain": "text" });

        let line = server_json(&value);

        assert!(!line.contains(is_control_or_format), "{line:?}");
        assert!(line.contains("\\u009b") && line.contains("\\udb40\\udc01"));
        assert!(
            line.contains("\u{e9}\u{1f600}"),
            "printable text stays as it is"
        );
        let decoded: Value = serde_json::from_str(&line).expect("one JSON value");
        assert_eq!(decoded, value);
    }
}
