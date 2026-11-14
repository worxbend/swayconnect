use std::path::PathBuf;

use swayconnect_testkit::{FakeProcMode, FakeProcs};

use super::*;
use crate::apps::LauncherConfig;
use crate::proc::fake::{CallKind, FakeRunner};
use crate::proc::{ChildEnv, ProgramPath, RealRunner};

fn device(text: &str) -> DeviceId {
    DeviceId::parse(text).expect("device id")
}

fn mine() -> DeviceId {
    device("AAAAAAAAAAAAAAAAAAAAAA")
}

fn other() -> DeviceId {
    device("AQEBAQEBAQEBAQEBAQEBAQ")
}

fn app(id: &str, terminal: bool, argv: &[&str]) -> IndexedApp {
    IndexedApp {
        entry_id: id.to_owned(),
        name: id.to_owned(),
        generic_name: None,
        keywords: Vec::new(),
        has_icon: false,
        terminal,
        risky: terminal,
        argv: argv.iter().map(|word| (*word).to_owned()).collect(),
    }
}

fn argv_of(command: &ChildCommand) -> Vec<String> {
    std::iter::once(command.program().as_os_str())
        .chain(command.args().iter().map(|word| word.as_os_str()))
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

fn fake_config(programs: &FakeProcs) -> AppsConfig {
    let path = |name: &str| ProgramPath::new(programs.program(name)).expect("absolute");
    let terminal = vec![
        programs.program("alacritty").display().to_string(),
        "-e".to_owned(),
    ];
    AppsConfig {
        systemd_run: path("systemd-run"),
        terminal: ProgramArgv::try_from(terminal).expect("argv"),
        ..AppsConfig::default()
    }
}

#[test]
fn the_systemd_run_argv_is_exact() {
    let config = AppsConfig::default();
    let firefox = app("firefox", false, &["/usr/lib/firefox/firefox"]);
    assert_eq!(
        argv_of(&launch_command(&config, &firefox, "0a1b2c3d")),
        [
            "/usr/bin/systemd-run",
            "--user",
            "--scope",
            "--collect",
            "--unit=app-swayconnect-firefox-0a1b2c3d",
            "--",
            "/usr/lib/firefox/firefox",
        ]
    );
}

#[test]
fn a_terminal_entry_is_wrapped_and_a_dash_word_stays_after_the_separator() {
    let config = AppsConfig::default();
    let htop = app("htop", true, &["htop", "--tree"]);
    assert_eq!(
        argv_of(&launch_command(&config, &htop, "ffffffff")),
        [
            "/usr/bin/systemd-run",
            "--user",
            "--scope",
            "--collect",
            "--unit=app-swayconnect-htop-ffffffff",
            "--",
            "/usr/bin/alacritty",
            "-e",
            "htop",
            "--tree",
        ]
    );
    let dashed = app("x", false, &["--setenv=A=b"]);
    let argv = argv_of(&launch_command(&config, &dashed, "00000000"));
    assert_eq!(argv[5..], ["--", "--setenv=A=b"]);
}

#[test]
fn a_long_entry_id_is_cut_in_the_unit_name() {
    let long = "a".repeat(255);
    let argv = argv_of(&launch_command(
        &AppsConfig::default(),
        &app(&long, false, &["a"]),
        "12345678",
    ));
    assert_eq!(
        argv[4],
        format!("--unit=app-swayconnect-{}-12345678", "a".repeat(64))
    );
}

#[tokio::test(start_paused = true)]
async fn a_child_still_running_after_the_check_counts_as_started() {
    let runner = FakeRunner::default();
    let command = launch_command(&AppsConfig::default(), &app("a", false, &["a"]), "0");
    start(&runner, command).await.expect("started");
    assert_eq!(runner.calls()[0].kind, CallKind::Spawn);
    assert!(!runner.was_killed(0), "a started app is never killed");
}

#[tokio::test(start_paused = true)]
async fn an_early_failure_is_reported_and_a_clean_exit_is_not() {
    for (exit, fails) in [(Exit::Code(1), true), (Exit::Code(0), false)] {
        let runner = FakeRunner::default();
        let command = launch_command(&AppsConfig::default(), &app("a", false, &["a"]), "0");
        let task_runner = runner.clone();
        let started = tokio::spawn(async move { start(&task_runner, command).await });
        while runner.spawned_count() == 0 {
            tokio::task::yield_now().await;
        }
        runner.end_child(0, exit);
        let outcome = started.await.expect("task");
        assert_eq!(
            matches!(outcome, Err(StartError::Exited(code)) if code == exit),
            fails
        );
    }
    let runner = FakeRunner::default();
    runner.fail_next_spawn();
    let command = launch_command(&AppsConfig::default(), &app("a", false, &["a"]), "0");
    assert!(matches!(
        start(&runner, command).await,
        Err(StartError::Proc(ProcError::Spawn { .. }))
    ));
}

#[tokio::test]
async fn fake_proc_receives_the_argv_and_its_exit_code_counts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let programs = FakeProcs::install(&dir.path().join("bin"));
    let config = fake_config(&programs);
    let runner = RealRunner::new(ChildEnv::default());
    let htop = app("htop", true, &["htop"]);

    start(&runner, launch_command(&config, &htop, "abcdef01"))
        .await
        .expect("started");
    let terminal = programs.program("alacritty").display().to_string();
    assert_eq!(
        programs.calls("systemd-run"),
        [vec![
            "--user".to_owned(),
            "--scope".to_owned(),
            "--collect".to_owned(),
            "--unit=app-swayconnect-htop-abcdef01".to_owned(),
            "--".to_owned(),
            terminal,
            "-e".to_owned(),
            "htop".to_owned(),
        ]]
    );

    programs.set_mode("systemd-run", FakeProcMode::Exit(1));
    let failed = start(&runner, launch_command(&config, &htop, "abcdef02")).await;
    assert!(matches!(failed, Err(StartError::Exited(Exit::Code(1)))));
}

#[tokio::test(start_paused = true)]
async fn one_launcher_runs_and_its_opener_holds_it() {
    let runner = FakeRunner::default();
    let argv = LauncherConfig::default().argv;
    let launcher = Launcher::new(argv, Arc::new(runner.clone()));
    let mut view = launcher.view();

    let first = launcher.open(&mine(), Some(42)).expect("open");
    assert!(!first.already_open);
    let open = view.borrow_and_update().clone().expect("open launcher");
    assert_eq!(open.holder, mine());
    assert_eq!(open.focus_window_id, Some(42));
    assert_eq!(
        held_for(Some(&open), &mine()).holder,
        Some(Holder::ThisDevice)
    );
    assert_eq!(
        held_for(Some(&open), &other()).holder,
        Some(Holder::OtherDevice)
    );

    let second = launcher.open(&other(), Some(7)).expect("open again");
    assert!(second.already_open);
    assert_eq!(runner.spawned_count(), 1);
    assert_eq!(runner.argvs(), [vec!["/usr/bin/fuzzel".to_owned()]]);
    assert_eq!(
        view.borrow().as_ref().map(|o| o.holder.clone()),
        Some(mine())
    );

    runner.end_child(0, Exit::Code(0));
    view.wait_for(Option::is_none).await.expect("closed");
    assert_eq!(
        held_for(None, &mine()),
        Held {
            running: false,
            holder: None
        }
    );
    let reopened = launcher.open(&other(), None).expect("reopen");
    assert!(!reopened.already_open);
    assert_eq!(
        view.borrow().as_ref().map(|o| o.holder.clone()),
        Some(other())
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_spawn_leaves_no_holder() {
    let runner = FakeRunner::default();
    runner.fail_next_spawn();
    let launcher = Launcher::new(LauncherConfig::default().argv, Arc::new(runner.clone()));
    assert!(matches!(
        launcher.open(&mine(), None),
        Err(OpenError::Spawn(ProcError::Spawn { .. }))
    ));
    assert!(launcher.view().borrow().is_none());
}

#[tokio::test(start_paused = true)]
async fn close_and_drop_kill_the_launcher() {
    let runner = FakeRunner::default();
    let launcher = Launcher::new(LauncherConfig::default().argv, Arc::new(runner.clone()));
    launcher.open(&mine(), None).expect("open");
    launcher.close().await;
    assert!(runner.was_killed(0));
    assert!(launcher.view().borrow().is_none());

    let launcher = Launcher::new(LauncherConfig::default().argv, Arc::new(runner.clone()));
    launcher.open(&mine(), None).expect("open");
    let mut view = launcher.view();
    drop(launcher);
    // The watch task ends the child when the launcher goes away.
    let _ = view.wait_for(Option::is_none).await;
    assert!(runner.was_killed(1));
}

/// The panic switch: a `launcher.open` that passed the read-only check before it, and
/// reaches the spawn after it, starts nothing; also when no launcher ran at the switch.
#[tokio::test(start_paused = true)]
async fn after_close_no_launcher_opens() {
    let runner = FakeRunner::default();
    let launcher = Launcher::new(LauncherConfig::default().argv, Arc::new(runner.clone()));
    launcher.close().await;
    assert!(matches!(
        launcher.open(&mine(), Some(1)),
        Err(OpenError::Closed)
    ));
    assert_eq!(runner.spawned_count(), 0);
    assert!(launcher.view().borrow().is_none());
}

#[tokio::test]
async fn a_real_launcher_child_is_killed_by_close() {
    let dir = tempfile::tempdir().expect("temp dir");
    let programs = FakeProcs::install(&dir.path().join("bin"));
    programs.set_mode("fuzzel", FakeProcMode::Hang);
    let fuzzel: PathBuf = programs.program("fuzzel");
    let argv = ProgramArgv::try_from(vec![fuzzel.display().to_string(), "--dmenu".to_owned()])
        .expect("argv");
    let launcher = Launcher::new(argv, Arc::new(RealRunner::new(ChildEnv::default())));
    launcher.open(&mine(), Some(1)).expect("open");
    let pid = launcher.view().borrow().as_ref().expect("open").pid;
    assert!(PathBuf::from(format!("/proc/{pid}")).exists());
    launcher.close().await;
    assert!(launcher.view().borrow().is_none());
    assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
}
