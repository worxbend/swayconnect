//! The M2a test rig: a daemon in this process on the fake sway, a fake `/proc` and the
//! testkit's `fake-proc` for every program, started through the real runner.

use std::fs;
use std::io::{BufRead as _, BufReader, ErrorKind, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::Duration;

use rustix::fs::{FileType, Mode, OFlags};
use serde_json::Value;
use swayconnect_cli::client::{Profile, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::audit::audit_file;
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::paths::Paths;
use swayconnect_protocol::Scope;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::assert_no_system_programs;
use swayconnect_testkit::{FakeProcs, FakeSway, ProgramSection, TestConfig, TestDirs};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::support::{self, Pairing, soon};

pub const INHIBIT: &str = "systemd-inhibit";
pub const NOTIFY: &str = "notify-send";
pub const LOCK: &str = "lock.sh";
const PAIRING_WINDOW: Duration = Duration::from_secs(300);
/// Every program section (`docs/DESIGN.md` 11.9): the M2b audio service starts `pactl` at
/// once, so it must be `fake-proc` too.
const M2A_SECTIONS: [ProgramSection; 6] = ProgramSection::ALL;

pub struct Rig {
    pub daemon: DaemonHandle,
    pub paths: Paths,
    pub sway: FakeSway,
    pub programs: FakeProcs,
    proc_root: PathBuf,
    // Last, so that the directories go after the daemon and the fake sway.
    pub dirs: TestDirs,
}

impl Rig {
    /// `prepare` scripts the fake sway before the daemon first connects to it.
    pub async fn start(read_only: bool, prepare: impl FnOnce(&FakeSway)) -> Rig {
        let dirs = TestDirs::new();
        let sway = FakeSway::start(&dirs.runtime_dir(), &support::real_sway_sockets());
        prepare(&sway);
        let programs = FakeProcs::install(&dirs.root().join("bin"));
        let proc_root = dirs.root().join("proc");
        fs::create_dir(&proc_root).expect("fake /proc");
        let text = TestConfig::with_sections(&programs, &M2A_SECTIONS)
            .preamble("listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"")
            .preamble(&format!("read_only = {read_only}"))
            .build();
        assert_no_system_programs(&text);
        let config = Config::from_toml(&text).expect("config");
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let host = Host {
            proc_root: proc_root.clone(),
            ..Host::new(SwayTarget::Explicit(dirs.sway_socket()))
        };
        let daemon = Daemon::start_on(config, paths.clone(), host)
            .await
            .expect("daemon starts");
        Rig {
            daemon,
            paths,
            sway,
            programs,
            proc_root,
            dirs,
        }
    }

    /// The whole pairing flow with the owner's yes.
    pub async fn pair(&self, scopes: &[Scope]) -> Profile {
        let mut admin = soon("admin", AdminClient::connect(&self.paths))
            .await
            .expect("admin socket");
        let request = PairingRequest {
            scopes: scopes.iter().copied().collect(),
            lifetime: PAIRING_WINDOW,
            host: None,
        };
        let uri = soon("pairing window", admin.open_pairing(&request))
            .await
            .expect("window");
        let uri = PairingUri::parse(&uri).expect("pairing URI");
        let mut pairing = Pairing { admin, uri };
        let pending = pairing.present_secret().await;
        pairing.answer(true).await;
        soon("confirmed", pending.confirmed())
            .await
            .expect("pairing confirmed")
    }

    pub async fn connect(&self, profile: &Profile) -> Session {
        soon("auth", support::connector().connect(profile))
            .await
            .expect("authenticated")
    }

    pub async fn device(&self, scopes: &[Scope]) -> (Profile, Session) {
        let profile = self.pair(scopes).await;
        let session = self.connect(&profile).await;
        (profile, session)
    }

    /// A process of the own uid named `name` appears in the fake `/proc`.
    pub fn add_process(&self, pid: u32, name: &str) {
        let uid = fs::metadata(self.dirs.root()).expect("own directory").uid();
        let dir = self.proc_root.join(pid.to_string());
        fs::create_dir(&dir).expect("fake process");
        let status =
            format!("Name:\t{name}\nState:\tS (sleeping)\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n");
        fs::write(dir.join("status"), status).expect("fake status");
    }

    pub fn remove_process(&self, pid: u32) {
        fs::remove_dir_all(self.proc_root.join(pid.to_string())).expect("process removed");
    }

    /// Audit lines of device requests; pairing lines are the owner's.
    pub fn request_lines(&self) -> Vec<Value> {
        let path = audit_file(&self.paths);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(error) => panic!("cannot read {}: {error}", path.display()),
        };
        text.lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("audit line is JSON"))
            .filter(|line| line["op"] != "pair")
            .collect()
    }

    /// Nothing reached sway, no program ran, nothing was audited.
    pub fn assert_untouched(&self) {
        assert_eq!(self.sway.commands(), Vec::<String>::new());
        for program in [INHIBIT, NOTIFY, LOCK] {
            let calls = self.programs.calls(program);
            assert_eq!(calls, Vec::<Vec<String>>::new(), "{program}");
        }
        assert_eq!(self.request_lines(), Vec::<Value>::new());
    }
}

/// The call log of one fake program, made a FIFO. The program waits on its log write until
/// the test reads it, so the test reads the argv without polling, and the child counts as
/// running until then.
pub struct Gate {
    log: PathBuf,
}

impl Gate {
    pub fn install(programs: &FakeProcs, name: &str) -> Gate {
        // `<program>.log`, where `fake-proc` appends its argv (`docs/V2_PLAN.md` TK.2).
        let mut log = programs.program(name).into_os_string();
        log.push(".log");
        let log = PathBuf::from(log);
        let mode = Mode::from_raw_mode(0o600);
        rustix::fs::mknodat(rustix::fs::CWD, &log, FileType::Fifo, mode, 0).expect("FIFO");
        Gate { log }
    }

    /// Lets the one waiting call through and returns its arguments.
    pub async fn release(&self) -> Vec<String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let log = self.log.clone();
        // A thread, not `spawn_blocking`: the runtime of a failed test must not wait for it.
        std::thread::spawn(move || sender.send(fs::read_to_string(&log)));
        let text = soon("fake program call", receiver)
            .await
            .expect("reader")
            .expect("read log");
        let [call] = text
            .lines()
            .collect::<Vec<_>>()
            .try_into()
            .expect("exactly one call");
        serde_json::from_str(call).expect("argv as a JSON array")
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        // Frees a program still waiting at the gate: its log write then fails and it exits.
        let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let _ = rustix::fs::open(&self.log, flags, Mode::empty());
    }
}

/// `swayconnectd pair` as a child, with its stdout read on a thread so the test waits
/// without blocking the runtime. Killed when the test ends early.
pub struct PairCommand {
    child: Child,
    lines: UnboundedReceiver<String>,
}

impl PairCommand {
    pub fn start(dirs: &TestDirs) -> PairCommand {
        let mut command = dirs.command(env!("CARGO_BIN_EXE_swayconnectd"));
        command
            .arg("pair")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = command.spawn().expect("pair starts");
        let stdout = child.stdout.take().expect("stdout pipe");
        let (sender, lines) = unbounded_channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        PairCommand { child, lines }
    }

    /// The next line that contains `text`.
    pub async fn line_with(&mut self, text: &str) -> String {
        loop {
            let line = soon(text, self.lines.recv())
                .await
                .expect("pair ended early");
            if line.contains(text) {
                return line;
            }
        }
    }

    pub fn answer(&mut self, text: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        stdin.write_all(text.as_bytes()).expect("answer written");
    }
}

impl Drop for PairCommand {
    fn drop(&mut self) {
        // Both fail for a child that has exited already, the normal case.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
