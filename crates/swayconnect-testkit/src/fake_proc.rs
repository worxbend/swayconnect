//! Fake programs for every child-process key of a test config (`docs/DESIGN.md` 11.9, 11.12).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::fake_proc_spec::{LOG_SUFFIX, MODE_SUFFIX, Mode, STDOUT_SUFFIX, sibling};
use crate::test_config::program_names;

/// Where the build script puts `fake-proc`: the profile directory, which holds `deps/`
/// (test binaries) and `examples/`. Must match `INSTALLED_NAME` in `build.rs`.
const INSTALLED_NAME: &str = "swayconnect-fake-proc";

/// One directory holding `fake-proc` under each program name of `docs/DESIGN.md` 11.9.
///
/// Each name is a symlink to the one built binary, not a copy: a test never writes an
/// executable, so a parallel test's `fork` cannot make `exec` fail with `ETXTBSY`. The
/// fake finds its files through `argv[0]`, the path a test config names, and `/proc`
/// shows the program name as `comm`.
#[derive(Debug)]
pub struct FakeProcs {
    dir: PathBuf,
}

impl FakeProcs {
    /// Creates `dir` and every fake program in it. Panics when the file system refuses
    /// or a program file already exists there.
    pub fn install(dir: &Path) -> Self {
        let fake_proc = built_fake_proc();
        fs::create_dir_all(dir)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
        for name in program_names() {
            let program = dir.join(name);
            std::os::unix::fs::symlink(&fake_proc, &program)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", program.display()));
        }
        Self {
            dir: dir.to_owned(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The absolute path a config names for `name`. Panics for a name that was not installed.
    pub fn program(&self, name: &str) -> PathBuf {
        assert!(
            program_names().any(|known| known == name),
            "{name} is not a fake program"
        );
        self.dir.join(name)
    }

    /// What every later call of `name` does after logging.
    pub fn set_mode(&self, name: &str, mode: Mode) {
        write(
            &sibling(&self.program(name), MODE_SUFFIX),
            mode.to_string().as_bytes(),
        );
    }

    /// Bytes every later call of `name` writes to stdout.
    pub fn set_stdout(&self, name: &str, bytes: &[u8]) {
        write(&sibling(&self.program(name), STDOUT_SUFFIX), bytes);
    }

    /// The arguments (after `argv[0]`) of every call of `name` so far, oldest first.
    pub fn calls(&self, name: &str) -> Vec<Vec<String>> {
        let log_path = sibling(&self.program(name), LOG_SUFFIX);
        let text = match fs::read_to_string(&log_path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
            Err(error) => panic!("cannot read {}: {error}", log_path.display()),
        };
        text.lines()
            .map(|line| {
                serde_json::from_str(line).unwrap_or_else(|error| {
                    panic!("bad line in {}: {error}: {line}", log_path.display())
                })
            })
            .collect()
    }
}

/// `<profile dir>/swayconnect-fake-proc`, found from the running test binary
/// (`<profile dir>/deps/<test>` or `<profile dir>/examples/<example>`).
fn built_fake_proc() -> PathBuf {
    let running = std::env::current_exe().expect("path of the running test binary");
    let profile_dir = running
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("no profile directory above {}", running.display()));
    let fake_proc = profile_dir.join(INSTALLED_NAME);
    assert!(
        fake_proc.is_file(),
        "{} is missing; the swayconnect-testkit build script puts it there",
        fake_proc.display()
    );
    fake_proc
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn every_mode_reads_back_from_its_text() {
        for mode in [
            Mode::Exit(0),
            Mode::Exit(255),
            Mode::Sleep(Duration::from_millis(1500)),
            Mode::Hang,
        ] {
            assert_eq!(mode.to_string().parse::<Mode>(), Ok(mode));
        }
    }

    #[test]
    fn a_malformed_mode_is_an_error() {
        for text in [
            "",
            "exit",
            "exit 256",
            "exit -1",
            "sleep_ms x",
            "hang 1",
            "exit 0 1",
        ] {
            assert!(text.parse::<Mode>().is_err(), "{text:?}");
        }
    }
}
