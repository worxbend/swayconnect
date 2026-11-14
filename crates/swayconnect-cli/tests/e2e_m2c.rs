//! The M2c subcommands of `swayconnect` (`docs/M2C_PLAN.md` slice G-CLI) against a
//! `swayconnectd` child process, the fake sway, and `fake-proc` for `pactl`, `fuzzel` and
//! the other programs.
//!
//! As in `e2e_m2b.rs`, the child has no `WAYLAND_DISPLAY` and no session bus: the `audio`
//! state has no devices, `media` has no players, and an input that passes every check ends
//! in `backend_unavailable`. The real programs are the business of the daemon's
//! `tests/real_*.rs`.

mod e2e_support;

use std::time::Duration;

use e2e_support::{EXIT_SERVER_ERROR, Harness, stderr};
use swayconnect_testkit::FakeProcMode;

/// The one desktop entry of the testkit's `XDG_DATA_DIRS`.
const NOTES_ENTRY: &str = "notes";
const M2C_OPS: [&str; 4] = [
    "audio.set_default",
    "media.next",
    "media.previous",
    "launcher.open",
];
/// Long enough for the test, short enough to leave no `fake-proc` behind for long.
const LAUNCHER_RUNS_FOR: Duration = Duration::from_secs(5);
const UNAVAILABLE: &str = "swayconnect: the request failed: backend_unavailable: ";

fn paired_harness() -> Harness {
    let harness = Harness::start();
    harness.pair();
    harness.wait_for_the_first_snapshot();
    harness
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_default_pairing_is_offered_every_m2c_op() {
    let harness = paired_harness();

    let info = harness.client_output(&["info"]);

    for op in M2C_OPS {
        assert!(info.contains(op), "{op}: {info}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_list_commands_print_the_topics_of_the_daemon() {
    let harness = paired_harness();

    let apps = harness.client_output(&["apps"]);
    let notes_row = apps.lines().find(|line| line.starts_with(NOTES_ENTRY));
    assert!(apps.starts_with("ENTRY  FLAGS"), "{apps}");
    assert!(notes_row.is_some(), "{apps}");
    let apps_json = harness.client_output(&["--json", "apps"]);
    assert!(
        apps_json.contains(&format!("\"entry_id\":\"{NOTES_ENTRY}\"")),
        "{apps_json}"
    );

    let devices = harness.client_output(&["audio", "devices"]);
    assert!(
        devices.starts_with("ID  ROLE  KIND  VOLUME  FLAGS  DESCRIPTION"),
        "{devices}"
    );
    assert_eq!(harness.client_output(&["--json", "audio", "devices"]), "");

    let players = harness.client_output(&["media", "players"]);
    assert_eq!(players, "ID  STATUS  FLAGS  CAN  NAME  TRACK\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audio_default_and_media_skips_without_a_target_change_nothing() {
    let harness = paired_harness();

    for (arguments, message) in [
        (
            &["media", "next"][..],
            "swayconnect: no active media player",
        ),
        (
            &["media", "previous"],
            "swayconnect: no active media player",
        ),
        (
            &["media", "next", "--player", "1"],
            "swayconnect: the request failed: not_found: ",
        ),
        (
            &["audio", "default", "1"],
            "swayconnect: the request failed: not_found: ",
        ),
    ] {
        let errors = harness.client_failure(arguments, EXIT_SERVER_ERROR);
        assert!(errors.starts_with(message), "{arguments:?}: {errors}");
    }

    let pactl = harness.programs.calls("pactl");
    let writes: Vec<_> = pactl
        .iter()
        .filter(|call| call.iter().any(|argument| argument.starts_with("set-")))
        .collect();
    assert_eq!(writes, Vec::<&Vec<String>>::new(), "{pactl:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_launcher_opens_once_and_takes_input_only_while_it_runs() {
    let harness = paired_harness();

    // No launcher runs yet.
    let errors = harness.client_failure(&["key", "Escape", "--launcher"], EXIT_SERVER_ERROR);
    assert!(
        errors.starts_with("swayconnect: the request failed: launcher_closed: "),
        "{errors}"
    );

    harness
        .programs
        .set_mode("fuzzel", FakeProcMode::Sleep(LAUNCHER_RUNS_FOR));
    let opened = harness.client_output(&["launcher", "open"]);
    assert_eq!(opened, "opened: type into it with --launcher\n");
    let again = harness.client_output(&["--json", "launcher", "open"]);
    assert_eq!(again, "{\"already_open\":true}\n");
    assert_eq!(harness.programs.calls("fuzzel"), [Vec::<String>::new()]);

    // fuzzel holds the keyboard over the window focused at open, as real sway shows it.
    harness.sway.hold_keyboard_in_layer_surface(6);
    // This device holds the launcher, so every check passes; no Wayland socket is named.
    let output = harness.client_with_stdin(&["type", "--launcher"], "foot\n");
    let errors = stderr(&output);
    assert_eq!(output.status.code(), Some(EXIT_SERVER_ERROR), "{errors}");
    assert!(errors.starts_with(UNAVAILABLE), "{errors}");
    let errors = harness.client_failure(&["key", "Return", "--launcher"], EXIT_SERVER_ERROR);
    assert!(errors.starts_with(UNAVAILABLE), "{errors}");
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}
