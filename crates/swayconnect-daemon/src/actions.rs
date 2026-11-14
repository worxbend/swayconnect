//! Typed action to validated sway command (`docs/DESIGN.md` 4.4). Written by slice S6d.
//!
//! The target of an action is checked against the published snapshot. The command is a
//! typed value of `swayconnect-sway`: this file handles no command text.

use swayconnect_protocol::ops::{ContainerLayout, WorkspaceTarget};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{Desktop, Workspace};
use swayconnect_sway::{Command, LayoutKind, WorkspaceNumber};

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
        Action::MoveWindow { window_id: id, to } => {
            let con_id = window_id(id, desktop)?;
            let number = match to {
                WorkspaceTarget::Number(number) => u32::from(number),
                WorkspaceTarget::Id(id) => unshared_number(id, &desktop.workspaces)?,
            };
            let number = workspace_number(number)?;
            Ok(Command::MoveToWorkspace { con_id, number })
        }
        Action::Floating {
            window_id: id,
            enabled,
        } => Ok(Command::Floating {
            con_id: window_id(id, desktop)?,
            enabled,
        }),
        Action::Layout {
            window_id: id,
            layout,
        } => Ok(Command::Layout {
            con_id: window_id(id, desktop)?,
            kind: layout_kind(layout),
        }),
        Action::MoveToScratchpad(id) => Ok(Command::MoveScratchpad {
            con_id: window_id(id, desktop)?,
        }),
        Action::ScratchpadShow(id) => Ok(Command::ScratchpadShow {
            con_id: scratchpad_id(id, desktop)?,
        }),
        Action::Close(id) => Ok(Command::Kill {
            con_id: window_id(id, desktop)?,
        }),
        Action::OutputsPowerOn => Ok(Command::OutputsPowerOn),
    }
}

/// Steps 7 and 8 for the target window of an `input.*` request (`docs/PROTOCOL.md` 17.13):
/// sway online, and `id` a window of `desktop` (not a workspace, not a hidden scratchpad
/// window). Focus at delivery time is the input guard's check.
pub(crate) fn check_input_window(id: u64, desktop: &Desktop) -> Result<(), ActionError> {
    if !desktop.sway.online {
        return Err(ActionError::SwayUnavailable);
    }
    window_id(id, desktop).map(drop)
}

/// The protocol's closed layout enum, one to one onto the sway crate's.
fn layout_kind(layout: ContainerLayout) -> LayoutKind {
    match layout {
        ContainerLayout::SplitH => LayoutKind::SplitH,
        ContainerLayout::SplitV => LayoutKind::SplitV,
        ContainerLayout::Tabbed => LayoutKind::Tabbed,
        ContainerLayout::Stacking => LayoutKind::Stacking,
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
    workspace_number(number).map(Command::FocusWorkspace)
}

fn workspace_number(number: u32) -> Result<WorkspaceNumber, ActionError> {
    u16::try_from(number)
        .ok()
        .and_then(WorkspaceNumber::new)
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

/// `id` if it is the id of a window of the state's `scratchpad`. A shown window is not there.
fn scratchpad_id(id: u64, desktop: &Desktop) -> Result<u64, ActionError> {
    let mut hidden = desktop.scratchpad.iter().flatten();
    if hidden.any(|window| window.id == id) {
        Ok(id)
    } else {
        Err(ActionError::NotFound)
    }
}

#[cfg(test)]
mod tests;
