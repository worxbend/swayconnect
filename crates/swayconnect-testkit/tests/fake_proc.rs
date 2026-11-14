//! `fake-proc` run as a child the way the daemon runs programs, the config builder, and
//! the guard against system programs.

use std::io::{BufRead, BufReader};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Output, Stdio};
use std::time::Duration;

use swayconnect_testkit::{
    FakeProcMode, FakeProcs, ProgramSection, TestConfig, TestDirs, assert_no_system_programs,
};

const FAULT: i32 = 125;
const SIGKILL: i32 = 9;

fn install(dirs: &TestDirs) -> FakeProcs {
    FakeProcs::install(&dirs.root().join("bin"))
}

fn run(dirs: &TestDirs, programs: &FakeProcs, name: &str, arguments: &[&str]) -> Output {
    dirs.command(programs.program(name))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("start fake program")
}

#[test]
fn a_call_logs_its_arguments_and_exits_0_by_default() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    let tricky = "quote \" backslash \\ newline \n tab \t DEL \u{7f} Ж 😀";

    let first = run(
        &dirs,
        &programs,
        "pactl",
        &["set-sink-mute", "--", "--server=x", "1"],
    );
    let second = run(&dirs, &programs, "pactl", &[tricky, ""]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert!(first.stdout.is_empty());
    assert_eq!(
        programs.calls("pactl"),
        [
            vec!["set-sink-mute", "--", "--server=x", "1"],
            vec![tricky, ""]
        ]
    );
    assert!(programs.calls("notify-send").is_empty());
}

#[test]
fn the_exit_mode_sets_the_exit_code() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    programs.set_mode("lock.sh", FakeProcMode::Exit(3));

    let output = run(&dirs, &programs, "lock.sh", &[]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(programs.calls("lock.sh"), [Vec::<String>::new()]);
}

#[test]
fn the_sleep_mode_exits_0_after_the_sleep() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    programs.set_mode("sleep", FakeProcMode::Sleep(Duration::from_millis(10)));

    let output = run(&dirs, &programs, "sleep", &["7200"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(programs.calls("sleep"), [vec!["7200"]]);
}

#[test]
fn scripted_stdout_is_written() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    programs.set_stdout("pactl", b"[{\"name\":\"sink\"}]\n");

    let output = run(&dirs, &programs, "pactl", &["-f", "json", "list", "sinks"]);

    assert_eq!(output.stdout, b"[{\"name\":\"sink\"}]\n");
}

#[test]
fn the_hang_mode_runs_until_killed() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    programs.set_mode("systemd-inhibit", FakeProcMode::Hang);
    // The stdout line is the ready signal: it is written after the call is logged.
    programs.set_stdout("systemd-inhibit", b"ready\n");
    let mut child = dirs
        .command(programs.program("systemd-inhibit"))
        .args(["--what=idle", "--mode=block"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start fake program");
    let mut ready = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut ready)
        .expect("read ready line");

    child.kill().expect("kill");
    let status = child.wait().expect("wait");

    assert_eq!(ready, "ready\n");
    // A fake that had already exited would report its exit code, not the signal.
    assert_eq!(status.signal(), Some(SIGKILL));
    assert_eq!(
        programs.calls("systemd-inhibit"),
        [vec!["--what=idle", "--mode=block"]]
    );
}

#[test]
fn a_malformed_mode_file_is_a_fault_not_a_scripted_result() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    let mode_file = format!("{}.mode", programs.program("fuzzel").display());
    std::fs::write(mode_file, "exit zero").expect("write mode file");

    let output = run(&dirs, &programs, "fuzzel", &[]);

    assert_eq!(output.status.code(), Some(FAULT));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown mode"));
}

#[test]
fn a_relative_argv0_is_a_fault() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);

    let output = dirs
        .command(programs.program("fuzzel"))
        .arg0("fuzzel")
        .stdin(Stdio::null())
        .output()
        .expect("start fake program");

    assert_eq!(output.status.code(), Some(FAULT));
    assert!(programs.calls("fuzzel").is_empty());
}

#[test]
#[should_panic(expected = "not a fake program")]
fn an_unknown_program_name_panics() {
    let dirs = TestDirs::new();

    install(&dirs).program("bash");
}

#[test]
fn the_built_config_points_every_program_key_at_a_fake() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);
    let fake = |name: &str| format!("\"{}\"", programs.program(name).display());

    let config = TestConfig::new(&programs)
        .preamble("read_only = false")
        .line(ProgramSection::KeepAwake, "cooldown_s = 60")
        .build();

    let expected = [
        "read_only = false\n".to_owned(),
        format!("[session]\nlock = [{}]\n", fake("lock.sh")),
        format!(
            "[keep_awake]\ninhibit = {}\nsleep = {}\ncooldown_s = 60\n",
            fake("systemd-inhibit"),
            fake("sleep")
        ),
        format!("[notify]\nnotify_send = {}\n", fake("notify-send")),
        format!("[audio]\npactl = {}\n", fake("pactl")),
        format!(
            "[apps]\nsystemd_run = {}\nterminal = [{}, \"-e\"]\n",
            fake("systemd-run"),
            fake("alacritty")
        ),
        format!("[launcher]\nargv = [{}]\n", fake("fuzzel")),
    ]
    .join("\n");
    assert_eq!(config, expected);
    assert!(programs.dir().starts_with(dirs.root()));
}

#[test]
fn a_config_can_name_only_the_sections_a_daemon_knows() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);

    let config = TestConfig::with_sections(
        &programs,
        &[ProgramSection::KeepAwake, ProgramSection::Notify],
    )
    .build();

    assert!(config.contains("[keep_awake]") && config.contains("[notify]"));
    for absent in ["[session]", "[audio]", "[apps]", "[launcher]"] {
        assert!(!config.contains(absent), "{absent}");
    }
}

#[test]
#[should_panic(expected = "was not chosen")]
fn a_line_for_a_section_not_chosen_panics() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);

    let _ = TestConfig::with_sections(&programs, &[ProgramSection::Notify])
        .line(ProgramSection::Audio, "max_volume_pct = 100");
}

#[test]
#[should_panic(expected = "names a program under /usr/")]
fn the_guard_fails_a_built_config_that_names_a_program_under_usr() {
    let dirs = TestDirs::new();
    let programs = install(&dirs);

    TestConfig::with_sections(&programs, &[ProgramSection::Session])
        .line(
            ProgramSection::Session,
            "lock_processes = [\"/usr/bin/swaylock\"]",
        )
        .build();
}

#[test]
fn the_guard_refuses_each_system_directory_in_either_quote() {
    let configs = [
        "[keep_awake]\ninhibit = \"/usr/bin/systemd-inhibit\"",
        "[notify]\nnotify_send = '/usr/bin/notify-send'",
        "[launcher]\nargv = [\"/bin/fuzzel\"]",
        "[audio]\npactl = \"/sbin/pactl\"",
    ];
    for config in configs {
        let refused = std::panic::catch_unwind(|| assert_no_system_programs(config));
        assert!(refused.is_err(), "{config}");
    }
    assert_no_system_programs("[session]\nlock = [\"/tmp/x/lock.sh\"]\nname = \"usr\"");
}
