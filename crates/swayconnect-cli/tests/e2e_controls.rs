//! The M2a subcommands of `swayconnect` (`docs/V2_PLAN.md` slice CLI.2a) against a
//! `swayconnectd` child process, the fake sway, and `fake-proc` for the lock command,
//! `systemd-inhibit`, `sleep` and `notify-send`.

mod e2e_support;

use std::fs;
use std::time::Duration;

use e2e_support::{EXIT_SERVER_ERROR, EXIT_USAGE_OR_PROFILE, Harness};
use swayconnect_testkit::FakeProcMode;

/// Long enough for the second `lock` to find the first lock command running. The fake ends
/// by itself, so none outlives a test that fails.
const LOCK_RUNS_FOR: Duration = Duration::from_secs(5);
/// The fake inhibitor holds until `awake off` kills it, or ends by itself after this.
const INHIBITOR_RUNS_FOR: Duration = Duration::from_secs(30);
const WITH_DESTRUCTIVE: [&str; 3] = ["pair", "--scopes", "state.read,control,destructive"];

fn harness_paired_through(owner_arguments: &[&str]) -> Harness {
    let harness = Harness::start();
    harness.pair_through(owner_arguments);
    harness.wait_for_the_first_snapshot();
    harness
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_float_and_close_send_their_sway_commands() {
    let harness = harness_paired_through(&WITH_DESTRUCTIVE);
    let info = harness.client_output(&["info"]);
    for op in ["window.move", "window.floating", "window.close"] {
        assert!(info.contains(op), "{op}: {info}");
    }

    let unconfirmed = harness.client_failure(&["close", "7"], EXIT_USAGE_OR_PROFILE);
    assert!(unconfirmed.contains("--yes"), "{unconfirmed}");
    assert_eq!(harness.client_output(&["move", "6", "3"]), "ok\n");
    assert_eq!(harness.client_output(&["move", "6", "--id", "5"]), "ok\n");
    assert_eq!(harness.client_output(&["float", "6", "on"]), "ok\n");
    assert_eq!(
        harness.client_output(&["--json", "float", "6", "off"]),
        "{}\n"
    );
    assert_eq!(harness.client_output(&["close", "7", "--yes"]), "ok\n");
    let sent = [
        "[con_id=6] move container to workspace number 3",
        "[con_id=6] move container to workspace number 2",
        "[con_id=6] floating enable",
        "[con_id=6] floating disable",
        "[con_id=7] kill",
    ];
    assert_eq!(harness.sway.commands(), sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_command_on_no_window_exits_1_and_close_needs_its_own_scope() {
    let harness = harness_paired_through(&["pair"]);

    // 999 is no id at all; 4 is a workspace.
    for arguments in [&["move", "999", "3"][..], &["float", "4", "on"]] {
        let errors = harness.client_failure(arguments, EXIT_SERVER_ERROR);
        let refusal = "swayconnect: the request failed: not_found: ";
        assert!(errors.starts_with(refusal), "{arguments:?}: {errors}");
    }
    // The default pairing has no `destructive` (owner, 2026-11-14).
    let errors = harness.client_failure(&["close", "6", "--yes"], EXIT_SERVER_ERROR);
    assert!(
        errors.contains("the server does not offer operation `window.close`"),
        "{errors}"
    );
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_starts_the_lock_command_once_while_it_runs() {
    let harness = harness_paired_through(&["pair"]);
    harness
        .programs
        .set_mode("lock.sh", FakeProcMode::Sleep(LOCK_RUNS_FOR));

    assert_eq!(harness.client_output(&["lock"]), "lock started\n");
    assert_eq!(harness.client_output(&["lock"]), "already locked\n");
    let as_json = harness.client_output(&["--json", "lock"]);
    assert_eq!(as_json, "{\"already_locked\":true}\n");
    assert_eq!(harness.programs.calls("lock.sh"), [Vec::<String>::new()]);
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lock_command_that_cannot_start_exits_1_with_spawn_failed() {
    let harness = harness_paired_through(&["pair"]);
    fs::remove_file(harness.programs.program("lock.sh")).expect("fake lock.sh removed");

    let errors = harness.client_failure(&["lock"], EXIT_SERVER_ERROR);

    let refusal = "swayconnect: the request failed: spawn_failed: ";
    assert!(errors.starts_with(refusal), "{errors}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn awake_holds_the_inhibitor_for_the_asked_time_and_off_ends_it() {
    let harness = harness_paired_through(&["pair"]);
    harness
        .programs
        .set_mode("systemd-inhibit", FakeProcMode::Sleep(INHIBITOR_RUNS_FOR));

    let too_long = harness.client_failure(&["awake", "on", "--for", "3h"], EXIT_SERVER_ERROR);
    assert!(
        too_long.starts_with("swayconnect: the request failed: bad_request: "),
        "{too_long}"
    );
    let on = harness.client_output(&["awake", "on", "--for", "5m"]);
    assert_eq!(on, "keep awake on for 5m\n");
    let replaced = harness.client_output(&["--json", "awake", "on"]);
    assert_eq!(replaced, "{\"remaining_ms\":3600000}\n");
    assert_eq!(harness.client_output(&["awake", "off"]), "keep awake off\n");

    let sleep = harness.programs.program("sleep");
    let sleep = sleep.to_str().expect("UTF-8 path");
    let inhibit = |seconds: &str| {
        [
            "--what=idle",
            "--mode=block",
            "--who=swayconnect",
            "--why=Kept awake from a paired device",
            sleep,
            seconds,
        ]
        .map(str::to_owned)
        .to_vec()
    };
    assert_eq!(
        harness.programs.calls("systemd-inhibit"),
        [inhibit("300"), inhibit("3600")]
    );
    // The wait for the first snapshot and four client runs: five connects of a device with tier 2
    // scopes, one notice for them (once per device per 12 h, DESIGN 11.8); one keep-awake
    // notice, because the replace came from the same device.
    let notices = harness.calls_once_there_are("notify-send", 2);
    let texts: Vec<&str> = notices
        .iter()
        .filter_map(|argv| argv.last().map(String::as_str))
        .collect();
    let count = |prefix: &str| texts.iter().filter(|t| t.starts_with(prefix)).count();
    assert_eq!(count("Keep awake on (device "), 1, "{texts:?}");
    assert_eq!(
        count("Device with remote control rights connected (device "),
        1,
        "{texts:?}"
    );
    assert_eq!(texts.len(), 2, "{texts:?}");
    // Every fake output is powered on, so no `output * power on`.
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}
