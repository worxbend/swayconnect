//! The M2b subcommands of `swayconnect` (`docs/V2_PLAN.md` slice CLI.2b) against a
//! `swayconnectd` child process, the fake sway, and `fake-proc` for `pactl`, `systemd-run`
//! and `notify-send`.
//!
//! The child has no `WAYLAND_DISPLAY` and no session bus, so no virtual keyboard is ever
//! created and no real player is reached: an input that passes every check ends in
//! `backend_unavailable`, and `media` has no players. `fake-proc` prints nothing, so the
//! `audio` state has no devices.

mod e2e_support;

use e2e_support::{EXIT_SERVER_ERROR, EXIT_USAGE_OR_PROFILE, Harness, stderr};

/// The one desktop entry of the testkit's `XDG_DATA_DIRS`.
const NOTES_ENTRY: &str = "notes";
const M2B_OPS: [&str; 6] = [
    "audio.set_volume",
    "audio.set_mute",
    "media.play_pause",
    "app.launch",
    "input.text",
    "input.key",
];

fn paired_harness() -> Harness {
    let harness = Harness::start();
    harness.pair();
    harness.wait_for_the_first_snapshot();
    harness
}

/// Runs `type` with `stdin` and checks that it failed with exit code 1; returns its stderr.
fn type_failure(harness: &Harness, arguments: &[&str], stdin: &str) -> String {
    let output = harness.client_with_stdin(arguments, stdin);
    let errors = stderr(&output);
    let code = output.status.code();
    assert_eq!(code, Some(EXIT_SERVER_ERROR), "{arguments:?}: {errors}");
    assert_eq!(output.stdout, b"", "{arguments:?}");
    errors
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_default_pairing_is_offered_every_m2b_op() {
    let harness = paired_harness();

    let info = harness.client_output(&["info"]);

    for op in M2B_OPS {
        assert!(info.contains(op), "{op}: {info}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_starts_the_entry_under_systemd_run_after_the_workspace() {
    let harness = paired_harness();

    let started = harness.client_output(&["launch", NOTES_ENTRY, "--workspace", "3"]);
    assert_eq!(started, "started\n");
    let errors = harness.client_failure(&["launch", "gone.desktop"], EXIT_SERVER_ERROR);
    assert_eq!(
        errors,
        "swayconnect: the server lists no app gone.desktop\n"
    );

    let calls = harness.calls_once_there_are("systemd-run", 1);
    let [call] = calls.as_slice() else {
        panic!("one systemd-run, got {calls:?}");
    };
    let fixed = ["--user", "--scope", "--collect"];
    assert_eq!(call[..3], fixed, "{call:?}");
    assert!(
        call[3].starts_with("--unit=app-swayconnect-notes"),
        "{call:?}"
    );
    assert_eq!(call[4..], ["--", "/nonexistent/notes"], "{call:?}");
    let focus = "workspace --no-auto-back-and-forth number 3";
    assert_eq!(harness.sway.commands(), [focus]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_is_checked_against_the_focus_and_reaches_no_real_keyboard() {
    let harness = paired_harness();

    // Window 6 has the focus in the fake sway.
    let errors = type_failure(&harness, &["type", "--window", "7"], "hello\n");
    assert!(
        errors.starts_with("swayconnect: typing stopped: focus_changed: "),
        "{errors}"
    );
    assert!(
        errors.ends_with("(0 characters typed; window 6 has the focus now)\n"),
        "{errors}"
    );
    let errors = harness.client_failure(&["key", "Return", "--window", "7"], EXIT_SERVER_ERROR);
    assert!(
        errors.starts_with("swayconnect: the request failed: focus_changed: "),
        "{errors}"
    );

    // Every check passes; the test environment names no Wayland socket.
    let unavailable = "swayconnect: the request failed: backend_unavailable: ";
    let errors = type_failure(&harness, &["type", "--window", "6"], "hello");
    assert!(errors.starts_with(unavailable), "{errors}");
    let errors = harness.client_failure(&["key", "a", "--window", "6"], EXIT_SERVER_ERROR);
    assert!(errors.starts_with(unavailable), "{errors}");
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn type_refuses_text_in_arguments_without_printing_it() {
    let harness = paired_harness();

    let output = harness.client_with_stdin(&["type", "--window", "6", "hunter2"], "");

    assert_eq!(output.status.code(), Some(EXIT_USAGE_OR_PROFILE));
    let errors = stderr(&output);
    assert!(errors.contains("read from stdin"), "{errors}");
    assert!(!errors.contains("hunter2"), "{errors}");
    assert_eq!(output.stdout, b"");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audio_and_media_without_a_device_or_player_change_nothing() {
    let harness = paired_harness();

    for (arguments, message) in [
        (&["volume", "30"][..], "swayconnect: no default sink: "),
        (&["mute", "on"], "swayconnect: no default sink: "),
        (&["play"], "swayconnect: no active media player"),
        (
            &["volume", "30", "--device", "1"],
            "swayconnect: the request failed: ",
        ),
        (
            &["mute", "off", "--device", "1"],
            "swayconnect: the request failed: ",
        ),
        (
            &["play", "--player", "1"],
            "swayconnect: the request failed: ",
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
