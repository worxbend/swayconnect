//! `RealRunner` against the testkit's `fake-proc`; no real system program is started.

use std::ffi::OsString;
use std::time::{Duration, Instant};

use swayconnect_testkit::{FakeProcMode, FakeProcs};

use super::fake::{CallKind, FakeRunner};
use super::*;

const TIMEOUT: Duration = Duration::from_secs(10);

struct Setup {
    _dir: tempfile::TempDir,
    programs: FakeProcs,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().expect("temp dir");
    let programs = FakeProcs::install(&dir.path().join("bin"));
    Setup {
        _dir: dir,
        programs,
    }
}

fn program(setup: &Setup, name: &str) -> ProgramPath {
    ProgramPath::new(setup.programs.program(name)).expect("absolute")
}

fn env(pairs: &[(&str, &str)]) -> ChildEnv {
    ChildEnv::allowlisted(
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    )
}

#[tokio::test]
async fn run_passes_argv_exactly_and_reports_the_exit_code() {
    let setup = setup();
    setup.programs.set_mode("pactl", FakeProcMode::Exit(3));
    let command = ChildCommand::new(&program(&setup, "pactl"))
        .arg("set-sink-mute")
        .arg("--")
        .arg("--server=x")
        .arg("1");

    let finished = RealRunner::new(ChildEnv::default())
        .run(command, TIMEOUT)
        .await
        .expect("run");

    assert_eq!(finished.exit, Exit::Code(3));
    assert!(!finished.exit.success());
    assert_eq!(
        setup.programs.calls("pactl"),
        vec![vec!["set-sink-mute", "--", "--server=x", "1"]]
    );
}

#[tokio::test]
async fn run_captures_stdout_up_to_the_cap() {
    let setup = setup();
    setup.programs.set_stdout("pactl", b"[]\n");
    let small = RealRunner::new(ChildEnv::default())
        .run(ChildCommand::new(&program(&setup, "pactl")), TIMEOUT)
        .await
        .expect("run");
    assert_eq!(small.stdout.bytes, b"[]\n");
    assert!(!small.stdout.truncated);
    assert!(small.exit.success());

    setup
        .programs
        .set_stdout("pactl", &vec![b'x'; MAX_OUTPUT_BYTES + 10]);
    let large = RealRunner::new(ChildEnv::default())
        .run(ChildCommand::new(&program(&setup, "pactl")), TIMEOUT)
        .await
        .expect("run");
    assert_eq!(large.stdout.bytes.len(), MAX_OUTPUT_BYTES);
    assert!(large.stdout.truncated);
    assert_eq!(
        large.exit,
        Exit::Code(0),
        "the child is not blocked by the cap"
    );
}

#[tokio::test]
async fn run_kills_a_child_that_outlives_the_timeout() {
    let setup = setup();
    setup.programs.set_mode("notify-send", FakeProcMode::Hang);
    let started = Instant::now();

    let result = RealRunner::new(ChildEnv::default())
        .run(
            ChildCommand::new(&program(&setup, "notify-send")),
            Duration::from_millis(200),
        )
        .await;

    assert!(matches!(result, Err(ProcError::TimedOut(_))), "{result:?}");
    assert!(started.elapsed() < TIMEOUT);
}

#[tokio::test]
async fn a_missing_program_is_a_spawn_error() {
    let setup = setup();
    let missing = ProgramPath::new(setup.programs.dir().join("absent")).expect("absolute");
    let runner = RealRunner::new(ChildEnv::default());

    let run = runner.run(ChildCommand::new(&missing), TIMEOUT).await;
    assert!(matches!(run, Err(ProcError::Spawn { .. })), "{run:?}");
    let spawn = runner.spawn(ChildCommand::new(&missing));
    assert!(matches!(spawn, Err(ProcError::Spawn { .. })));
}

#[tokio::test]
async fn a_spawned_child_ends_by_itself_or_by_kill() {
    let setup = setup();
    let runner = RealRunner::new(ChildEnv::default());
    setup
        .programs
        .set_mode("sleep", FakeProcMode::Sleep(Duration::from_millis(10)));
    let mut short = runner
        .spawn(ChildCommand::new(&program(&setup, "sleep")).arg("1"))
        .expect("spawn");
    assert_eq!(short.exited().await.expect("exit"), Exit::Code(0));
    short.kill().expect("a kill after the end does nothing");
    assert_eq!(short.try_exit().expect("try_exit"), Some(Exit::Code(0)));

    setup
        .programs
        .set_mode("systemd-inhibit", FakeProcMode::Hang);
    let mut held = runner
        .spawn(ChildCommand::new(&program(&setup, "systemd-inhibit")))
        .expect("spawn");
    assert_eq!(held.try_exit().expect("try_exit"), None);
    held.kill().expect("kill");
    assert_eq!(held.exited().await.expect("exit"), Exit::Signal(9));
}

/// Waits until the fake program `name` has written its call line, which it does from its
/// own `main`. std's `spawn` returns when close-on-exec fires, before the kernel has set up
/// the new program's environment, so `/proc/<pid>/environ` can still read empty then.
/// `<program>.log` is made a FIFO: the fake blocks on it until this reads the line.
fn gate_on_call(setup: &Setup, name: &str) -> std::sync::mpsc::Receiver<String> {
    let mut log = setup.programs.program(name).into_os_string();
    log.push(".log");
    let log = std::path::PathBuf::from(log);
    let mode = rustix::fs::Mode::from_raw_mode(0o600);
    rustix::fs::mknodat(rustix::fs::CWD, &log, rustix::fs::FileType::Fifo, mode, 0).expect("FIFO");
    let (sender, receiver) = std::sync::mpsc::channel();
    // A thread: a fake that never runs must not block the runtime of a failed test.
    std::thread::spawn(move || {
        let _ = sender.send(std::fs::read_to_string(&log).expect("read the FIFO"));
    });
    receiver
}

#[tokio::test]
async fn a_child_gets_only_allowlisted_variables_null_stdin_and_root_cwd() {
    let setup = setup();
    setup.programs.set_mode("fuzzel", FakeProcMode::Hang);
    let called = gate_on_call(&setup, "fuzzel");
    let runner = RealRunner::new(env(&[
        ("WAYLAND_DISPLAY", "wayland-test"),
        ("PATH", "/nonexistent"),
        ("LD_PRELOAD", "/tmp/evil.so"),
        ("SWAYCONNECT_SECRET", "x"),
    ]));
    let fuzzel = program(&setup, "fuzzel");
    let mut child = runner
        .spawn(ChildCommand::new(&fuzzel).arg("--dmenu"))
        .expect("spawn");
    let proc_dir = std::path::PathBuf::from(format!("/proc/{}", child.pid()));
    let call = called.recv_timeout(TIMEOUT).expect("the fake program runs");
    assert_eq!(call.trim_end(), r#"["--dmenu"]"#);

    let environ = std::fs::read(proc_dir.join("environ")).expect("environ");
    let cmdline = std::fs::read(proc_dir.join("cmdline")).expect("cmdline");
    let cwd = std::fs::read_link(proc_dir.join("cwd")).expect("cwd");
    let stdin = std::fs::read_link(proc_dir.join("fd/0")).expect("stdin");
    child.kill().expect("kill");
    child.exited().await.expect("exit");

    let mut variables: Vec<&[u8]> = environ.split(|byte| *byte == 0).collect();
    variables.sort_unstable();
    assert_eq!(
        variables,
        [
            &b""[..],
            b"PATH=/nonexistent",
            b"WAYLAND_DISPLAY=wayland-test"
        ],
        "std orders the variables itself"
    );
    let mut expected_cmdline = fuzzel.as_path().as_os_str().as_encoded_bytes().to_vec();
    expected_cmdline.extend_from_slice(b"\0--dmenu\0");
    assert_eq!(cmdline, expected_cmdline, "argv[0] is the configured path");
    assert_eq!(cwd, std::path::Path::new("/"));
    assert_eq!(stdin, std::path::Path::new("/dev/null"));
}

#[test]
fn the_allowlist_drops_every_other_variable() {
    let kept = env(&[
        ("HOME", "/home/owner"),
        ("LD_LIBRARY_PATH", "/tmp"),
        ("XDG_DATA_DIRS", "/usr/share"),
        ("home", "lower case is another name"),
    ]);
    let names: Vec<&OsString> = kept.vars().iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["HOME", "XDG_DATA_DIRS"]);
}

#[test]
fn config_programs_must_be_absolute() {
    #[derive(Debug, Deserialize)]
    struct Section {
        #[serde(default)]
        path: Option<ProgramPath>,
        #[serde(default)]
        argv: Option<ProgramArgv>,
    }
    let parse = |text: &str| toml::from_str::<Section>(text);

    assert!(parse(r#"path = "notify-send""#).is_err());
    assert!(parse(r#"path = """#).is_err());
    let path = parse(r#"path = "/fake/notify-send""#)
        .expect("valid")
        .path
        .expect("path");
    assert_eq!(path.as_path(), Path::new("/fake/notify-send"));
    assert!(parse(r#"argv = []"#).is_err());
    assert!(parse(r#"argv = ["lock.sh"]"#).is_err());
    let section = parse(r#"argv = ["/opt/alacritty", "-e"]"#).expect("valid");
    let argv = section.argv.expect("argv");
    assert_eq!(argv.program().as_path(), Path::new("/opt/alacritty"));
    assert_eq!(
        ChildCommand::from(&argv),
        ChildCommand::new(argv.program()).arg("-e")
    );
    assert_eq!(
        ProgramPath::new(PathBuf::from("bin/x")),
        Err(ProgramError::NotAbsolute(PathBuf::from("bin/x")))
    );
}

#[tokio::test(start_paused = true)]
async fn the_fake_runner_records_calls_and_scripts_children() {
    let runner = FakeRunner::default();
    let program = ProgramPath::new(PathBuf::from("/fake/systemd-inhibit")).expect("absolute");
    let finished = runner
        .run(ChildCommand::new(&program).arg("--list"), TIMEOUT)
        .await
        .expect("default reply");
    assert!(finished.exit.success());
    runner.reply_next_run(Err(ProcError::TimedOut(TIMEOUT)));
    assert!(
        runner
            .run(ChildCommand::new(&program), TIMEOUT)
            .await
            .is_err()
    );

    runner.fail_next_spawn();
    assert!(runner.spawn(ChildCommand::new(&program)).is_err());
    let mut first = runner.spawn(ChildCommand::new(&program)).expect("spawn");
    let mut second = runner.spawn(ChildCommand::new(&program)).expect("spawn");
    runner.end_child(0, Exit::Code(0));
    assert_eq!(first.exited().await.expect("exit"), Exit::Code(0));
    assert!(!runner.was_killed(0));
    assert_eq!(second.try_exit().expect("try_exit"), None);
    second.kill().expect("kill");
    assert!(runner.was_killed(1));
    assert_eq!(second.exited().await.expect("exit"), Exit::Signal(9));
    assert_ne!(first.pid(), second.pid());

    assert_eq!(runner.spawned_count(), 2);
    assert_eq!(runner.argvs()[0], ["/fake/systemd-inhibit", "--list"]);
    let kinds: Vec<CallKind> = runner.calls().iter().map(|call| call.kind).collect();
    assert_eq!(
        kinds,
        [
            CallKind::Run(TIMEOUT),
            CallKind::Run(TIMEOUT),
            CallKind::Spawn,
            CallKind::Spawn,
            CallKind::Spawn
        ]
    );
}

#[tokio::test]
async fn spawn_lines_yields_capped_lines_then_the_end() {
    let setup = setup();
    let mut stdout = b"Event 'change' on sink #1\n".to_vec();
    stdout.extend(vec![b'x'; MAX_LINE_BYTES + 10]);
    stdout.extend_from_slice(b"\nlast line without newline");
    setup.programs.set_stdout("pactl", &stdout);

    let LineChild {
        mut child,
        mut lines,
    } = RealRunner::new(ChildEnv::default())
        .spawn_lines(ChildCommand::new(&program(&setup, "pactl")).arg("subscribe"))
        .expect("spawn_lines");

    let mut seen = Vec::new();
    while let Some(line) = lines.next_line().await.expect("line") {
        seen.push(line);
    }
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen[0], "Event 'change' on sink #1");
    assert_eq!(seen[1], "x".repeat(MAX_LINE_BYTES));
    assert_eq!(seen[2], "last line without newline");
    assert_eq!(child.exited().await.expect("exit"), Exit::Code(0));
    assert_eq!(setup.programs.calls("pactl"), vec![vec!["subscribe"]]);
}

#[tokio::test]
async fn fake_line_children_stream_until_killed() {
    let runner = FakeRunner::default();
    let program = ProgramPath::new(PathBuf::from("/fake/pactl")).expect("absolute");
    let LineChild {
        mut child,
        mut lines,
    } = runner
        .spawn_lines(ChildCommand::new(&program).arg("subscribe"))
        .expect("spawn_lines");

    runner.send_line(0, "Event 'new' on sink #7");
    assert_eq!(
        lines.next_line().await.expect("line").as_deref(),
        Some("Event 'new' on sink #7")
    );
    child.kill().expect("kill");
    assert_eq!(lines.next_line().await.expect("end"), None);
    assert!(runner.was_killed(0));
    assert_eq!(runner.calls()[0].kind, CallKind::SpawnLines);
}
