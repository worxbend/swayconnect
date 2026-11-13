//! `swayconnect` against a `swayconnectd` child process and the fake sway (`docs/DESIGN.md`
//! 9.3, row "CLI end to end").
//!
//! Every child has a cleared environment and temp XDG directories. The daemon listens on
//! `127.0.0.1:0` and its `sway.socket` is the fake sway (`docs/DESIGN.md` 9.2).

mod e2e_support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use e2e_support::{
    CLIENT, EXIT_CONNECTION, EXIT_REJECTED, EXIT_USAGE_OR_PROFILE, FOCUS_WINDOW_6_REQUEST, Harness,
    PATIENCE, Process, parse, stderr,
};
use serde_json::{Value, json};
use swayconnect_testkit::{Event, FakeSway, Request, TestDirs, fixtures};

/// Moves the focus of the fake sway from workspace 4, window 6 to workspace 5, window 7.
fn focus_the_second_workspace(sway: &FakeSway) {
    let mut workspaces = parse(&fixtures::workspaces());
    workspaces[0]["focused"] = json!(false);
    workspaces[1]["focused"] = json!(true);
    let mut tree = parse(&fixtures::tree());
    let output = &mut tree["nodes"][0]["nodes"];
    output[0]["nodes"][0]["focused"] = json!(false);
    output[1]["nodes"][0]["focused"] = json!(true);
    sway.set_reply(Request::GetWorkspaces, workspaces.to_string());
    sway.set_reply(Request::GetTree, tree.to_string());
    sway.send_event(Event::Workspace, json!({"change": "focus"}).to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_read_subcommand_works_against_a_daemon_child() {
    let harness = Harness::start();
    harness.pair();
    let mode = fs::metadata(harness.profile_file()).expect("profile file");
    assert_eq!(mode.permissions().mode() & 0o777, 0o600);

    // `watch` first: its line with `sway online` says the daemon has its first snapshot.
    let mut watch = Process::start(harness.client_command(&["watch"]), false);
    let first = watch.lines.expect("sway online");
    assert!(
        first.ends_with("sway online  workspaces 2  windows 2  focused notes"),
        "{first}"
    );

    let info = harness.client_output(&["info"]);
    assert!(info.contains("server:     desk (0.1.0)"), "{info}");
    assert!(info.contains("scopes:     state.read, state.titles, control"));
    assert!(info.contains("ops:        ping, state.subscribe"));
    assert!(info.contains("topics:     desktop"));
    assert!(info.contains("read-only:  no"));
    let info = parse(&harness.client_output(&["info", "--json"]));
    assert_eq!(info["server"]["name"], "desk");
    let device_id = info["device_id"].as_str().expect("device_id").to_owned();

    let state = harness.client_output(&["state", "--json"]);
    assert_eq!(state.lines().count(), 1, "one JSON value per line");
    let state = &parse(&state)["states"][0];
    assert_eq!(state["topic"], "desktop");
    assert_eq!(state["data"]["sway"]["online"], true);
    assert_eq!(
        state["data"]["windows"][0]["title"],
        "Packing list for the lake trip"
    );
    let pretty = parse(&harness.client_output(&["state"]));
    assert_eq!(pretty["states"][0]["data"], state["data"]);

    let workspaces = harness.client_output(&["workspaces"]);
    let rows: Vec<Vec<&str>> = workspaces
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    let expected = [
        vec!["ID", "NUM", "NAME", "OUTPUT", "FLAGS"],
        vec!["4", "1", "1", "TEST-1", "focused,visible"],
        vec!["5", "2", "2:", "web", "TEST-1", "-"],
    ];
    assert_eq!(rows, expected);
    let workspaces = harness.client_output(&["workspaces", "--json"]);
    let ids: Vec<Value> = workspaces
        .lines()
        .map(|line| parse(line)["id"].clone())
        .collect();
    assert_eq!(ids, [json!(4), json!(5)]);

    let windows = harness.client_output(&["windows"]);
    let rows: Vec<&str> = windows.lines().collect();
    assert_eq!(rows.len(), 3, "{windows}");
    assert!(rows[1].starts_with('6') && rows[1].ends_with("Packing list for the lake trip"));
    assert!(rows[2].starts_with('7') && rows[2].ends_with("Sourdough for beginners"));
    let windows = harness.client_output(&["windows", "--json"]);
    let apps: Vec<Value> = windows
        .lines()
        .map(|line| parse(line)["app_id"].clone())
        .collect();
    assert_eq!(apps, [json!("notes"), json!("Browser")]);

    focus_the_second_workspace(&harness.sway);
    let changed = watch.lines.expect("focused Browser");
    assert!(
        changed.contains("sway online  workspaces 2  windows 2"),
        "{changed}"
    );

    let revoke = harness.daemon_command(&["revoke", &device_id]).output();
    assert!(revoke.expect("revoke runs").status.success());
    let (exit_code, errors) = watch.finish();
    assert_eq!(exit_code, Some(EXIT_REJECTED), "{errors}");
    assert!(errors.contains("revoked"), "{errors}");
    let refused = harness.client(&["info"]);
    assert_eq!(refused.status.code(), Some(EXIT_REJECTED));
    assert!(
        stderr(&refused).contains("auth_failed"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(harness.sway.commands(), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_no_from_the_owner_exits_4_and_stores_no_profile() {
    let harness = Harness::start();
    let (mut owner, client) = harness.pair_until_the_prompt(&["pair"]);

    owner.type_line("n");

    let (exit_code, errors) = client.finish();
    assert_eq!(exit_code, Some(EXIT_REJECTED), "{errors}");
    assert!(!harness.profile_file().exists());
    let without_profile = harness.client(&["state"]);
    assert_eq!(without_profile.status.code(), Some(EXIT_USAGE_OR_PROFILE));
    assert!(stderr(&without_profile).contains("run `swayconnect pair` first"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_daemon_is_a_connection_failure() {
    let mut harness = Harness::start();
    harness.pair();

    harness.daemon.child.kill().expect("daemon killed");
    harness.daemon.child.wait().expect("daemon exit status");

    let subcommands: [&[&str]; 9] = [
        &["info"],
        &["state"],
        &["workspaces"],
        &["windows"],
        &["watch"],
        &["workspace", "3"],
        &["focus", "6"],
        &["fullscreen", "6", "on"],
        &["raw", FOCUS_WINDOW_6_REQUEST],
    ];
    for subcommand in subcommands {
        harness.client_failure(subcommand, EXIT_CONNECTION);
    }
}

#[test]
fn pair_refuses_a_uri_argument_without_repeating_it() {
    let dirs = TestDirs::new();
    let secret = "c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2U";
    let uri = format!("swayconnect://pair?v=1&host=127.0.0.1&port=9&fp={secret}&s={secret}");

    let output = assert_cmd::Command::from_std(dirs.command(CLIENT))
        .args(["pair", &uri])
        .timeout(PATIENCE)
        .output()
        .expect("client runs");

    assert_eq!(output.status.code(), Some(EXIT_USAGE_OR_PROFILE));
    assert!(stderr(&output).contains("read from stdin"));
    assert!(!stderr(&output).contains(secret));
    assert_eq!(output.stdout, b"");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_test_directories_are_removed_with_the_harness() {
    let harness = Harness::start();
    harness.pair();
    let roots = [&harness.daemon_dirs, &harness.client_dirs].map(|dirs| dirs.root().to_owned());
    assert!(harness.profile_file().exists());
    assert!(
        harness
            .daemon_dirs
            .data_dir()
            .join("identity.cert.der")
            .exists()
    );

    drop(harness);

    for root in roots {
        assert!(!root.exists(), "{} is left behind", root.display());
    }
}
