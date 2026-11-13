//! The action subcommands of `swayconnect` against a `swayconnectd` child process and the
//! fake sway (`docs/DESIGN.md` 7.2 and 9.3, row "CLI end to end").

mod e2e_support;

use e2e_support::{
    EXIT_SERVER_ERROR, EXIT_USAGE_OR_PROFILE, FOCUS_WINDOW_6_REQUEST, Harness, stderr,
};
use serde_json::json;

const FOCUS_WINDOW_6_COMMAND: &str = "[con_id=6] focus";
/// More runs than the action bucket holds tokens, with room for refills on a slow machine.
const MAX_RUNS_UNTIL_RATE_LIMITED: usize = 40;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_action_subcommand_sends_its_command_until_the_rate_limit() {
    let harness = Harness::start();
    harness.pair();
    harness.wait_for_the_first_snapshot();
    let info = harness.client_output(&["info"]);
    let offered =
        "ops:        ping, state.subscribe, workspace.focus, window.focus, window.fullscreen";
    assert!(info.contains(offered), "{info}");

    assert_eq!(harness.client_output(&["workspace", "3"]), "ok\n");
    assert_eq!(harness.client_output(&["workspace", "--id", "5"]), "ok\n");
    assert_eq!(harness.client_output(&["focus", "7"]), "ok\n");
    assert_eq!(harness.client_output(&["fullscreen", "6", "on"]), "ok\n");
    let as_json = harness.client_output(&["--json", "fullscreen", "6", "off"]);
    assert_eq!(as_json, "{}\n");
    let sent = [
        "workspace --no-auto-back-and-forth number 3",
        "workspace --no-auto-back-and-forth number 2",
        "[con_id=7] focus",
        "[con_id=6] fullscreen enable",
        "[con_id=6] fullscreen disable",
    ];
    assert_eq!(harness.sway.commands(), sent);

    // The bucket held five tokens and refills while the runs above take their time, so the
    // number of further successes is not fixed. Each of them reaches sway; the refusal not.
    let mut further_successes = 0;
    let limited = loop {
        let output = harness.client(&["focus", "6"]);
        if !output.status.success() {
            break output;
        }
        further_successes += 1;
        assert!(
            further_successes < MAX_RUNS_UNTIL_RATE_LIMITED,
            "the rate limit never refused an action"
        );
    };
    assert_eq!(limited.status.code(), Some(EXIT_SERVER_ERROR));
    assert_eq!(limited.stdout, b"");
    let errors = stderr(&limited);
    let refusal = "swayconnect: the request failed: rate_limited: ";
    assert!(errors.starts_with(refusal), "{errors}");
    let delay = errors.trim_end().rsplit_once(" (retry after ");
    let delay = delay.and_then(|(_message, delay)| delay.strip_suffix(" ms)"));
    let delay: u64 = delay
        .expect("the retry delay")
        .parse()
        .expect("milliseconds");
    assert!((1..=500).contains(&delay), "{errors}");
    let reached_sway = harness.sway.commands();
    assert_eq!(reached_sway.len(), sent.len() + further_successes);
    assert!(
        reached_sway[sent.len()..]
            .iter()
            .all(|command| command == FOCUS_WINDOW_6_COMMAND)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_action_exits_1_with_the_error_code_and_raw_prints_every_reply() {
    let harness = Harness::start();
    harness.pair();
    harness.wait_for_the_first_snapshot();

    // 999 is no id at all; 4 is a workspace, 6 a window.
    let refused: [&[&str]; 4] = [
        &["focus", "999"],
        &["focus", "4"],
        &["fullscreen", "4", "on"],
        &["workspace", "--id", "6"],
    ];
    for arguments in refused {
        let errors = harness.client_failure(arguments, EXIT_SERVER_ERROR);
        let refusal = "swayconnect: the request failed: not_found: ";
        assert!(errors.starts_with(refusal), "{arguments:?}: {errors}");
    }
    let usage = harness.client_failure(&["workspace", "1000"], EXIT_USAGE_OR_PROFILE);
    assert!(usage.contains("1000"), "{usage}");
    assert_eq!(harness.sway.commands(), Vec::<String>::new());

    let replies = harness
        .raw_replies(r#"{"type":"req","id":7,"op":"sway.command","args":{"command":"exec true"}}"#);
    let [reply] = replies.as_slice() else {
        panic!("one reply, got {replies:?}");
    };
    assert_eq!((&reply["id"], &reply["ok"]), (&json!(7), &json!(false)));
    assert_eq!(reply["error"]["code"], "unknown_op");
    // No request at all: the server says `bye` and closes, which ends `raw` before its 2 s.
    let replies = harness.raw_replies(r#"{"type":"req","op":"ping"}"#);
    let [bye] = replies.as_slice() else {
        panic!("one reply, got {replies:?}");
    };
    assert_eq!(
        (&bye["type"], &bye["code"]),
        (&json!("bye"), &json!("protocol_error"))
    );
    let replies = harness.raw_replies(FOCUS_WINDOW_6_REQUEST);
    assert_eq!(
        replies,
        [json!({"type": "res", "id": 1, "ok": true, "result": {}})]
    );
    assert_eq!(harness.sway.commands(), [FOCUS_WINDOW_6_COMMAND]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_without_control_cannot_act_through_any_subcommand() {
    let harness = Harness::start();
    harness.pair_through(&["pair", "--scopes", "state.read"]);
    harness.wait_for_the_first_snapshot();

    let actions: [&[&str]; 4] = [
        &["workspace", "3"],
        &["workspace", "--id", "5"],
        &["focus", "6"],
        &["fullscreen", "6", "on"],
    ];
    for arguments in actions {
        let errors = harness.client_failure(arguments, EXIT_SERVER_ERROR);
        assert!(
            errors.contains("the server does not offer operation"),
            "{arguments:?}: {errors}"
        );
    }
    // `raw` sends what the subcommands refuse to send; the server refuses it then.
    let replies = harness.raw_replies(FOCUS_WINDOW_6_REQUEST);
    let [reply] = replies.as_slice() else {
        panic!("one reply, got {replies:?}");
    };
    assert_eq!(reply["error"]["code"], "forbidden");
    assert_eq!(reply["error"]["detail"], json!({"scope": "control"}));
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_only_daemon_serves_the_state_and_refuses_every_action() {
    let harness = Harness::start_with("read_only = true\n");
    harness.pair();
    harness.wait_for_the_first_snapshot();
    let info = harness.client_output(&["info"]);
    assert!(info.contains("read-only:  yes"), "{info}");
    assert_eq!(harness.client_output(&["workspaces"]).lines().count(), 3);

    let errors = harness.client_failure(&["focus", "6"], EXIT_SERVER_ERROR);
    assert!(
        errors.contains("the server does not offer operation"),
        "{errors}"
    );
    let replies = harness.raw_replies(FOCUS_WINDOW_6_REQUEST);
    let [reply] = replies.as_slice() else {
        panic!("one reply, got {replies:?}");
    };
    assert_eq!(reply["error"]["code"], "read_only");
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}
