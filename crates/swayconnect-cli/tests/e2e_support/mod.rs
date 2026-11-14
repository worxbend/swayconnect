//! `swayconnectd` as a child process with the fake sway, and `swayconnect` runs against it
//! (`docs/DESIGN.md` 9.2, 9.3).
//!
//! Every child has a cleared environment and temp XDG directories. The daemon listens on
//! `127.0.0.1:0`, its `sway.socket` is the fake sway, and every program key of its config
//! names a `fake-proc` program (`docs/DESIGN.md` 11.9), so no test starts a real one.

#![allow(
    dead_code,
    reason = "every test binary compiles this module and uses a part of it"
)]

use std::fs;
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::Value;
use swayconnect_testkit::{FakeProcs, FakeSway, ProgramSection, TestConfig, TestDirs};

pub const CLIENT: &str = env!("CARGO_BIN_EXE_swayconnect");
pub const DAEMON_NAME: &str = "swayconnectd";
pub const PATIENCE: Duration = Duration::from_secs(20);
const RETRY_DELAY: Duration = Duration::from_millis(100);
pub const EXIT_SERVER_ERROR: i32 = 1;
pub const EXIT_USAGE_OR_PROFILE: i32 = 2;
pub const EXIT_CONNECTION: i32 = 3;
pub const EXIT_REJECTED: i32 = 4;
/// The program sections this daemon knows, every one since M2b: its audio service starts
/// `pactl` at once, so that must be `fake-proc` too.
pub const DAEMON_SECTIONS: [ProgramSection; 6] = ProgramSection::ALL;
/// A process name no process has, so the daemon finds the screen unlocked whatever the
/// real desktop shows: its lock heuristic reads the real `/proc`.
const NO_LOCK_PROCESS: &str = "sc-test-nolock";
/// The same for keyboard grabbers: a real launcher open on the desktop does not refuse
/// the input of a test.
const NO_GRABBER_PROCESS: &str = "sc-test-nograb";
pub const FOCUS_WINDOW_6_REQUEST: &str =
    r#"{"type":"req","id":1,"op":"window.focus","args":{"window_id":6}}"#;

/// The daemon binary is built by its own package into the directory of this package's
/// binary. `just test` builds it first; cargo alone does that only for a workspace run.
pub fn daemon_binary() -> PathBuf {
    let daemon = Path::new(CLIENT).with_file_name(DAEMON_NAME);
    assert!(
        daemon.is_file(),
        "{} is missing: build package swayconnect-daemon first (`just test` does)",
        daemon.display()
    );
    daemon
}

/// The lines a child writes to one of its pipes, read by a thread.
pub struct Lines {
    pub incoming: Receiver<String>,
    pub seen: Vec<String>,
}

impl Lines {
    pub fn of(pipe: impl Read + Send + 'static) -> Lines {
        let (sender, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Lines {
            incoming,
            seen: Vec::new(),
        }
    }

    /// The next line that contains `text`.
    pub fn expect(&mut self, text: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let patience = deadline.saturating_duration_since(Instant::now());
            match self.incoming.recv_timeout(patience) {
                Ok(line) => {
                    self.seen.push(line.clone());
                    if line.contains(text) {
                        return line;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the child ended before `{text}`; got {:#?}", self.seen)
                }
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no `{text}` in time; got {:#?}", self.seen)
                }
            }
        }
    }
}

/// Kills the child when a test ends early, so no process outlives the test run.
pub struct Process {
    pub child: Child,
    pub lines: Lines,
}

impl Process {
    /// Starts `command` and reads the lines of its stdout, or of its stderr.
    pub fn start(mut command: Command, reads_stderr: bool) -> Process {
        command.stdin(Stdio::piped());
        if reads_stderr {
            command.stdout(Stdio::null()).stderr(Stdio::piped());
        } else {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = command.spawn().expect("child starts");
        let lines = if reads_stderr {
            Lines::of(child.stderr.take().expect("stderr pipe"))
        } else {
            Lines::of(child.stdout.take().expect("stdout pipe"))
        };
        Process { child, lines }
    }

    pub fn type_line(&mut self, text: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        writeln!(stdin, "{text}").expect("line written to the child");
    }

    /// Exit code and everything the child wrote to stderr. Only for a child started with
    /// `reads_stderr` false.
    pub fn finish(mut self) -> (Option<i32>, String) {
        let status = self.child.wait().expect("child exit status");
        let mut errors = String::new();
        let stderr = self.child.stderr.as_mut().expect("stderr pipe");
        stderr.read_to_string(&mut errors).expect("stderr text");
        (status.code(), errors)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Both fail when the child has exited already, which is the normal case.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The fields drop in this order: the daemon is killed before its directories are removed.
pub struct Harness {
    pub daemon: Process,
    pub sway: FakeSway,
    pub programs: FakeProcs,
    pub daemon_dirs: TestDirs,
    pub client_dirs: TestDirs,
}

impl Harness {
    /// Must run inside a tokio runtime: the fake sway spawns its accept task.
    pub fn start() -> Harness {
        Harness::start_with("")
    }

    /// `extra_config` is TOML that follows the `server_name` line.
    pub fn start_with(extra_config: &str) -> Harness {
        let daemon_dirs = TestDirs::new();
        // The harness reads the environment, so the fake can refuse the owner's real socket.
        let forbidden: Vec<PathBuf> = ["SWAYSOCK", "I3SOCK"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        let sway = FakeSway::start(&daemon_dirs.runtime_dir(), &forbidden);
        let programs = FakeProcs::install(&daemon_dirs.root().join("bin"));
        let preamble = format!(
            "listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\n{extra_config}[sway]\nsocket = \"{}\"\n",
            sway.socket_path().display()
        );
        let lock_processes = format!("lock_processes = [\"{NO_LOCK_PROCESS}\"]");
        let grabbers = format!("keyboard_grabbers = [\"{NO_GRABBER_PROCESS}\"]");
        let config = TestConfig::with_sections(&programs, &DAEMON_SECTIONS)
            .preamble(&preamble)
            .line(ProgramSection::Session, &lock_processes)
            .line(ProgramSection::Session, &grabbers)
            .build();
        fs::write(daemon_dirs.config_file(), config).expect("config file");
        let mut run = daemon_dirs.command(daemon_binary());
        run.arg("run");
        let mut daemon = Process::start(run, true);
        daemon.lines.expect("listening");
        daemon.lines.expect("sway connected");
        Harness {
            daemon,
            sway,
            programs,
            daemon_dirs,
            client_dirs: TestDirs::new(),
        }
    }

    pub fn daemon_command(&self, arguments: &[&str]) -> Command {
        let mut command = self.daemon_dirs.command(daemon_binary());
        command.args(arguments);
        command
    }

    pub fn client_command(&self, arguments: &[&str]) -> Command {
        let mut command = self.client_dirs.command(CLIENT);
        command.args(arguments);
        command
    }

    /// Runs `swayconnect` to its end with nothing on stdin.
    pub fn client(&self, arguments: &[&str]) -> Output {
        self.client_with_stdin(arguments, "")
    }

    /// Runs `swayconnect` to its end with `stdin` on its standard input.
    pub fn client_with_stdin(&self, arguments: &[&str], stdin: &str) -> Output {
        assert_cmd::Command::from_std(self.client_command(arguments))
            .write_stdin(stdin)
            .timeout(PATIENCE)
            .output()
            .expect("client runs")
    }

    /// Stdout of a `swayconnect` run that must succeed.
    pub fn client_output(&self, arguments: &[&str]) -> String {
        let output = self.client(arguments);
        assert!(
            output.status.success(),
            "{arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 output")
    }

    /// The calls of `program`, once there are `count` of them: a fake program may log
    /// its call after the daemon answered.
    pub fn calls_once_there_are(&self, program: &str, count: usize) -> Vec<Vec<String>> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let calls = self.programs.calls(program);
            if calls.len() >= count {
                return calls;
            }
            assert!(Instant::now() < deadline, "{program}: {calls:?}");
            std::thread::sleep(RETRY_DELAY);
        }
    }

    pub fn profile_file(&self) -> PathBuf {
        let client_dir = self.client_dirs.data_home().join("swayconnect/client");
        client_dir.join("default.json")
    }

    /// `swayconnectd pair` and `swayconnect pair` up to the owner's prompt. Checks that
    /// both sides show the same code.
    pub fn pair_until_the_prompt(&self, owner_arguments: &[&str]) -> (Process, Process) {
        let mut owner = Process::start(self.daemon_command(owner_arguments), false);
        let uri = owner.lines.expect("swayconnect://pair");
        let mut client =
            Process::start(self.client_command(&["pair", "--name", "cli test"]), false);
        client.type_line(&uri);
        let client_code = client.lines.expect("code: ");
        let pending = owner.lines.expect("pending: ");
        assert!(
            pending.starts_with("pending: cli test (linux) from 127.0.0.1:"),
            "{pending}"
        );
        assert_eq!(owner.lines.expect("code: "), client_code);
        (owner, client)
    }

    /// Pairs with the owner's yes. The profile is on disk afterwards.
    pub fn pair(&self) {
        self.pair_through(&["pair"]);
    }

    /// Pairs with the owner's yes to `swayconnectd` with `owner_arguments`.
    pub fn pair_through(&self, owner_arguments: &[&str]) {
        let (mut owner, mut client) = self.pair_until_the_prompt(owner_arguments);
        owner.type_line("y");
        assert_eq!(client.lines.expect("paired"), "paired: desk");
        let (exit_code, errors) = client.finish();
        assert_eq!((exit_code, errors.as_str()), (Some(0), ""));
    }

    /// Returns once the daemon has its first snapshot of the fake sway: an action sent
    /// before that is `sway_unavailable`.
    pub fn wait_for_the_first_snapshot(&self) {
        let mut watch = Process::start(self.client_command(&["watch"]), false);
        watch.lines.expect("sway online");
    }

    /// Runs `swayconnect` with `arguments` and checks that it failed with `exit_code` and
    /// printed nothing. Returns its stderr.
    pub fn client_failure(&self, arguments: &[&str], exit_code: i32) -> String {
        let output = self.client(arguments);
        let errors = stderr(&output);
        assert_eq!(
            output.status.code(),
            Some(exit_code),
            "{arguments:?}: {errors}"
        );
        assert_eq!(output.stdout, b"", "{arguments:?}");
        errors
    }

    /// The frames that `swayconnect raw` printed for `payload`, one per line.
    pub fn raw_replies(&self, payload: &str) -> Vec<Value> {
        let printed = self.client_output(&["raw", payload]);
        printed.lines().map(parse).collect()
    }
}

pub fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("JSON")
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
