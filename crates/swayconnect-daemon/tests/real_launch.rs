//! Launching against the real programs (`docs/M2C_PLAN.md` 4, slice R4): a read-write daemon
//! in this process indexes the desktop entries of `tests/fixtures/real-launch` and starts
//! them with the real `systemd-run --user --scope` (owner decision D1). The test entry runs
//! foot in the headless sway of a `RealSession`. Every other program is `fake-proc`. Runs
//! only with the `real-backends` feature: `just real`.
//!
//! D1: `systemd-run` and `systemctl` reach the owner's user manager only through the one
//! link of `RealSession::allow_user_systemd`. The units the daemon creates are named
//! `app-swayconnect-<entry id>-<hex>.scope`; the test stops only new units of its own two
//! fixture entries, also when it panics, so a unit the owner's daemon starts meanwhile is
//! never touched.

#![cfg(feature = "real-backends")]

#[path = "v2_m2b/requests.rs"]
#[allow(dead_code, reason = "only the launch requests are used here")]
mod requests;
mod support;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use requests::{ask, detail, refused, topic_state};
use serde_json::Value;
use support::live::LiveAudio;
use support::{Pairing, soon};
use swayconnect_cli::client::{Event, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::apps::index::IndexDirs;
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::proc::{ChildEnv, RealRunner};
use swayconnect_protocol::ops::{Args, TOPIC_APPS};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{Apps, EntryId, ErrorCode, JsonObject, Scope};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{
    FakeProcs, LiveEnv, ProgramSection, RealSession, TestConfig, TestDirs, leftovers_naming,
    live_processes_in_group,
};

/// The real `systemd-run`; the terminal can never run, because the one terminal entry must
/// be refused before any start.
const REAL_APPS: &str =
    "[apps]\nsystemd_run = \"/usr/bin/systemd-run\"\nterminal = [\"/nonexistent/terminal\"]\n";
const FAKE_SECTIONS: [ProgramSection; 5] = [
    ProgramSection::Session,
    ProgramSection::KeepAwake,
    ProgramSection::Notify,
    ProgramSection::Audio,
    ProgramSection::Launcher,
];
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/real-launch");
const FOOT_ENTRY: &str = "test-foot";
const TERMINAL_ENTRY: &str = "test-terminal";
/// The `--app-id` in the `Exec` of `test-foot.desktop`.
const FOOT_APP_ID: &str = "swayconnect-test-r4";
const TARGET_WORKSPACE: u16 = 3;
const SYSTEMCTL: &str = "/usr/bin/systemctl";
/// The units this test may cause and stop (D1): those of its two fixture entries.
const TEST_UNITS: [&str; 2] = [
    "app-swayconnect-test-foot-*",
    "app-swayconnect-test-terminal-*",
];
/// Every unit a swayconnect daemon creates; the same before and after the test.
const DAEMON_UNITS: &str = "app-swayconnect-*";
const PAIRING_WINDOW: Duration = Duration::from_secs(300);
const LAUNCH_SCOPES: [Scope; 2] = [Scope::StateRead, Scope::Launch];

/// A sealed session with the D1 link, and a daemon whose children get only the session's
/// variables. Drop stops every test unit this rig caused (D1), also on panic, before the
/// session and its link go.
struct RealLaunch {
    /// The live sound server before the test; the same after it.
    live_before: Option<LiveAudio>,
    daemon: Option<DaemonHandle>,
    paths: Paths,
    units_before: BTreeSet<String>,
    test_units_before: BTreeSet<String>,
    _programs: FakeProcs,
    _dirs: TestDirs,
    session: RealSession,
}

impl RealLaunch {
    async fn start() -> RealLaunch {
        let live_before = LiveAudio::read();
        let mut session = RealSession::start(&LiveEnv::from_vars(std::env::vars_os()));
        session.allow_user_systemd();
        let units_before = list_units(&session, &[DAEMON_UNITS]).expect("list the daemon units");
        let test_units_before = list_units(&session, &TEST_UNITS).expect("list the test units");
        let dirs = TestDirs::new();
        let programs = FakeProcs::install(&dirs.root().join("bin"));
        let proc_root = dirs.root().join("proc");
        fs::create_dir(&proc_root).expect("fake /proc");
        let fake = TestConfig::with_sections(&programs, &FAKE_SECTIONS)
            .preamble("listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\nread_only = false")
            .build();
        let config = Config::from_toml(&format!("{fake}\n{REAL_APPS}")).expect("config");
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let sealed = session.checked_env(
            session
                .env()
                .map(|(name, value)| (OsString::from(name), value.to_owned())),
        );
        let exec_path = std::env::split_paths(&session_var(&session, "PATH")).collect();
        let host = Host {
            runner: Arc::new(RealRunner::new(ChildEnv::allowlisted(sealed))),
            proc_root,
            apps: IndexDirs {
                data_dirs: vec![PathBuf::from(FIXTURE)],
                exec_path,
            },
            ..Host::new(SwayTarget::Explicit(PathBuf::from(session_var(
                &session, "SWAYSOCK",
            ))))
        };
        let daemon = Daemon::start_on(config, paths.clone(), host)
            .await
            .expect("daemon starts");
        RealLaunch {
            live_before,
            daemon: Some(daemon),
            paths,
            units_before,
            test_units_before,
            _programs: programs,
            _dirs: dirs,
            session,
        }
    }

    /// Pairs a device with the owner's yes and connects it.
    async fn device(&self, scopes: &[Scope]) -> Session {
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
        let profile = soon("confirmed", pending.confirmed())
            .await
            .expect("pairing confirmed");
        soon("auth", support::connector().connect(&profile))
            .await
            .expect("authenticated")
    }

    /// The units of the fixture entries loaded now that were not loaded at the start.
    fn caused_units(&self) -> Result<BTreeSet<String>, String> {
        let now = list_units(&self.session, &TEST_UNITS)?;
        Ok(now
            .difference(&self.test_units_before)
            .filter(|unit| {
                [FOOT_ENTRY, TERMINAL_ENTRY]
                    .iter()
                    .any(|entry| is_unit_of(unit, entry))
            })
            .cloned()
            .collect())
    }

    /// Stops every unit this rig caused; `systemctl stop` waits until each is stopped.
    fn stop_caused_units(&self) -> Result<(), String> {
        let caused = self.caused_units()?;
        if caused.is_empty() {
            return Ok(());
        }
        let mut args = vec!["stop", "--no-ask-password", "--"];
        args.extend(caused.iter().map(String::as_str));
        systemctl(&self.session, &args).map(drop)
    }

    /// The windows of the sealed sway.
    fn windows(&self) -> Vec<Window> {
        let tree = self.session.swaymsg(&["-t", "get_tree"]);
        let tree: Value = serde_json::from_str(&tree).expect("sway tree is JSON");
        let mut windows = Vec::new();
        collect_windows(&tree, None, &mut windows);
        windows
    }

    /// Stops the daemon and the caused units, checks that the daemon units are as before,
    /// then tears the session down and checks that no process of it is left, also none the
    /// daemon started outside the session's group (the foot of the scope), and that the
    /// live sound server is as before.
    async fn finish(mut self) {
        if let Some(daemon) = self.daemon.take() {
            daemon.stop().await;
        }
        self.stop_caused_units().expect("stop the caused units");
        let units_after = list_units(&self.session, &[DAEMON_UNITS]).expect("list the units");
        assert_eq!(
            units_after, self.units_before,
            "{DAEMON_UNITS} before and after"
        );
        let group = self.session.process_group();
        let runtime_dir = self.session.runtime_dir();
        let live_before = self.live_before.take();
        drop(self);
        assert_eq!(live_processes_in_group(group), Vec::<u32>::new());
        assert_eq!(leftovers_naming(&runtime_dir), Vec::<u32>::new());
        LiveAudio::assert_unchanged(&live_before);
    }
}

impl Drop for RealLaunch {
    fn drop(&mut self) {
        // A panic here would abort a test that is already panicking, so only report.
        if let Err(error) = self.stop_caused_units() {
            eprintln!("D1: cannot stop the units this test caused ({TEST_UNITS:?}): {error}");
        }
    }
}

/// One window of the sway tree.
#[derive(Debug)]
struct Window {
    app_id: Option<String>,
    workspace: Option<String>,
    pid: u32,
}

fn collect_windows(node: &Value, workspace: Option<&str>, windows: &mut Vec<Window>) {
    let workspace = match node["type"].as_str() {
        Some("workspace") => node["name"].as_str(),
        _ => workspace,
    };
    if let Some(pid) = node["pid"].as_u64() {
        windows.push(Window {
            app_id: node["app_id"].as_str().map(str::to_owned),
            workspace: workspace.map(str::to_owned),
            pid: u32::try_from(pid).expect("a pid fits u32"),
        });
    }
    for child in ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|member| node[member].as_array())
        .flatten()
    {
        collect_windows(child, workspace, windows);
    }
}

/// `systemctl --user` with the session's environment, checked by its guard, so it reaches
/// only the user manager through the D1 link. Returns stdout.
fn systemctl(session: &RealSession, args: &[&str]) -> Result<String, String> {
    let mut command = session.command(SYSTEMCTL);
    command.arg("--user").args(args);
    let output = session.output(command);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("systemctl {args:?}: {}: {stderr}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("systemctl output: {error}"))
}

/// The names of the loaded units that match one of `patterns`.
fn list_units(session: &RealSession, patterns: &[&str]) -> Result<BTreeSet<String>, String> {
    let mut args = vec![
        "list-units",
        "--all",
        "--plain",
        "--no-legend",
        "--full",
        "--",
    ];
    args.extend(patterns);
    let stdout = systemctl(session, &args)?;
    Ok(stdout
        .lines()
        .filter_map(|line| line.split_whitespace().find(|word| *word != "●"))
        .map(str::to_owned)
        .collect())
}

fn session_var(session: &RealSession, name: &str) -> OsString {
    session
        .env()
        .find(|(var, _)| *var == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_else(|| panic!("the sealed session sets {name}"))
}

fn launch_on(entry_id: &str, number: Option<u16>) -> Args {
    let entry_id = EntryId::parse(entry_id).expect("entry id");
    Args::AppLaunch { entry_id, number }
}

/// The apps state once both fixture entries are listed.
async fn listed_apps(phone: &mut Session) -> Apps {
    let both = |data: &JsonObject| Apps::decode(data).is_ok_and(|apps| apps.apps.len() == 2);
    Apps::decode(&topic_state(phone, TOPIC_APPS, both).await).expect("apps state")
}

/// The next apps state whose `recent` list is `recent`.
async fn apps_with_recent(phone: &mut Session, recent: &[&str]) -> Apps {
    loop {
        match soon("state.changed", phone.next_event()).await {
            Ok(Event::State(state)) if state.topic == TOPIC_APPS => {
                let apps = Apps::decode(&state.data).expect("apps state");
                if apps.recent == recent {
                    return apps;
                }
            }
            Ok(Event::State(_)) => {}
            other => panic!("not a state event: {other:?}"),
        }
    }
}

fn is_unit_of(name: &str, entry_id: &str) -> bool {
    let Some(suffix) = name
        .strip_prefix(&format!("app-swayconnect-{entry_id}-"))
        .and_then(|rest| rest.strip_suffix(".scope"))
    else {
        return false;
    };
    suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_desktop_entry_starts_in_its_own_scope_on_the_target_workspace() {
    let rig = RealLaunch::start().await;
    let mut phone = rig.device(&LAUNCH_SCOPES).await;
    let apps = listed_apps(&mut phone).await;
    let risky: Vec<(&str, bool)> = apps
        .apps
        .iter()
        .map(|app| (app.entry_id.as_str(), app.risky))
        .collect();
    assert_eq!(risky, [(FOOT_ENTRY, false), (TERMINAL_ENTRY, true)]);

    ask(&mut phone, &launch_on(FOOT_ENTRY, Some(TARGET_WORKSPACE)))
        .await
        .expect("the test entry starts");
    tokio::task::block_in_place(|| rig.session.wait_for_window(FOOT_APP_ID));
    let windows = rig.windows();
    let [window] = windows.as_slice() else {
        panic!("one window in the sealed sway: {windows:?}");
    };
    assert_eq!(window.app_id.as_deref(), Some(FOOT_APP_ID));
    let target = TARGET_WORKSPACE.to_string();
    assert_eq!(window.workspace.as_deref(), Some(target.as_str()));

    let caused = rig.caused_units().expect("list the test units");
    let [unit] = Vec::from_iter(caused).try_into().unwrap_or_else(|units| {
        panic!("the launch caused one unit: {units:?}");
    });
    assert!(is_unit_of(&unit, FOOT_ENTRY), "unit name {unit}");
    let cgroup = fs::read_to_string(format!("/proc/{}/cgroup", window.pid)).expect("cgroup");
    assert!(
        cgroup.trim_end().ends_with(&format!("/{unit}")),
        "foot runs in {unit}: {cgroup}"
    );
    let shots = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-session");
    fs::create_dir_all(&shots).expect("screenshot dir");
    rig.session.screenshot(&shots.join("launch-foot.png"));

    let apps = apps_with_recent(&mut phone, &[FOOT_ENTRY]).await;
    assert_eq!(apps.recent, [FOOT_ENTRY]);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_entry_is_refused_without_launch_risky() {
    let rig = RealLaunch::start().await;
    let mut phone = rig.device(&LAUNCH_SCOPES).await;
    listed_apps(&mut phone).await;

    let error = refused(
        &mut phone,
        &launch_on(TERMINAL_ENTRY, Some(TARGET_WORKSPACE)),
        ErrorCode::Forbidden,
    )
    .await;
    assert_eq!(detail(&error, "scope"), "launch.risky");
    let caused = rig.caused_units().expect("list the test units");
    assert_eq!(caused, BTreeSet::new(), "no unit for a refused entry");
    let windows = rig.windows();
    assert!(windows.is_empty(), "no window: {windows:?}");
    let workspaces = rig.session.swaymsg(&["-t", "get_workspaces"]);
    let workspaces: Value = serde_json::from_str(&workspaces).expect("workspaces JSON");
    let focused = workspaces
        .as_array()
        .expect("workspace list")
        .iter()
        .find(|workspace| workspace["focused"] == true)
        .expect("a focused workspace");
    assert_ne!(
        focused["num"], TARGET_WORKSPACE,
        "a refused entry does not switch the workspace"
    );
    rig.finish().await;
}
