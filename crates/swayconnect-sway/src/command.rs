//! The only place that builds sway command text (`docs/DESIGN.md` 4.4).
//!
//! A command holds integers and one boolean. No string of any origin can enter it.

/// A workspace number sway is asked to focus: 0 to 999.
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

/// Every command this project can send to sway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    FocusWorkspace(WorkspaceNumber),
    FocusWindow { con_id: u64 },
    EnableFullscreen { con_id: u64 },
    DisableFullscreen { con_id: u64 },
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(number: u16) -> Command {
        Command::FocusWorkspace(WorkspaceNumber::new(number).expect("number is 0 to 999"))
    }

    #[test]
    fn each_variant_renders_the_exact_text() {
        let table = [
            (workspace(0), "workspace --no-auto-back-and-forth number 0"),
            (workspace(7), "workspace --no-auto-back-and-forth number 7"),
            (
                workspace(999),
                "workspace --no-auto-back-and-forth number 999",
            ),
            (Command::FocusWindow { con_id: 42 }, "[con_id=42] focus"),
            (
                Command::EnableFullscreen { con_id: 42 },
                "[con_id=42] fullscreen enable",
            ),
            (
                Command::DisableFullscreen { con_id: 42 },
                "[con_id=42] fullscreen disable",
            ),
            (
                Command::FocusWindow { con_id: u64::MAX },
                "[con_id=18446744073709551615] focus",
            ),
        ];
        for (command, text) in table {
            assert_eq!(command.render(), text, "{command:?}");
        }
    }

    #[test]
    fn workspace_number_stops_at_999() {
        assert_eq!(
            WorkspaceNumber::new(999).map(WorkspaceNumber::get),
            Some(999)
        );
        assert_eq!(WorkspaceNumber::new(1000), None);
        assert_eq!(WorkspaceNumber::new(u16::MAX), None);
    }

    fn is_digits(text: &str, max_len: usize) -> bool {
        !text.is_empty() && text.len() <= max_len && text.bytes().all(|b| b.is_ascii_digit())
    }

    /// `^(workspace --no-auto-back-and-forth number \d{1,3}|\[con_id=\d+\] (focus|fullscreen (enable|disable)))$`
    fn matches_pattern(text: &str) -> bool {
        if let Some(number) = text.strip_prefix("workspace --no-auto-back-and-forth number ") {
            return is_digits(number, 3);
        }
        let Some((con_id, verb)) = text
            .strip_prefix("[con_id=")
            .and_then(|rest| rest.split_once("] "))
        else {
            return false;
        };
        is_digits(con_id, usize::MAX)
            && matches!(verb, "focus" | "fullscreen enable" | "fullscreen disable")
    }

    #[test]
    fn the_matcher_refuses_text_outside_the_pattern() {
        for text in [
            "",
            "exit",
            "workspace number 1",
            "workspace --no-auto-back-and-forth number 1000",
            "workspace --no-auto-back-and-forth number -1",
            "workspace --no-auto-back-and-forth number 1; exit",
            "workspace --no-auto-back-and-forth number ",
            "[con_id=1] kill",
            "[con_id=] focus",
            "[con_id=1] focus, kill",
            "[con_id=1] fullscreen toggle",
            "[con_id=1 title=x] focus",
            " [con_id=1] focus",
            "[con_id=1] focus\n",
        ] {
            assert!(!matches_pattern(text), "{text:?}");
        }
    }

    #[test]
    fn every_rendered_command_matches_the_pattern() {
        let con_ids = [0, 1, 9, 10, 4_294_967_296, u64::MAX];
        let commands =
            (0..=WorkspaceNumber::MAX)
                .map(workspace)
                .chain(con_ids.into_iter().flat_map(|con_id| {
                    [
                        Command::FocusWindow { con_id },
                        Command::EnableFullscreen { con_id },
                        Command::DisableFullscreen { con_id },
                    ]
                }));
        for command in commands {
            let text = command.render();
            assert!(matches_pattern(&text), "{text:?}");
        }
    }
}
