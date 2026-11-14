use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::proc::fake::FakeRunner;

use super::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/desktop-entries"
);

#[derive(Default)]
struct FakeIcons {
    hashes: Mutex<HashMap<String, IconHash>>,
    asked: Mutex<Vec<String>>,
}

impl FakeIcons {
    fn set(&self, entry_id: &str, hash: IconHash) {
        lock(&self.hashes).insert(entry_id.to_owned(), hash);
    }
}

impl AppIcons for FakeIcons {
    fn entry_icon(&self, entry_id: &str) -> Option<IconHash> {
        lock(&self.asked).push(entry_id.to_owned());
        lock(&self.hashes).get(entry_id).copied()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fixture_dirs() -> IndexDirs {
    IndexDirs {
        data_dirs: vec![
            Path::new(FIXTURES).join("high"),
            Path::new(FIXTURES).join("low"),
        ],
        exec_path: Vec::new(),
    }
}

struct Rig {
    service: AppsService,
    runner: FakeRunner,
    icons: Arc<FakeIcons>,
}

fn rig_with(dirs: IndexDirs) -> Rig {
    let runner = FakeRunner::default();
    let icons = Arc::new(FakeIcons::default());
    let config = AppsConfig {
        deny: vec!["denied".to_owned()],
        ..AppsConfig::default()
    };
    let service = AppsService::new(config, dirs, Arc::new(runner.clone()), icons.clone());
    Rig {
        service,
        runner,
        icons,
    }
}

/// `count` entries `app000`, `app001`, ... named `App 000`, ... under `data_dir`.
fn write_entries(data_dir: &Path, count: usize) {
    let applications = data_dir.join("applications");
    fs::create_dir_all(&applications).expect("dir");
    for i in 0..count {
        let text = format!("[Desktop Entry]\nType=Application\nName=App {i:03}\nExec=app\n");
        fs::write(applications.join(format!("app{i:03}.desktop")), text).expect("write");
    }
}

fn temp_dirs(data_dir: &Path) -> IndexDirs {
    IndexDirs {
        data_dirs: vec![data_dir.to_owned()],
        exec_path: Vec::new(),
    }
}

fn rig() -> Rig {
    rig_with(fixture_dirs())
}

fn entry(id: &str) -> EntryId {
    EntryId::parse(id).expect("entry id")
}

fn published(rig: &Rig) -> Arc<PublishedApps> {
    rig.service.subscribe().borrow().clone()
}

fn listed_ids(rig: &Rig) -> Vec<String> {
    published(rig)
        .state
        .apps
        .iter()
        .map(|app| app.entry_id.clone())
        .collect()
}

async fn launch(rig: &Rig, id: &str) -> Result<(), LaunchError> {
    let prepared = rig.service.prepare_launch(&entry(id), true)?;
    rig.service.launch(prepared).await
}

#[test]
fn the_first_state_is_rev_1_and_only_entries_with_an_icon_ask_for_one() {
    let rig = rig();
    let first = published(&rig);
    assert_eq!(first.rev, 1);
    assert_eq!(
        listed_ids(&rig),
        [
            "firefox",
            "gparted",
            "htop",
            "mpv",
            "only-sway",
            "quoted",
            "sub-tool"
        ]
    );
    assert_eq!(*lock(&rig.icons.asked), ["firefox", "gparted", "mpv"]);
    assert!(first.state.recent.is_empty() && !first.state.truncated);
}

#[test]
fn only_an_exact_id_of_the_state_passes_and_unknown_comes_before_risky() {
    let rig = rig();
    for unknown in [
        "chromium",
        "Firefox",
        "firefox.desktop",
        "xterm",
        "denied",
        "nodisplay",
    ] {
        let refused = rig.service.prepare_launch(&entry(unknown), false);
        assert!(matches!(refused, Err(LaunchError::NotFound)), "{unknown}");
    }
    assert!(rig.service.prepare_launch(&entry("firefox"), false).is_ok());
}

#[test]
fn a_risky_entry_needs_launch_risky() {
    let rig = rig();
    for risky in ["htop", "gparted"] {
        let refused = rig
            .service
            .prepare_launch(&entry(risky), false)
            .expect_err("risky");
        assert_eq!(refused.code(), ErrorCode::Forbidden);
        assert_eq!(refused.missing_scope(), Some(Scope::LaunchRisky));
        assert!(rig.service.prepare_launch(&entry(risky), true).is_ok());
    }
    assert_eq!(LaunchError::NotFound.missing_scope(), None);
    assert_eq!(rig.runner.spawned_count(), 0);
}

#[test]
fn a_terminal_emulator_needs_launch_risky() {
    let dir = tempfile::tempdir().expect("temp dir");
    let applications = dir.path().join("applications");
    fs::create_dir_all(&applications).expect("dir");
    let text = "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot\n\
                Terminal=false\nCategories=System;TerminalEmulator;\n";
    fs::write(applications.join("foot.desktop"), text).expect("write");
    let rig = rig_with(temp_dirs(dir.path()));
    let refused = rig
        .service
        .prepare_launch(&entry("foot"), false)
        .expect_err("risky");
    assert!(matches!(refused, LaunchError::NeedsLaunchRisky));
    assert!(rig.service.prepare_launch(&entry("foot"), true).is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_launch_starts_the_index_argv_in_its_own_scope_and_becomes_recent() {
    let rig = rig();
    launch(&rig, "firefox").await.expect("launched");
    let argv = &rig.runner.argvs()[0];
    assert_eq!(
        argv[..5],
        [
            "/usr/bin/systemd-run",
            "--user",
            "--scope",
            "--collect",
            argv[4].as_str()
        ]
    );
    let suffix = argv[4]
        .strip_prefix("--unit=app-swayconnect-firefox-")
        .expect("unit name");
    assert!(suffix.len() == 8 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(argv[5..], ["--", "/usr/lib/firefox/firefox"]);
    let after = published(&rig);
    assert_eq!(
        (after.rev, after.state.recent.clone()),
        (2, vec!["firefox".to_owned()])
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_start_is_spawn_failed_and_changes_nothing() {
    let rig = rig();
    rig.runner.fail_next_spawn();
    let failed = launch(&rig, "firefox").await.expect_err("spawn fails");
    assert_eq!(failed.code(), ErrorCode::SpawnFailed);
    let after = published(&rig);
    assert_eq!(after.rev, 1);
    assert!(after.state.recent.is_empty());
}

#[tokio::test(start_paused = true)]
async fn recent_keeps_8_ids_most_recent_first_without_repeats() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_entries(dir.path(), 10);
    let rig = rig_with(temp_dirs(dir.path()));
    for i in (0..10).chain([3]) {
        launch(&rig, &format!("app{i:03}")).await.expect("launched");
    }
    assert_eq!(
        published(&rig).state.recent,
        [
            "app003", "app009", "app008", "app007", "app006", "app005", "app004", "app002"
        ]
    );
}

#[test]
fn icons_resolved_later_appear_on_refresh() {
    let rig = rig();
    let hash = IconHash::of(b"firefox png");
    rig.service.refresh_icons();
    assert_eq!(published(&rig).rev, 1, "nothing changed");
    rig.icons.set("firefox", hash);
    rig.service.refresh_icons();
    let after = published(&rig);
    assert_eq!(after.rev, 2);
    assert_eq!(after.state.apps[0].icon, Some(hash));
}

#[test]
fn a_rescan_lists_a_new_entry() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_entries(dir.path(), 0);
    let rig = rig_with(temp_dirs(dir.path()));
    assert!(listed_ids(&rig).is_empty());
    write_entries(dir.path(), 1);
    rig.service.rescan();
    assert_eq!(
        (published(&rig).rev, listed_ids(&rig)),
        (2, vec!["app000".to_owned()])
    );
}

#[test]
fn an_entry_cut_from_the_state_cannot_be_launched() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_entries(dir.path(), 513);
    let rig = rig_with(temp_dirs(dir.path()));
    assert!(published(&rig).state.truncated);
    assert!(rig.service.prepare_launch(&entry("app511"), false).is_ok());
    let cut = rig.service.prepare_launch(&entry("app512"), false);
    assert!(matches!(cut, Err(LaunchError::NotFound)));
}
