use super::*;

fn number(number: u16) -> WorkspaceNumber {
    WorkspaceNumber::new(number).expect("number is 0 to 999")
}

fn workspace(value: u16) -> Command {
    Command::FocusWorkspace(number(value))
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
        (
            Command::MoveToWorkspace {
                con_id: 42,
                number: number(3),
            },
            "[con_id=42] move container to workspace number 3",
        ),
        (
            Command::MoveToWorkspace {
                con_id: 42,
                number: number(999),
            },
            "[con_id=42] move container to workspace number 999",
        ),
        (
            Command::Floating {
                con_id: 42,
                enabled: true,
            },
            "[con_id=42] floating enable",
        ),
        (
            Command::Floating {
                con_id: 42,
                enabled: false,
            },
            "[con_id=42] floating disable",
        ),
        (
            Command::Layout {
                con_id: 42,
                kind: LayoutKind::SplitH,
            },
            "[con_id=42] layout splith",
        ),
        (
            Command::Layout {
                con_id: 42,
                kind: LayoutKind::SplitV,
            },
            "[con_id=42] layout splitv",
        ),
        (
            Command::Layout {
                con_id: 42,
                kind: LayoutKind::Tabbed,
            },
            "[con_id=42] layout tabbed",
        ),
        (
            Command::Layout {
                con_id: 42,
                kind: LayoutKind::Stacking,
            },
            "[con_id=42] layout stacking",
        ),
        (
            Command::MoveScratchpad { con_id: 42 },
            "[con_id=42] move scratchpad",
        ),
        (
            Command::ScratchpadShow { con_id: 42 },
            "[con_id=42] scratchpad show",
        ),
        (Command::Kill { con_id: 42 }, "[con_id=42] kill"),
        (Command::OutputsPowerOn, "output * power on"),
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

/// The `[con_id=N] ...` alternatives of the pattern, after `"] "`.
fn is_window_verb(verb: &str) -> bool {
    if let Some(number) = verb.strip_prefix("move container to workspace number ") {
        return is_digits(number, 3);
    }
    matches!(
        verb,
        "focus"
            | "fullscreen enable"
            | "fullscreen disable"
            | "floating enable"
            | "floating disable"
            | "layout splith"
            | "layout splitv"
            | "layout tabbed"
            | "layout stacking"
            | "move scratchpad"
            | "scratchpad show"
            | "kill"
    )
}

/// The pattern of `docs/DESIGN.md` 11.4:
///
/// ```text
/// ^(workspace --no-auto-back-and-forth number \d{1,3}
///  |\[con_id=\d+\] (focus|fullscreen (enable|disable)|floating (enable|disable)
///    |move container to workspace number \d{1,3}|layout (splith|splitv|tabbed|stacking)
///    |move scratchpad|scratchpad show|kill)
///  |output \* power on)$
/// ```
fn matches_pattern(text: &str) -> bool {
    if text == "output * power on" {
        return true;
    }
    if let Some(number) = text.strip_prefix("workspace --no-auto-back-and-forth number ") {
        return is_digits(number, 3);
    }
    let Some((con_id, verb)) = text
        .strip_prefix("[con_id=")
        .and_then(|rest| rest.split_once("] "))
    else {
        return false;
    };
    is_digits(con_id, usize::MAX) && is_window_verb(verb)
}

#[test]
fn the_matcher_accepts_each_alternative_of_the_pattern() {
    for text in [
        "workspace --no-auto-back-and-forth number 1",
        "[con_id=1] focus",
        "[con_id=1] fullscreen disable",
        "[con_id=1] floating enable",
        "[con_id=1] move container to workspace number 12",
        "[con_id=1] layout stacking",
        "[con_id=1] move scratchpad",
        "[con_id=1] scratchpad show",
        "[con_id=1] kill",
        "output * power on",
    ] {
        assert!(matches_pattern(text), "{text:?}");
    }
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
        "[con_id=1] kill; exit",
        "[con_id=1] kill, exit",
        "[con_id=] focus",
        "[con_id=1] focus, kill",
        "[con_id=1] fullscreen toggle",
        "[con_id=1 title=x] focus",
        " [con_id=1] focus",
        "[con_id=1] focus\n",
        "[con_id=1] floating toggle",
        "[con_id=1] layout toggle split",
        "[con_id=1] layout default",
        "[con_id=1] layout splith; exit",
        "[con_id=1] move container to workspace number 1000",
        "[con_id=1] move container to workspace number ",
        "[con_id=1] move container to workspace x",
        "[con_id=1] move container to workspace number 1; exit",
        "[con_id=1] move scratchpad, kill",
        "[con_id=1] scratchpad show; exit",
        "[con_id=1] output * power on",
        "scratchpad show",
        "move scratchpad",
        "kill",
        "output * power off",
        "output * power toggle",
        "output HDMI-A-1 power on",
        "output * power on; exit",
        "output * power on\n",
        " output * power on",
    ] {
        assert!(!matches_pattern(text), "{text:?}");
    }
}

/// One index per variant. The exhaustive match makes a new variant fail to
/// compile here until [`samples`] and [`VARIANTS`] cover it.
fn variant_index(command: Command) -> usize {
    match command {
        Command::FocusWorkspace(_) => 0,
        Command::FocusWindow { .. } => 1,
        Command::EnableFullscreen { .. } => 2,
        Command::DisableFullscreen { .. } => 3,
        Command::MoveToWorkspace { .. } => 4,
        Command::Floating { .. } => 5,
        Command::Layout { .. } => 6,
        Command::MoveScratchpad { .. } => 7,
        Command::ScratchpadShow { .. } => 8,
        Command::Kill { .. } => 9,
        Command::OutputsPowerOn => 10,
    }
}

const VARIANTS: usize = 11;

const LAYOUTS: [LayoutKind; 4] = [
    LayoutKind::SplitH,
    LayoutKind::SplitV,
    LayoutKind::Tabbed,
    LayoutKind::Stacking,
];

/// Every variant with every value of its enum and boolean members.
fn samples(con_id: u64, target: WorkspaceNumber) -> Vec<Command> {
    let window = [
        Command::FocusWindow { con_id },
        Command::EnableFullscreen { con_id },
        Command::DisableFullscreen { con_id },
        Command::MoveToWorkspace {
            con_id,
            number: target,
        },
        Command::Floating {
            con_id,
            enabled: true,
        },
        Command::Floating {
            con_id,
            enabled: false,
        },
        Command::MoveScratchpad { con_id },
        Command::ScratchpadShow { con_id },
        Command::Kill { con_id },
    ];
    let layouts = LAYOUTS
        .into_iter()
        .map(|kind| Command::Layout { con_id, kind });
    let fixed = [Command::FocusWorkspace(target), Command::OutputsPowerOn];
    window.into_iter().chain(layouts).chain(fixed).collect()
}

#[test]
fn the_samples_cover_every_variant() {
    let mut seen = [false; VARIANTS];
    for command in samples(1, number(1)) {
        seen[variant_index(command)] = true;
    }
    assert_eq!(seen, [true; VARIANTS]);
}

#[test]
fn every_rendered_command_matches_the_pattern() {
    let con_ids = [0, 1, 9, 10, 4_294_967_296, u64::MAX];
    let numbers = (0..=WorkspaceNumber::MAX).map(number);
    for target in numbers {
        for con_id in con_ids {
            for command in samples(con_id, target) {
                let text = command.render();
                assert!(matches_pattern(&text), "{text:?}");
            }
        }
    }
}

#[test]
fn no_rendered_command_is_a_command_list() {
    for command in samples(u64::MAX, number(999)) {
        let text = command.render();
        assert!(
            !text.contains([';', ',', '\n', '"', '\'']),
            "{text:?} could chain or quote"
        );
    }
}
