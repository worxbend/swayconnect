//! A daemon config for tests whose every program key names a fake program
//! (`docs/DESIGN.md` 11.9, `docs/V2_PLAN.md` TK.2), and the guard against system programs.

use crate::fake_proc::FakeProcs;
// A JSON string literal from `json_string` is also a TOML basic string.
use crate::fake_proc_spec::json_string;

/// A config section of `docs/DESIGN.md` 11.9 that names a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramSection {
    Session,
    KeepAwake,
    Notify,
    Audio,
    Apps,
    Launcher,
}

/// One program key: the fake program it names, and the fixed arguments that follow it
/// when the key is an argv list.
struct ProgramKey {
    section: ProgramSection,
    key: &'static str,
    program: &'static str,
    argv_tail: Option<&'static [&'static str]>,
}

const fn path_key(section: ProgramSection, key: &'static str, program: &'static str) -> ProgramKey {
    ProgramKey {
        section,
        key,
        program,
        argv_tail: None,
    }
}

const fn argv_key(
    section: ProgramSection,
    key: &'static str,
    program: &'static str,
    tail: &'static [&'static str],
) -> ProgramKey {
    ProgramKey {
        section,
        key,
        program,
        argv_tail: Some(tail),
    }
}

/// Every program key of `docs/DESIGN.md` 11.9, in section order.
const PROGRAM_KEYS: [ProgramKey; 8] = [
    argv_key(ProgramSection::Session, "lock", "lock.sh", &[]),
    path_key(ProgramSection::KeepAwake, "inhibit", "systemd-inhibit"),
    path_key(ProgramSection::KeepAwake, "sleep", "sleep"),
    path_key(ProgramSection::Notify, "notify_send", "notify-send"),
    path_key(ProgramSection::Audio, "pactl", "pactl"),
    path_key(ProgramSection::Apps, "systemd_run", "systemd-run"),
    argv_key(ProgramSection::Apps, "terminal", "alacritty", &["-e"]),
    argv_key(ProgramSection::Launcher, "argv", "fuzzel", &[]),
];

/// Every program name a program key uses; [`FakeProcs::install`] creates each.
pub(crate) fn program_names() -> impl Iterator<Item = &'static str> {
    PROGRAM_KEYS.iter().map(|program_key| program_key.program)
}

/// Path prefixes a test config must never name: real programs live there.
const SYSTEM_DIRS: [&str; 3] = ["/usr/", "/bin/", "/sbin/"];

impl ProgramSection {
    /// Every section. A daemon that does not know a section yet exits 2 on it, so a test
    /// against such a daemon names only the sections it knows.
    pub const ALL: [Self; 6] = [
        Self::Session,
        Self::KeepAwake,
        Self::Notify,
        Self::Audio,
        Self::Apps,
        Self::Launcher,
    ];

    fn table(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::KeepAwake => "keep_awake",
            Self::Notify => "notify",
            Self::Audio => "audio",
            Self::Apps => "apps",
            Self::Launcher => "launcher",
        }
    }

    fn program_keys(self) -> impl Iterator<Item = &'static ProgramKey> {
        PROGRAM_KEYS
            .iter()
            .filter(move |program_key| program_key.section == self)
    }
}

/// Builds the TOML text of a test config. [`build`](Self::build) runs
/// [`assert_no_system_programs`] on the whole text, extra lines included.
#[derive(Debug)]
pub struct TestConfig<'a> {
    programs: &'a FakeProcs,
    preamble: String,
    sections: Vec<(ProgramSection, Vec<String>)>,
}

impl<'a> TestConfig<'a> {
    /// Every section of [`ProgramSection::ALL`].
    pub fn new(programs: &'a FakeProcs) -> Self {
        Self::with_sections(programs, &ProgramSection::ALL)
    }

    /// Only `sections`, for a daemon that does not know the others yet.
    pub fn with_sections(programs: &'a FakeProcs, sections: &[ProgramSection]) -> Self {
        Self {
            programs,
            preamble: String::new(),
            sections: sections
                .iter()
                .map(|section| (*section, Vec::new()))
                .collect(),
        }
    }

    /// TOML placed before the program sections: root keys such as `read_only`, and any
    /// table the builder does not write.
    pub fn preamble(mut self, toml: &str) -> Self {
        self.preamble.push_str(toml);
        self.preamble.push('\n');
        self
    }

    /// One more `key = value` line inside `section`, for example `cooldown_s = 60`.
    /// Panics when `section` was not chosen.
    pub fn line(mut self, section: ProgramSection, toml_line: &str) -> Self {
        let (_, lines) = self
            .sections
            .iter_mut()
            .find(|(chosen, _)| *chosen == section)
            .unwrap_or_else(|| panic!("section {section:?} was not chosen"));
        lines.push(toml_line.to_owned());
        self
    }

    /// The config text. Panics when it names a system program.
    pub fn build(&self) -> String {
        let mut text = self.preamble.clone();
        for (section, extra_lines) in &self.sections {
            text.push_str(&format!("\n[{}]\n", section.table()));
            for program_key in section.program_keys() {
                text.push_str(&self.render(program_key));
            }
            for line in extra_lines {
                text.push_str(line);
                text.push('\n');
            }
        }
        assert_no_system_programs(&text);
        text
    }

    fn render(&self, program_key: &ProgramKey) -> String {
        let program_path = self.programs.program(program_key.program);
        let program = json_string(
            program_path
                .to_str()
                .unwrap_or_else(|| panic!("{} is not UTF-8", program_path.display())),
        );
        let value = match program_key.argv_tail {
            None => program,
            Some(tail) => {
                let tail_items = tail.iter().map(|argument| json_string(argument));
                let items: Vec<String> = std::iter::once(program).chain(tail_items).collect();
                format!("[{}]", items.join(", "))
            }
        };
        format!("{} = {value}\n", program_key.key)
    }
}

/// TOML that sets every program key with a `/usr/bin` default to a path that does not
/// exist, for a harness that runs no fake program. `[session] lock` has no default and
/// stays unset, so `session.lock` stays off.
pub fn nonexistent_programs() -> String {
    let mut text = String::new();
    for section in ProgramSection::ALL
        .into_iter()
        .filter(|section| *section != ProgramSection::Session)
    {
        text.push_str(&format!("\n[{}]\n", section.table()));
        for program_key in section.program_keys() {
            let program = json_string(&format!("/nonexistent/{}", program_key.program));
            let value = match program_key.argv_tail {
                None => program,
                Some(_) => format!("[{program}]"),
            };
            text.push_str(&format!("{} = {value}\n", program_key.key));
        }
    }
    text
}

/// Panics when `config` holds a quoted string that starts in `/usr`, `/bin` or `/sbin`.
///
/// Checks the text, so it works for any test config, built or written by hand. It cannot
/// see a program key left out: the daemon's default (a `/usr/bin` path) then applies, so
/// a test config names every program key of each section the daemon knows.
pub fn assert_no_system_programs(config: &str) {
    for quote in ['"', '\''] {
        for system_dir in SYSTEM_DIRS {
            let needle = format!("{quote}{system_dir}");
            assert!(
                !config.contains(&needle),
                "test config names a program under {system_dir}; use FakeProcs:\n{config}"
            );
        }
    }
}
