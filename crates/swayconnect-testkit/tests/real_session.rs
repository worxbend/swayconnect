//! The sealed session against the real programs (`docs/M2C_PLAN.md` 3, slice R0).
//! Runs only with the `real-backends` feature: `just real`.

#![cfg(feature = "real-backends")]

use std::collections::BTreeSet;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rustix::fs::{CWD, Mode, mkfifoat};
use serde_json::Value;
use swayconnect_testkit::{
    LiveEnv, RealSession, SEALED_SINKS, SEALED_SOURCE, kept_log_dir, live_processes_in_group,
    processes_naming,
};

const FOOT: &str = "/usr/bin/foot";
const FUZZEL: &str = "/usr/bin/fuzzel";
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const SCREEN_CHANGE_TIMEOUT: Duration = Duration::from_secs(20);
const ENV: &str = "/usr/bin/env";
/// Variables bash sets for itself when it starts with an empty environment.
const SHELL_OWN: [&str; 3] = ["PWD", "SHLVL", "_"];
const FIFO_TIMEOUT: Duration = Duration::from_secs(20);

/// The test process's environment names the live session; the guard refuses it.
fn live_env() -> LiveEnv {
    LiveEnv::from_vars(std::env::vars_os())
}

/// Where screenshots stay after the run, as evidence.
fn shot_path(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-session");
    fs::create_dir_all(&dir).expect("create the screenshot dir");
    dir.join(format!("{name}.png"))
}

/// Name, mute and volume of each live sink, read only (`pactl -f json list sinks` on the
/// live server). `None` when no live sound server answers.
fn live_sinks() -> Option<Vec<(Value, Value, Value)>> {
    let output = Command::new("/usr/bin/pactl")
        .args(["-f", "json", "list", "sinks"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let sinks: Vec<Value> = serde_json::from_slice(&output.stdout).expect("pactl JSON");
    Some(
        sinks
            .into_iter()
            .map(|sink| {
                (
                    sink["name"].clone(),
                    sink["mute"].clone(),
                    sink["volume"].clone(),
                )
            })
            .collect(),
    )
}

/// The names of the variables in the output of `env`.
fn names(env: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(env)
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_owned()))
        .collect()
}

/// Everything written into the FIFO `path` until its writer closes it.
fn read_fifo(path: &Path) -> Vec<u8> {
    let (sender, receiver) = mpsc::channel();
    let path = path.to_owned();
    std::thread::spawn(move || {
        let _ = sender.send(fs::read(path).expect("read the FIFO"));
    });
    receiver
        .recv_timeout(FIFO_TIMEOUT)
        .expect("the writer closed the FIFO")
}

fn assert_torn_down(group: u32, runtime_dir: &Path) {
    assert_eq!(
        live_processes_in_group(group),
        Vec::<u32>::new(),
        "group {group}"
    );
    assert_eq!(processes_naming(runtime_dir), Vec::<u32>::new());
    assert!(
        !runtime_dir.exists(),
        "{} was not removed",
        runtime_dir.display()
    );
}

/// Takes screenshots until one differs from `baseline`.
fn wait_for_screen_change(session: &RealSession, baseline: &Path, name: &str) -> PathBuf {
    let before = fs::read(baseline).expect("baseline screenshot");
    let path = shot_path(name);
    let deadline = Instant::now() + SCREEN_CHANGE_TIMEOUT;
    while Instant::now() < deadline {
        session.screenshot(&path);
        if fs::read(&path).expect("screenshot") != before {
            return path;
        }
    }
    panic!("the headless output did not change within {SCREEN_CHANGE_TIMEOUT:?}");
}

#[test]
fn ten_sessions_start_and_stop_without_leftovers() {
    let live = live_env();
    let live_sinks_before = live_sinks();

    for round in 0..10 {
        let session = RealSession::start(&live);
        let group = session.process_group();
        let runtime_dir = session.runtime_dir();

        let sinks = session.pactl(&["list", "short", "sinks"]);
        let sources = session.pactl(&["list", "short", "sources"]);
        for sink in SEALED_SINKS {
            assert!(sinks.contains(sink), "round {round}: {sinks}");
        }
        assert!(sources.contains(SEALED_SOURCE), "round {round}: {sources}");
        assert_eq!(sinks.lines().count(), 2, "only the null sinks: {sinks}");
        assert_eq!(session.pactl(&["get-default-source"]).trim(), SEALED_SOURCE);
        let outputs = session.swaymsg(&["-t", "get_outputs"]);
        assert!(outputs.contains("\"HEADLESS-1\""), "{outputs}");
        session.screenshot(&shot_path(&format!("smoke-{round}")));

        drop(session);
        assert_torn_down(group, &runtime_dir);
    }

    assert_eq!(live_sinks(), live_sinks_before, "the live sinks changed");
}

#[test]
fn foot_and_fuzzel_run_on_headless_pixman() {
    let mut session = RealSession::start(&live_env());
    let group = session.process_group();
    let runtime_dir = session.runtime_dir();
    let empty = shot_path("empty");
    session.screenshot(&empty);

    let mut foot = session.command(FOOT);
    foot.args(["--app-id", "sealed-foot", "--title", "sealed-foot"])
        .args(["sh", "-c", "cat > /dev/null"]);
    session.spawn(foot);
    session.wait_for_window("sealed-foot");
    let with_foot = wait_for_screen_change(&session, &empty, "foot");
    let tree = session.swaymsg(&["-t", "get_tree"]);
    assert!(tree.contains("\"app_id\": \"sealed-foot\""), "{tree}");

    // fuzzel is a layer surface, absent from the sway tree: its drawing is the proof.
    let fuzzel = session.command(FUZZEL);
    session.spawn(fuzzel);
    wait_for_screen_change(&session, &with_foot, "fuzzel");

    drop(session);
    assert_torn_down(group, &runtime_dir);
}

#[test]
fn the_guard_refuses_a_command_that_names_the_live_session() {
    let live = live_env();
    let mut session = RealSession::start(&live);
    let live_dir = std::env::var_os("XDG_RUNTIME_DIR").expect("just real runs in a login session");
    let mut leaked = session.command("/usr/bin/true");
    leaked.env("SWAYSOCK", Path::new(&live_dir).join("sway-ipc.sock"));

    let refused = panic::catch_unwind(AssertUnwindSafe(|| session.spawn(leaked)));

    let message = refused.expect_err("the guard trips");
    let message = message
        .downcast_ref::<String>()
        .expect("a formatted panic message");
    assert!(
        message.contains("sealed session guard: SWAYSOCK="),
        "{message}"
    );
}

/// A command built without `RealSession::command` (no `env_clear`) still gets nothing of
/// the test process's environment, which names the live session.
#[test]
fn a_started_program_inherits_nothing_of_the_test_process() {
    let mut session = RealSession::start(&live_env());
    let sealed: BTreeSet<String> = session.env().map(|(name, _)| name.to_owned()).collect();

    let bare = session.output(Command::new(ENV));
    let from_session = session.output(session.command(ENV));
    let fifo = session.runtime_dir().join("env.fifo");
    let mut spawned = Command::new("/bin/sh");
    spawned
        .arg("-c")
        .arg(format!("exec {ENV} > '{}'", fifo.display()));
    mkfifoat(CWD, &fifo, Mode::from_raw_mode(0o600)).expect("mkfifo");
    session.spawn(spawned);
    let reader = read_fifo(&fifo);

    assert!(bare.status.success());
    assert_eq!(names(&bare.stdout), BTreeSet::new(), "a bare command");
    assert_eq!(names(&from_session.stdout), sealed);
    let leaked: Vec<String> = names(&reader)
        .into_iter()
        .filter(|name| !SHELL_OWN.contains(&name.as_str()))
        .collect();
    assert_eq!(leaked, Vec::<String>::new(), "a spawned bare command");
}

/// The sealed sound server has no device of any kind: WirePlumber and PipeWire read no
/// drop-in that could add an ALSA, Bluetooth, V4L2 or libcamera monitor.
#[test]
fn the_sealed_audio_has_no_hardware_object() {
    let session = RealSession::start(&live_env());

    let dump = session.output(session.command("/usr/bin/pw-dump"));

    assert!(
        dump.status.success(),
        "{}",
        String::from_utf8_lossy(&dump.stderr)
    );
    let objects: Vec<Value> = serde_json::from_slice(&dump.stdout).expect("pw-dump JSON");
    let hardware: Vec<&Value> = objects
        .iter()
        .filter(|object| {
            let props = &object["info"]["props"];
            object["type"] == "PipeWire:Interface:Device"
                || props["factory.name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("api."))
                || props["device.api"].is_string()
        })
        .collect();
    assert!(hardware.is_empty(), "{hardware:#?}");
    let nodes = objects
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Node")
        .count();
    assert!(nodes >= 3, "the null devices are nodes: {nodes}");
}

#[test]
fn a_test_that_panics_still_tears_the_session_down() {
    let mut started = None;

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let session = RealSession::start(&live_env());
        started = Some((session.process_group(), session.runtime_dir()));
        panic!("a failing test");
    }));

    assert!(result.is_err());
    let (group, runtime_dir) = started.expect("the session started");
    assert_torn_down(group, &runtime_dir);
    let kept = kept_log_dir(&runtime_dir);
    assert!(kept.join("sway.err").is_file(), "{} kept", kept.display());
    fs::remove_dir_all(kept).expect("remove the kept logs");
}

/// Decision D1: the narrow path to the real user manager is closed by default and opens
/// per test. The scope runs `cat /proc/self/cgroup` and ends with it; `--collect`
/// removes the unit even when it fails.
#[test]
fn systemd_run_reaches_the_user_manager_only_when_allowed() {
    let mut session = RealSession::start(&live_env());
    let unit = format!("app-swayconnect-test-r0-{}", std::process::id());

    let sealed = run_scope(&session, &unit);
    session.allow_user_systemd();
    let allowed = run_scope(&session, &unit);

    assert!(
        !sealed.status.success(),
        "reached a user manager while sealed"
    );
    assert!(
        allowed.status.success(),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    let cgroup = String::from_utf8_lossy(&allowed.stdout);
    assert!(cgroup.contains(&format!("/{unit}.scope")), "{cgroup}");
}

/// `systemd-run --user --scope` of `cat /proc/self/cgroup` in unit `unit`. It answers at
/// once, with or without a manager.
fn run_scope(session: &RealSession, unit: &str) -> Output {
    let mut command = session.command(SYSTEMD_RUN);
    command
        .args(["--user", "--scope", "--quiet", "--collect"])
        .arg(format!("--unit={unit}"))
        .args(["--", "/usr/bin/cat", "/proc/self/cgroup"]);
    session.output(command)
}
