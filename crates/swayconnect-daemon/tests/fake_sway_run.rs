//! The fake-sway daemon run of the emulator checks (`docs/V2_PLAN.md`, M2 status): the real
//! `swayconnectd` binary on the fake sway, with the desktop-entry fixture as
//! `XDG_DATA_DIRS`. Nothing else covers `main.rs` reading `XDG_DATA_DIRS` into topic `apps`.
//!
//! The M2 checkpoint run listed no apps: it named the fixture root, which holds no
//! `applications/`. The fixture is two data dirs, `high` before `low` (its README).

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead as _, BufReader};
use std::process::{Child, Stdio};
use std::time::Duration;

use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::paths::Paths;
use swayconnect_protocol::ops::TOPIC_APPS;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{Apps, Scope};
use swayconnect_testkit::{FakeSway, TestDirs, nonexistent_programs};

use crate::support::{Pairing, soon};

const DAEMON: &str = env!("CARGO_BIN_EXE_swayconnectd");
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/desktop-entries"
);
const PAIRING_WINDOW: Duration = Duration::from_secs(300);

/// Kills and reaps the daemon when the test ends, also on a panic.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        // Already gone is fine; the wait reaps it either way.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts `swayconnectd run` (`read_only = false`, every program missing) and waits for
/// its `listening` line. Its stderr is drained to the end, so logging never blocks it.
async fn start_daemon(dirs: &TestDirs, sway: &FakeSway, xdg_data_dirs: &str) -> Running {
    let config = format!(
        "listen = [\"127.0.0.1:0\"]\nread_only = false\n[sway]\nsocket = \"{}\"\n{}",
        sway.socket_path().display(),
        nonexistent_programs()
    );
    fs::write(dirs.config_file(), config).expect("config file");
    let mut child = dirs
        .command(DAEMON)
        .arg("run")
        .env("XDG_DATA_DIRS", xdg_data_dirs)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon starts");
    let stderr = child.stderr.take().expect("stderr pipe");
    let daemon = Running(child);
    let (listening, ready) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut listening = Some(listening);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(sender) = listening.take_if(|_| line.contains("listening")) {
                // The test may have given up waiting already.
                let _ = sender.send(());
            }
        }
    });
    soon("listening", ready).await.expect("listening line");
    daemon
}

/// The entry ids topic `apps` lists for a device of a daemon started with `xdg_data_dirs`.
async fn listed_apps(xdg_data_dirs: &str) -> Vec<String> {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &support::real_sway_sockets());
    let _daemon = start_daemon(&dirs, &sway, xdg_data_dirs).await;
    let paths = Paths {
        config_file: dirs.config_file(),
        data_dir: dirs.data_dir(),
        runtime_dir: dirs.runtime_dir(),
    };
    let mut admin = soon("admin", AdminClient::connect(&paths))
        .await
        .expect("admin socket");
    let request = PairingRequest {
        scopes: BTreeSet::from([Scope::StateRead, Scope::Launch]),
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
    let mut session = soon("auth", support::connector().connect(&profile))
        .await
        .expect("authenticated");
    let states = soon("subscribe", session.subscribe(&[TOPIC_APPS]))
        .await
        .expect("subscribed");
    let [first] = states.try_into().expect("one state");
    let apps = Apps::decode(&first.data).expect("apps state").apps;
    apps.into_iter().map(|app| app.entry_id).collect()
}

#[tokio::test]
async fn the_fixture_data_dirs_list_their_apps_and_the_fixture_root_lists_none() {
    let data_dirs = format!("{FIXTURE}/high:{FIXTURE}/low");
    let listed = [
        "denied",
        "firefox",
        "gparted",
        "htop",
        "mpv",
        "only-sway",
        "quoted",
        "sub-tool",
    ];
    assert_eq!(listed_apps(&data_dirs).await, listed);
    assert_eq!(
        listed_apps(FIXTURE).await,
        Vec::<String>::new(),
        "the root is not a data dir: no `applications/` in it"
    );
}
