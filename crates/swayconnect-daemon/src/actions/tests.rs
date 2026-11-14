//! Tests of `actions.rs`: action to typed command, and sway's failures.

use serde_json::json;
use swayconnect_protocol::limits::MAX_WINDOWS;
use swayconnect_protocol::{SwayStatus, Window};
use swayconnect_sway::{Node, Version, WorkspaceInfo};

use super::*;
use crate::model;

const FIRST_WINDOW_ID: u64 = 1000;

fn workspace(id: u64, number: Option<u32>, name: &str) -> Workspace {
    Workspace {
        id,
        number,
        name: name.to_owned(),
        output: Some("TEST-1".to_owned()),
        focused: false,
        visible: false,
        urgent: false,
        rect: None,
        layout: None,
    }
}

fn window(id: u64, workspace_id: u64) -> Window {
    Window {
        id,
        workspace_id,
        app_id: None,
        title: None,
        focused: false,
        urgent: false,
        floating: false,
        fullscreen: false,
        icon: None,
    }
}

fn desktop(workspaces: Vec<Workspace>, windows: Vec<Window>) -> Desktop {
    Desktop {
        sway: SwayStatus {
            online: true,
            version: Some("1.12".to_owned()),
        },
        workspaces,
        windows,
        truncated: false,
        outputs: None,
        scratchpad: None,
        layouts_cut: None,
    }
}

/// Workspaces 4 and 9 with numbers 1 and 2, windows 5 and 12.
fn two_workspaces() -> Desktop {
    desktop(
        vec![workspace(4, Some(1), "1"), workspace(9, Some(2), "2")],
        vec![window(5, 4), window(12, 9)],
    )
}

fn focus_number(number: u16) -> Command {
    Command::FocusWorkspace(WorkspaceNumber::new(number).expect("0 to 999"))
}

fn every_action_on(id: u64) -> [Action; 4] {
    [
        Action::FocusWorkspaceId(id),
        Action::FocusWindow(id),
        Action::Fullscreen {
            window_id: id,
            enabled: true,
        },
        Action::Fullscreen {
            window_id: id,
            enabled: false,
        },
    ]
}

#[test]
fn each_action_becomes_its_typed_command() {
    let desktop = two_workspaces();
    let table = [
        (Action::FocusWorkspaceNumber(0), focus_number(0)),
        (Action::FocusWorkspaceNumber(999), focus_number(999)),
        // No workspace has number 3: sway creates it.
        (Action::FocusWorkspaceNumber(3), focus_number(3)),
        (Action::FocusWorkspaceId(9), focus_number(2)),
        (Action::FocusWindow(12), Command::FocusWindow { con_id: 12 }),
        (
            Action::Fullscreen {
                window_id: 5,
                enabled: true,
            },
            Command::EnableFullscreen { con_id: 5 },
        ),
        (
            Action::Fullscreen {
                window_id: 5,
                enabled: false,
            },
            Command::DisableFullscreen { con_id: 5 },
        ),
    ];
    for (action, command) in table {
        assert_eq!(command_for(action, &desktop), Ok(command), "{action:?}");
    }
}

/// [`two_workspaces`] with window 20 hidden in the scratchpad.
fn with_scratchpad() -> Desktop {
    let mut desktop = two_workspaces();
    desktop.scratchpad = Some(vec![swayconnect_protocol::ScratchpadWindow {
        id: 20,
        app_id: None,
        title: None,
        icon: None,
    }]);
    desktop
}

#[test]
fn each_window_and_session_action_becomes_its_typed_command() {
    let desktop = with_scratchpad();
    let number = |number| WorkspaceNumber::new(number).expect("0 to 999");
    let table = [
        (
            Action::MoveWindow {
                window_id: 5,
                to: WorkspaceTarget::Number(7),
            },
            Command::MoveToWorkspace {
                con_id: 5,
                number: number(7),
            },
        ),
        (
            Action::MoveWindow {
                window_id: 5,
                to: WorkspaceTarget::Id(9),
            },
            Command::MoveToWorkspace {
                con_id: 5,
                number: number(2),
            },
        ),
        (
            Action::Floating {
                window_id: 12,
                enabled: true,
            },
            Command::Floating {
                con_id: 12,
                enabled: true,
            },
        ),
        (
            Action::Layout {
                window_id: 12,
                layout: ContainerLayout::Stacking,
            },
            Command::Layout {
                con_id: 12,
                kind: LayoutKind::Stacking,
            },
        ),
        (
            Action::MoveToScratchpad(5),
            Command::MoveScratchpad { con_id: 5 },
        ),
        (
            Action::ScratchpadShow(20),
            Command::ScratchpadShow { con_id: 20 },
        ),
        (Action::Close(12), Command::Kill { con_id: 12 }),
        (Action::OutputsPowerOn, Command::OutputsPowerOn),
    ];
    for (action, command) in table {
        assert_eq!(command_for(action, &desktop), Ok(command), "{action:?}");
    }
}

#[test]
fn each_layout_maps_onto_the_same_sway_layout() {
    let pairs = [
        (ContainerLayout::SplitH, LayoutKind::SplitH),
        (ContainerLayout::SplitV, LayoutKind::SplitV),
        (ContainerLayout::Tabbed, LayoutKind::Tabbed),
        (ContainerLayout::Stacking, LayoutKind::Stacking),
    ];
    for (layout, kind) in pairs {
        assert_eq!(layout_kind(layout), kind);
    }
}

#[test]
fn a_window_op_on_a_scratchpad_window_or_a_scratchpad_show_of_a_window_is_not_found() {
    let desktop = with_scratchpad();
    let on_hidden = [
        Action::MoveToScratchpad(20),
        Action::Close(20),
        Action::Floating {
            window_id: 20,
            enabled: false,
        },
    ];
    let shown_or_workspace = [Action::ScratchpadShow(5), Action::ScratchpadShow(4)];
    for action in on_hidden.into_iter().chain(shown_or_workspace) {
        let outcome = command_for(action, &desktop);
        assert_eq!(outcome, Err(ActionError::NotFound), "{action:?}");
    }
    let no_list = command_for(Action::ScratchpadShow(20), &two_workspaces());
    assert_eq!(no_list, Err(ActionError::NotFound));
}

#[test]
fn a_move_checks_the_window_first_and_needs_an_unshared_workspace_number() {
    let mut desktop = two_workspaces();
    desktop.workspaces.push(workspace(30, None, "mail"));
    let to_id = |window_id, id| Action::MoveWindow {
        window_id,
        to: WorkspaceTarget::Id(id),
    };
    let cases = [
        (to_id(4, 9), ActionError::NotFound),
        (to_id(5, 12), ActionError::NotFound),
        (to_id(99, 30), ActionError::NotFound),
        (to_id(5, 30), ActionError::UnsupportedTarget),
    ];
    for (action, error) in cases {
        assert_eq!(command_for(action, &desktop), Err(error), "{action:?}");
    }
}

#[test]
fn power_on_needs_sway_but_no_target() {
    let offline = model::offline_desktop();
    assert_eq!(
        command_for(Action::OutputsPowerOn, &offline),
        Err(ActionError::SwayUnavailable)
    );
    let empty = desktop(Vec::new(), Vec::new());
    assert_eq!(
        command_for(Action::OutputsPowerOn, &empty),
        Ok(Command::OutputsPowerOn)
    );
}

#[test]
fn an_id_in_neither_list_is_not_found() {
    let desktop = two_workspaces();
    for id in [0, 1, 6, 13, u64::MAX] {
        for action in every_action_on(id) {
            let outcome = command_for(action, &desktop);
            assert_eq!(outcome, Err(ActionError::NotFound), "{action:?}");
        }
    }
}

#[test]
fn an_id_of_the_other_kind_is_not_found() {
    let desktop = two_workspaces();
    let [_, as_window @ ..] = every_action_on(4);
    let wrong_kind = as_window
        .into_iter()
        .chain([Action::FocusWorkspaceId(5), Action::FocusWorkspaceId(12)]);
    for action in wrong_kind {
        let outcome = command_for(action, &desktop);
        assert_eq!(outcome, Err(ActionError::NotFound), "{action:?}");
    }
}

#[test]
fn a_workspace_without_a_usable_number_is_an_unsupported_target() {
    let desktop = desktop(
        vec![
            workspace(4, None, "mail"),
            workspace(5, Some(1234), "1234"),
            workspace(6, Some(1000), "1000"),
            workspace(7, Some(1), "1:web"),
            workspace(8, Some(1), "1:mail"),
            workspace(9, Some(999), "999"),
        ],
        Vec::new(),
    );
    for id in [4, 5, 6, 7, 8] {
        let outcome = command_for(Action::FocusWorkspaceId(id), &desktop);
        assert_eq!(
            outcome,
            Err(ActionError::UnsupportedTarget),
            "workspace {id}"
        );
    }
    let highest = command_for(Action::FocusWorkspaceId(9), &desktop);
    assert_eq!(highest, Ok(focus_number(999)));
}

#[test]
fn a_number_above_999_is_refused_even_if_a_caller_lets_it_through() {
    for number in [1000, u16::MAX] {
        let outcome = command_for(Action::FocusWorkspaceNumber(number), &two_workspaces());
        assert_eq!(outcome, Err(ActionError::UnsupportedTarget), "{number}");
    }
}

#[test]
fn an_id_cut_by_truncation_is_not_found() {
    let window_ids = FIRST_WINDOW_ID..FIRST_WINDOW_ID + MAX_WINDOWS as u64 + 1;
    let window_nodes: Vec<_> = window_ids
        .clone()
        .map(|id| json!({"id": id, "type": "con", "pid": 1, "name": "w"}))
        .collect();
    let tree = json!({"id": 1, "type": "root", "nodes": [{
        "id": 2, "type": "output", "name": "TEST-1", "nodes": [{
            "id": 4, "type": "workspace", "num": 1, "name": "1", "nodes": window_nodes
        }]
    }]});
    let tree: Node = serde_json::from_value(tree).expect("tree");
    let listed = json!([{"id": 4, "num": 1, "name": "1"}]);
    let listed: Vec<WorkspaceInfo> = serde_json::from_value(listed).expect("list");
    let desktop = model::online_desktop(&tree, &listed, &Version::default());
    assert!(desktop.truncated);
    let last_kept = FIRST_WINDOW_ID + MAX_WINDOWS as u64 - 1;

    let kept = command_for(Action::FocusWindow(last_kept), &desktop);
    let cut = command_for(Action::FocusWindow(window_ids.end - 1), &desktop);

    assert_eq!(kept, Ok(Command::FocusWindow { con_id: last_kept }));
    assert_eq!(cut, Err(ActionError::NotFound));
}

#[test]
fn while_sway_is_offline_every_action_is_unavailable() {
    let offline = model::offline_desktop();
    let mut stale = two_workspaces();
    stale.sway.online = false;
    let actions = [Action::FocusWorkspaceNumber(1), Action::FocusWindow(5)];
    for action in actions.into_iter().chain(every_action_on(4)) {
        for desktop in [&offline, &stale] {
            let outcome = command_for(action, desktop);
            assert_eq!(outcome, Err(ActionError::SwayUnavailable), "{action:?}");
        }
    }
}

#[test]
fn the_error_text_of_sway_is_cleaned_and_cut_to_200_characters() {
    let hostile = format!("bad\u{1b}[2J\u{202e}{}", "é".repeat(300));
    let refused = swayconnect_sway::Error::CommandFailed(hostile);

    let ActionError::SwayError(text) = failure(&refused) else {
        panic!("a refused command is a sway error");
    };

    assert_eq!(text.chars().count(), 200);
    assert!(text.starts_with("bad\u{fffd}[2J\u{fffd}éé"), "{text:?}");
}

#[test]
fn a_reply_without_text_or_without_the_expected_shape_is_a_sway_error() {
    let no_text = swayconnect_sway::Error::CommandFailed(String::new());
    assert_eq!(
        failure(&no_text),
        ActionError::SwayError(NO_ERROR_TEXT.to_owned())
    );
    let quoted = serde_json::from_str::<u64>(r#""a window title""#).expect_err("not a u64");
    assert_eq!(
        failure(&quoted.into()),
        ActionError::SwayError(NOT_A_COMMAND_RESULT.to_owned())
    );
}

#[test]
fn a_lost_connection_is_unavailable_and_no_answer_is_a_timeout() {
    use swayconnect_sway::Error;
    assert_eq!(failure(&Error::Timeout), ActionError::Timeout);
    let broken_pipe = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
    for lost in [Error::Closed, Error::Io(broken_pipe), Error::BadMagic] {
        assert_eq!(failure(&lost), ActionError::SwayUnavailable, "{lost}");
    }
}
