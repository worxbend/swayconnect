//! The only place that builds sway command text (`docs/DESIGN.md` 4.4 and 11.4).
//!
//! A command holds integers, booleans and members of closed enums defined here.
//! No string of any origin can enter it.

/// A workspace number sway is asked to focus or move a window to: 0 to 999.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceNumber(u16);

impl WorkspaceNumber {
    pub const MAX: u16 = 999;

    /// `None` when `number` is above [`WorkspaceNumber::MAX`].
    pub fn new(number: u16) -> Option<WorkspaceNumber> {
        (number <= Self::MAX).then_some(WorkspaceNumber(number))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

/// The layouts `[con_id=N] layout <kind>` may set on a window's parent container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    SplitH,
    SplitV,
    Tabbed,
    Stacking,
}

impl LayoutKind {
    fn sway_name(self) -> &'static str {
        match self {
            LayoutKind::SplitH => "splith",
            LayoutKind::SplitV => "splitv",
            LayoutKind::Tabbed => "tabbed",
            LayoutKind::Stacking => "stacking",
        }
    }
}

/// Every command this project can send to sway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    FocusWorkspace(WorkspaceNumber),
    FocusWindow {
        con_id: u64,
    },
    EnableFullscreen {
        con_id: u64,
    },
    DisableFullscreen {
        con_id: u64,
    },
    MoveToWorkspace {
        con_id: u64,
        number: WorkspaceNumber,
    },
    Floating {
        con_id: u64,
        enabled: bool,
    },
    Layout {
        con_id: u64,
        kind: LayoutKind,
    },
    MoveScratchpad {
        con_id: u64,
    },
    /// `con_id` is a window of the scratchpad.
    ScratchpadShow {
        con_id: u64,
    },
    Kill {
        con_id: u64,
    },
    /// The one fixed command with no variable: wakes every output (keep awake).
    OutputsPowerOn,
}

impl Command {
    /// One sway command, never a `;` or `,` list.
    pub(crate) fn render(self) -> String {
        match self {
            Command::FocusWorkspace(number) => {
                format!("workspace --no-auto-back-and-forth number {}", number.get())
            }
            Command::FocusWindow { con_id } => format!("[con_id={con_id}] focus"),
            Command::EnableFullscreen { con_id } => format!("[con_id={con_id}] fullscreen enable"),
            Command::DisableFullscreen { con_id } => {
                format!("[con_id={con_id}] fullscreen disable")
            }
            Command::MoveToWorkspace { con_id, number } => format!(
                "[con_id={con_id}] move container to workspace number {}",
                number.get()
            ),
            Command::Floating { con_id, enabled } => {
                let state = if enabled { "enable" } else { "disable" };
                format!("[con_id={con_id}] floating {state}")
            }
            Command::Layout { con_id, kind } => {
                format!("[con_id={con_id}] layout {}", kind.sway_name())
            }
            Command::MoveScratchpad { con_id } => format!("[con_id={con_id}] move scratchpad"),
            Command::ScratchpadShow { con_id } => format!("[con_id={con_id}] scratchpad show"),
            Command::Kill { con_id } => format!("[con_id={con_id}] kill"),
            Command::OutputsPowerOn => "output * power on".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests;
