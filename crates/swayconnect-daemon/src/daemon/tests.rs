//! Tests of `daemon.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use swayconnect_protocol::DeviceId;
use swayconnect_protocol::limits::DEFAULT_PORT;
use swayconnect_protocol::ops::KeepAwake as KeepAwakeArgs;
use swayconnect_testkit::{FakeSway, TestDirs};

use super::*;
use crate::apps::OpenError;
use crate::input::fake::{FakeKeyboard, Recorded};
use crate::keep_awake::KeepAwakeError;
use crate::proc::fake::{CallKind, FakeRunner};

const CRATES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
// Split, so that this file does not hold the texts it searches for.
const ENVIRONMENT_READS: [&str; 3] = [
    concat!("std::env", "::var"),
    concat!("env::var", "_os"),
    concat!("env::va", "rs"),
];
/// A daemon starts `pactl` at once: tests name a program that does not exist.
const LOOPBACK_WITHOUT_PROGRAMS: &str =
    "listen = [\"127.0.0.1:0\"]\n[audio]\npactl = \"/fake/pactl\"";
const COMMAND_TEXTS: [&str; 2] = [
    concat!("[con", "_id="),
    concat!("no-auto", "-back-and-forth"),
];

fn rust_files_below(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("readable directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files_below(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// Every file under `crates/*/src/` that holds one of `needles`, as a path below `crates/`.
fn source_files_with(needles: &[&str]) -> Vec<String> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(CRATES_DIR).expect("crates directory") {
        let source_dir = entry.expect("crate directory").path().join("src");
        if source_dir.is_dir() {
            rust_files_below(&source_dir, &mut sources);
        }
    }
    assert!(sources.len() > 20, "the scan found the workspace sources");
    let crates_dir = Path::new(CRATES_DIR)
        .canonicalize()
        .expect("crates directory");
    let mut hits = Vec::new();
    for source in sources {
        let text = fs::read_to_string(&source).expect("readable source file");
        if needles.iter().any(|needle| text.contains(needle)) {
            let source = source.canonicalize().expect("source path");
            let relative = source.strip_prefix(&crates_dir).expect("below crates/");
            hits.push(relative.display().to_string());
        }
    }
    hits.sort();
    hits
}

#[test]
fn only_the_two_main_files_read_the_environment() {
    let allowed = [
        "swayconnect-cli/src/main.rs",
        "swayconnect-daemon/src/main.rs",
    ];
    let mut readers = source_files_with(&ENVIRONMENT_READS);
    readers.retain(|file| !allowed.contains(&file.as_str()));
    assert_eq!(readers, Vec::<String>::new());
}

#[test]
fn only_the_command_builder_spells_sway_command_text() {
    let mut spellers = source_files_with(&COMMAND_TEXTS);
    // The builder and its own tests, which check the text it renders.
    spellers.retain(|file| {
        file != "swayconnect-sway/src/command.rs"
            && !file.starts_with("swayconnect-sway/src/command/")
    });
    assert_eq!(spellers, Vec::<String>::new());
}

#[tokio::test]
async fn the_daemon_binds_an_ephemeral_port_and_removes_the_admin_socket_on_stop() {
    let dirs = TestDirs::new();
    let _sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let config = Config::from_toml(LOOPBACK_WITHOUT_PROGRAMS).expect("config");
    let target = SwayTarget::Explicit(dirs.sway_socket());

    let daemon = Daemon::start(config, paths.clone(), target)
        .await
        .expect("daemon starts");

    let [address] = daemon.bound() else {
        panic!("one listen address, got {:?}", daemon.bound());
    };
    assert!(address.ip().is_loopback());
    assert!(![0, DEFAULT_PORT].contains(&address.port()));
    assert!(paths.admin_socket().exists());
    assert!(paths.identity_cert().exists());

    daemon.stop().await;
    assert!(!paths.admin_socket().exists());
}

/// A daemon on fakes for the panic switch and stop tests. Keep the dirs and the fake sway
/// alive for the test's length.
struct FakeDaemon {
    daemon: DaemonHandle,
    runner: FakeRunner,
    keyboard: FakeKeyboard,
    _sway: FakeSway,
    _dirs: TestDirs,
}

async fn fake_daemon() -> FakeDaemon {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let toml = format!(
        "{LOOPBACK_WITHOUT_PROGRAMS}\n[keep_awake]\ninhibit = \"/fake/inhibit\"\n\
         sleep = \"/fake/sleep\"\n[notify]\nnotify_send = \"/fake/notify-send\"\n\
         [launcher]\nargv = [\"/fake/fuzzel\"]"
    );
    let config = Config::from_toml(&toml).expect("config");
    let runner = FakeRunner::default();
    let keyboard = FakeKeyboard::default();
    let host = Host {
        runner: Arc::new(runner.clone()),
        proc_root: dirs.data_dir(),
        keyboard: KeyboardHost::Sink(Arc::new(keyboard.clone())),
        ..Host::new(SwayTarget::Explicit(dirs.sway_socket()))
    };
    let daemon = Daemon::start_on(config, paths, host)
        .await
        .expect("daemon starts");
    FakeDaemon {
        daemon,
        runner,
        keyboard,
        _sway: sway,
        _dirs: dirs,
    }
}

#[tokio::test]
async fn the_panic_switch_ends_keep_awake_input_and_the_launcher_for_good() {
    let FakeDaemon {
        daemon,
        runner,
        keyboard,
        ..
    } = fake_daemon().await;
    let device = DeviceId::parse("AAAAAAAAAAAAAAAAAAAAAA").expect("device id");
    let on = KeepAwakeArgs::On { duration_s: None };
    let mut view = daemon.keep_awake.view();
    let mut launcher = daemon.services.launcher.view();

    daemon.keep_awake.set(&device, on).await.expect("on");
    daemon
        .services
        .launcher
        .open(&device, Some(6))
        .expect("launcher started");
    daemon.enter_read_only();
    assert_eq!(
        keyboard.log(),
        [Recorded::ReleaseAll],
        "every key released at once"
    );
    view.wait_for(|view| view.hold.is_none())
        .await
        .expect("released");
    launcher
        .wait_for(Option::is_none)
        .await
        .expect("launcher closed");
    assert!(runner.was_killed(child_index(&runner, "/fake/inhibit", 0)));
    assert!(runner.was_killed(child_index(&runner, "/fake/fuzzel", 0)));

    // A request that passed the read-only check before the switch starts nothing after it.
    let refused = daemon.keep_awake.set(&device, on).await;
    assert!(
        matches!(refused, Err(KeepAwakeError::ReadOnly)),
        "{refused:?}"
    );
    let reopened = daemon.services.launcher.open(&device, Some(6));
    assert!(matches!(reopened, Err(OpenError::Closed)), "{reopened:?}");
    let spawned = |program: &str| {
        runner
            .calls()
            .iter()
            .filter(|call| !matches!(call.kind, CallKind::Run(_)))
            .filter(|call| call.argv()[0] == program)
            .count()
    };
    assert_eq!((spawned("/fake/inhibit"), spawned("/fake/fuzzel")), (1, 1));
    daemon.stop().await;
}

#[tokio::test]
async fn stop_ends_keep_awake() {
    let FakeDaemon { daemon, runner, .. } = fake_daemon().await;
    let device = DeviceId::parse("AAAAAAAAAAAAAAAAAAAAAA").expect("device id");
    let on = KeepAwakeArgs::On { duration_s: None };
    daemon.keep_awake.set(&device, on).await.expect("on");
    daemon.stop().await;
    assert!(runner.was_killed(child_index(&runner, "/fake/inhibit", 0)));
}

/// The index `FakeRunner` gives the `nth` child started from `program`.
fn child_index(runner: &FakeRunner, program: &str, nth: usize) -> usize {
    let started: Vec<String> = runner
        .calls()
        .iter()
        .filter(|call| !matches!(call.kind, CallKind::Run(_)))
        .map(|call| call.argv()[0].clone())
        .collect();
    let mut indexes = started
        .iter()
        .enumerate()
        .filter(|(_, started)| started.as_str() == program);
    indexes
        .nth(nth)
        .map(|(index, _)| index)
        .expect("child started")
}

#[tokio::test]
async fn an_address_in_use_is_a_runtime_failure_not_a_config_error() {
    let dirs = TestDirs::new();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = taken.local_addr().expect("address");
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let config = Config::from_toml(&format!("listen = [\"{address}\"]")).expect("config");
    let target = SwayTarget::Explicit(dirs.sway_socket());

    let Err(error) = Daemon::start(config, paths.clone(), target).await else {
        panic!("the bind must fail");
    };
    assert!(matches!(error, DaemonError::Bind { .. }));
    assert_eq!(error.exit_code(), 1);
    assert!(
        !paths.admin_socket().exists(),
        "the failed start left an admin socket that nobody serves"
    );
}

#[tokio::test]
async fn a_second_daemon_is_a_config_error_even_when_its_address_is_in_use() {
    let dirs = TestDirs::new();
    let _sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let config = Config::from_toml(LOOPBACK_WITHOUT_PROGRAMS).expect("config");
    let target = SwayTarget::Explicit(dirs.sway_socket());
    let first = Daemon::start(config, paths.clone(), target.clone())
        .await
        .expect("the first daemon starts");
    let [address] = first.bound() else {
        panic!("one listen address, got {:?}", first.bound());
    };
    let same_address = Config::from_toml(&format!("listen = [\"{address}\"]")).expect("config");

    let Err(error) = Daemon::start(same_address, paths.clone(), target).await else {
        panic!("the second start must fail");
    };

    assert!(matches!(
        error,
        DaemonError::Start(StartError::AlreadyRunning(_))
    ));
    assert_eq!(error.exit_code(), 2);
    assert!(
        paths.admin_socket().exists(),
        "the refused start removed the socket of the running daemon"
    );
    first.stop().await;
}
