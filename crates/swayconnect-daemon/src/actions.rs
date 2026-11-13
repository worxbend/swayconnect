//! Typed action to validated sway command (`docs/DESIGN.md` 4.4). Written by slice S6d.
//!
//! The target of an action is checked against the published snapshot. The command is a
//! typed value of `swayconnect-sway`: this file handles no command text.

use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{Desktop, Workspace};
use swayconnect_sway::{Command, WorkspaceNumber};

use crate::sway_handle::{Action, ActionError};

/// Longest `message` of a `sway_error` (`docs/PROTOCOL.md` 10.1).
const MAX_SWAY_ERROR_SCALARS: usize = 200;
const NO_ERROR_TEXT: &str = "sway refused the command";
const NOT_A_COMMAND_RESULT: &str = "sway's reply is not a command result";

/// Steps 7 and 8 of `docs/PROTOCOL.md` 10.2: sway online, target in `desktop`, target
/// addressable.
pub(crate) fn command_for(action: Action, desktop: &Desktop) -> Result<Command, ActionError> {
    if !desktop.sway.online {
        return Err(ActionError::SwayUnavailable);
    }
    match action {
        Action::FocusWorkspaceNumber(number) => focus_workspace(u32::from(number)),
        Action::FocusWorkspaceId(id) => focus_workspace(unshared_number(id, &desktop.workspaces)?),
        Action::FocusWindow(id) => Ok(Command::FocusWindow {
            con_id: window_id(id, desktop)?,
        }),
        Action::Fullscreen {
            window_id: id,
            enabled: true,
        } => Ok(Command::EnableFullscreen {
            con_id: window_id(id, desktop)?,
        }),
        Action::Fullscreen {
            window_id: id,
            enabled: false,
        } => Ok(Command::DisableFullscreen {
            con_id: window_id(id, desktop)?,
        }),
    }
}

/// What a failed `RUN_COMMAND` means for the client. `Timeout` and `SwayUnavailable` also
/// say that the command connection is closed.
pub(crate) fn failure(error: &swayconnect_sway::Error) -> ActionError {
    use swayconnect_sway::Error;
    match error {
        Error::CommandFailed(text) if text.is_empty() => {
            ActionError::SwayError(NO_ERROR_TEXT.to_owned())
        }
        Error::CommandFailed(text) => ActionError::SwayError(sendable(text)),
        // The reply text is not passed on: it is not what sway documents.
        Error::Json(_) => ActionError::SwayError(NOT_A_COMMAND_RESULT.to_owned()),
        Error::Timeout => ActionError::Timeout,
        Error::Io(_)
        | Error::BadMagic
        | Error::FrameTooLarge(_)
        | Error::Closed
        | Error::UnexpectedReply { .. }
        | Error::UnsolicitedReply(_)
        | Error::SubscribeRefused => ActionError::SwayUnavailable,
    }
}

/// Sway's text as a client may get it: no control or format character, at most 200 scalars.
fn sendable(sway_text: &str) -> String {
    replace_unsafe(sway_text)
        .chars()
        .take(MAX_SWAY_ERROR_SCALARS)
        .collect()
}

fn focus_workspace(number: u32) -> Result<Command, ActionError> {
    u16::try_from(number)
        .ok()
        .and_then(WorkspaceNumber::new)
        .map(Command::FocusWorkspace)
        .ok_or(ActionError::UnsupportedTarget)
}

/// The number of the workspace with `id`, if no other workspace has it. A command names a
/// workspace by number only, so a shared number could reach the wrong one.
fn unshared_number(id: u64, workspaces: &[Workspace]) -> Result<u32, ActionError> {
    let target = workspaces
        .iter()
        .find(|workspace| workspace.id == id)
        .ok_or(ActionError::NotFound)?;
    let number = target.number.ok_or(ActionError::UnsupportedTarget)?;
    let holders = workspaces
        .iter()
        .filter(|workspace| workspace.number == Some(number));
    if holders.count() == 1 {
        Ok(number)
    } else {
        Err(ActionError::UnsupportedTarget)
    }
}

/// `id` if it is the id of a window. Workspaces share sway's id space, so a workspace id
/// must not pass: the command would hit the whole workspace.
fn window_id(id: u64, desktop: &Desktop) -> Result<u64, ActionError> {
    if desktop.windows.iter().any(|window| window.id == id) {
        Ok(id)
    } else {
        Err(ActionError::NotFound)
    }
}

#[cfg(test)]
mod tests {
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
}
