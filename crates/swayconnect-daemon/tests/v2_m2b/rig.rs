//! The M2b test rig: a daemon in this process on the fake sway, a fake `/proc`, the
//! testkit's `fake-proc` for every program (started through the real runner), a recording
//! keyboard, the fake media backend and two desktop entries.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value;
use swayconnect_cli::client::{Profile, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::apps::index::IndexDirs;
use swayconnect_daemon::audit::audit_file;
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::services::{KeyboardHost, MediaHost};
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::input::{Delivery, KeyboardSink, SinkError};
use swayconnect_daemon::media::MediaBackend;
use swayconnect_daemon::media::fake::FakeMedia;
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::proc::BoxFuture;
use swayconnect_protocol::Scope;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{FakeProcs, FakeSway, ProgramSection, Request, TestConfig, TestDirs};
use swayconnect_testkit::{assert_no_system_programs, fixtures};

use crate::support::{self, Pairing, soon};

pub const PACTL: &str = "pactl";
pub const SYSTEMD_RUN: &str = "systemd-run";
pub const TERMINAL: &str = "alacritty";
pub const LAUNCHER: &str = "fuzzel";
pub const NOTIFY: &str = "notify-send";
/// The text of the connect notice (`docs/DESIGN.md` 11.8), before the device short id.
pub const CONNECT_NOTICE: &str = "Device with remote control rights connected (device ";
/// Entry ids of [`DESKTOP_ENTRIES`]: a plain app and a terminal app (always risky).
pub const PLAIN_ENTRY: &str = "notes";
pub const RISKY_ENTRY: &str = "term";
const PAIRING_WINDOW: Duration = Duration::from_secs(300);

/// `Exec` lines name programs that do not exist: the fake `systemd-run` starts nothing.
const DESKTOP_ENTRIES: [(&str, &str); 2] = [
    (
        "notes.desktop",
        "[Desktop Entry]\nType=Application\nName=Notes\nExec=/opt/notes --new %U\n",
    ),
    (
        "term.desktop",
        "[Desktop Entry]\nType=Application\nName=Term\nExec=/opt/term\nTerminal=true\n",
    ),
];

/// What the fake `pactl` prints for every call. As a list it is one device, so the daemon
/// sees it once as a sink and once as a source (no `monitor_source`: not a monitor). As a
/// default name it holds newlines, so no default is set. `subscribe` prints it and ends.
const PACTL_OUTPUT: &str = r#"[
  {"name": "test.device", "description": "Test device", "mute": true,
   "volume": {"mono": {"value": 32768}}, "properties": {"device.bus": "usb"}}
]
"#;

pub struct Rig {
    pub daemon: DaemonHandle,
    pub paths: Paths,
    pub sway: FakeSway,
    pub programs: FakeProcs,
    pub keyboard: Arc<Keyboard>,
    pub media: Arc<FakeMedia>,
    proc_root: PathBuf,
    // Last, so that the directories go after the daemon and the fake sway.
    pub dirs: TestDirs,
}

impl Rig {
    /// The fake sway serves the layout fixture: window 12 is focused.
    pub async fn start(read_only: bool) -> Rig {
        let dirs = TestDirs::new();
        let sway = FakeSway::start(&dirs.runtime_dir(), &support::real_sway_sockets());
        sway.set_reply(Request::GetTree, fixtures::layout_tree());
        sway.set_reply(Request::GetWorkspaces, fixtures::layout_workspaces());
        let programs = FakeProcs::install(&dirs.root().join("bin"));
        // Before the daemon starts: the audio service runs pactl at once.
        programs.set_stdout(PACTL, PACTL_OUTPUT.as_bytes());
        let proc_root = dirs.root().join("proc");
        fs::create_dir(&proc_root).expect("fake /proc");
        let text = TestConfig::with_sections(&programs, &ProgramSection::ALL)
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
        let keyboard = Arc::new(Keyboard::default());
        let media = Arc::new(FakeMedia::default());
        let host = Host {
            proc_root: proc_root.clone(),
            keyboard: KeyboardHost::Sink(Arc::clone(&keyboard) as Arc<dyn KeyboardSink>),
            media: MediaHost::Backend(Arc::clone(&media) as Arc<dyn MediaBackend>),
            apps: desktop_entries(&dirs),
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
            keyboard,
            media,
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

    /// The pactl calls that change something; the reads and `subscribe` are left out.
    pub fn pactl_writes(&self) -> Vec<Vec<String>> {
        let mut calls = self.programs.calls(PACTL);
        // A write is `-- set-<what> <name> [value]` (`docs/DESIGN.md` 11.6).
        calls.retain(|argv| match argv.as_slice() {
            [dash, verb, ..] => dash == "--" && verb.starts_with("set-"),
            _ => false,
        });
        calls
    }

    /// The audit file as text, and its device-request lines; pairing lines are the owner's.
    pub fn audit(&self) -> (String, Vec<Value>) {
        let path = audit_file(&self.paths);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(error) => panic!("cannot read {}: {error}", path.display()),
        };
        let lines = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("audit line is JSON"))
            .filter(|line| line["op"] != "pair")
            .collect();
        (text, lines)
    }

    /// [`Rig::assert_nothing_ran`], and nothing was audited.
    pub fn assert_untouched(&self) {
        self.assert_nothing_ran();
        assert_eq!(self.audit().1, Vec::<Value>::new());
    }

    /// No sway command, no pactl write, no program started, no key typed, no media
    /// command, no desktop notice but the connect notice of a device with a tier 2 scope.
    pub fn assert_nothing_ran(&self) {
        assert_eq!(self.sway.commands(), Vec::<String>::new());
        assert_eq!(self.pactl_writes(), Vec::<Vec<String>>::new());
        for program in [SYSTEMD_RUN, TERMINAL, LAUNCHER] {
            let calls = self.programs.calls(program);
            assert_eq!(calls, Vec::<Vec<String>>::new(), "{program}");
        }
        let notices: Vec<Vec<String>> = self
            .programs
            .calls(NOTIFY)
            .into_iter()
            .filter(|argv| {
                argv.last()
                    .is_none_or(|text| !text.starts_with(CONNECT_NOTICE))
            })
            .collect();
        assert_eq!(notices, Vec::<Vec<String>>::new(), "{NOTIFY}");
        assert_eq!(self.keyboard.parts(), 0, "no key reached the keyboard");
        assert_eq!(self.media.sent(), Vec::new());
    }
}

/// A data directory with [`DESKTOP_ENTRIES`] in its `applications`.
fn desktop_entries(dirs: &TestDirs) -> IndexDirs {
    let data_dir = dirs.root().join("entries");
    let applications = data_dir.join("applications");
    fs::create_dir_all(&applications).expect("applications dir");
    for (name, text) in DESKTOP_ENTRIES {
        fs::write(applications.join(name), text).expect("desktop entry");
    }
    IndexDirs {
        data_dirs: vec![data_dir],
        exec_path: Vec::new(),
    }
}

/// A [`KeyboardSink`] that keeps every part it gets and never opens a Wayland socket.
#[derive(Default)]
pub struct Keyboard {
    deliveries: Mutex<Vec<Delivery>>,
}

impl Keyboard {
    pub fn parts(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Delivery>> {
        self.deliveries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl KeyboardSink for Keyboard {
    fn deliver(&self, delivery: Delivery) -> BoxFuture<'_, Result<(), SinkError>> {
        self.lock().push(delivery);
        Box::pin(async { Ok(()) })
    }

    fn release_all(&self) {}
}
