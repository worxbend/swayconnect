//! The M1 subcommands of `swayconnect` (`docs/V2_PLAN.md` slice CLI.1) against a
//! `swayconnectd` child process and the fake sway with the layout fixture of the testkit.

mod e2e_support;

use std::thread;
use std::time::{Duration, Instant};

use e2e_support::{EXIT_SERVER_ERROR, EXIT_USAGE_OR_PROFILE, Harness, PATIENCE, parse, stderr};
use serde_json::{Value, json};
use swayconnect_testkit::{Event, Request, fixtures};

const RETRY_DELAY: Duration = Duration::from_millis(200);
/// A hash in the strict form that no PNG of the daemon has.
const UNKNOWN_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAA";

/// Swaps the fake sway to the layout fixture and waits until the daemon publishes it.
fn harness_with_the_layout_fixture() -> Harness {
    let harness = Harness::start();
    harness.pair();
    harness.wait_for_the_first_snapshot();
    harness
        .sway
        .set_reply(Request::GetTree, fixtures::layout_tree());
    harness
        .sway
        .set_reply(Request::GetWorkspaces, fixtures::layout_workspaces());
    harness
        .sway
        .send_event(Event::Workspace, json!({"change": "focus"}).to_string());
    let deadline = Instant::now() + PATIENCE;
    while !harness.client(&["layout", "1"]).status.success() {
        assert!(
            Instant::now() < deadline,
            "the layout fixture never arrived"
        );
        thread::sleep(RETRY_DELAY);
    }
    harness
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn layout_draws_the_workspace_and_lists_every_window_of_it() {
    let harness = harness_with_the_layout_fixture();

    let text = harness.client_output(&["layout", "1"]);
    assert!(text.starts_with("workspace 1 (id 4), "), "{text}");
    for label in ["|6 notes", "|8 terminal", "|12 mail", "|13 calc"] {
        assert!(text.contains(label), "{label}: {text}");
    }
    let legend: Vec<Vec<&str>> = text
        .lines()
        .skip_while(|line| !line.starts_with("ID"))
        .map(|line| line.split_whitespace().collect())
        .collect();
    let expected: [&[&str]; 6] = [
        &["ID", "APP", "PLACE", "TITLE"],
        &["6", "notes", "tiled"],
        &["8", "terminal", "tiled"],
        &["9", "docs", "hidden", "tab"],
        &["12", "mail", "tiled"],
        &["13", "calc", "floating"],
    ];
    assert_eq!(legend.len(), expected.len(), "{text}");
    for (row, expected_row) in legend.iter().zip(expected) {
        assert!(row.starts_with(expected_row), "{expected_row:?}: {text}");
    }

    let fullscreen = harness.client_output(&["layout", "2"]);
    assert!(fullscreen.contains("|7 Browser"), "{fullscreen}");
    assert!(fullscreen.contains("fullscreen"), "{fullscreen}");

    let json_line = harness.client_output(&["layout", "--id", "14", "--json"]);
    assert_eq!(json_line.lines().count(), 1);
    let value = parse(&json_line);
    assert_eq!(value["workspace_id"], 14);
    assert_eq!(value["layout"]["fullscreen"], Value::Null);
    assert!(value["layout"]["tiled"].is_object(), "{value}");

    let missing = harness.client_failure(&["layout", "9"], EXIT_SERVER_ERROR);
    assert!(missing.contains("no workspace 9"), "{missing}");
    harness.client_failure(&["layout", "web"], EXIT_USAGE_OR_PROFILE);
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_json_prints_the_icon_member_and_the_scratchpad() {
    let harness = harness_with_the_layout_fixture();

    let lines: Vec<Value> = harness
        .client_output(&["windows", "--json"])
        .lines()
        .map(parse)
        .collect();

    let (listed, hidden): (Vec<&Value>, Vec<&Value>) = lines
        .iter()
        .partition(|window| window.get("scratchpad").is_none());
    assert!(!listed.is_empty());
    for window in &listed {
        let icon = window.get("icon").expect("member icon");
        assert!(icon.is_null() || icon.is_string(), "{window}");
    }
    assert_eq!(hidden.len(), 1, "{lines:?}");
    assert_eq!(hidden[0]["id"], 16);
    assert_eq!(hidden[0]["app_id"], "music");
    assert_eq!(hidden[0]["scratchpad"], true);

    let table = harness.client_output(&["windows"]);
    let last: Vec<&str> = table
        .lines()
        .last()
        .expect("rows")
        .split_whitespace()
        .collect();
    assert_eq!(last[..4], ["16", "-", "music", "scratchpad"], "{table}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn icon_refuses_a_bad_hash_and_writes_nothing_for_an_unknown_one() {
    let harness = Harness::start();
    harness.pair();
    let file = harness.client_dirs.root().join("app.png");
    let file_name = file.to_str().expect("UTF-8 path");

    let info = harness.client_output(&["info"]);
    assert!(info.contains("icon.get"), "{info}");

    let unknown =
        harness.client_failure(&["icon", UNKNOWN_HASH, "-o", file_name], EXIT_SERVER_ERROR);
    assert!(unknown.contains("no icon for this hash"), "{unknown}");
    assert!(!file.exists());

    let output = harness.client(&["icon", "firefox", "-o", file_name]);
    assert_eq!(output.status.code(), Some(EXIT_USAGE_OR_PROFILE));
    assert!(
        stderr(&output).contains("not an icon hash"),
        "{}",
        stderr(&output)
    );
    assert!(!file.exists());
}
