//! Actions against a running daemon and the fake sway: the exact command text, and every
//! refusal that must reach sway with nothing (`docs/DESIGN.md` 4.4, `docs/PROTOCOL.md` 10).

mod support;

use serde_json::{Value, json};
use support::{EVERY_SCOPE, Harness, next_desktop, online_desktop, raw_exchange, soon};
use swayconnect_cli::client::Session;
use swayconnect_protocol::ops::{Args, WorkspaceTarget};
use swayconnect_protocol::text::is_control_or_format;
use swayconnect_protocol::{ErrorBody, ErrorCode, JsonObject, Scope};
use swayconnect_testkit::{Event, Request, fixtures};

const NO_COMMANDS: [&str; 0] = [];
const FOCUS_WINDOW_6: Args = Args::WindowFocus { window_id: 6 };
const FOCUS_WINDOW_6_COMMAND: &str = "[con_id=6] focus";

async fn act(session: &mut Session, action: &Args) -> Result<JsonObject, ErrorBody> {
    match soon("action", session.request(action)).await {
        Ok(result) => Ok(result),
        failed => Err(support::refusal(failed)),
    }
}

async fn refused_code(session: &mut Session, action: &Args) -> ErrorCode {
    let refused = act(session, action).await;
    refused.expect_err("the action is refused").code
}

/// One request per action form, as `(op, args)`.
fn every_action_form() -> [(&'static str, Value); 5] {
    [
        ("workspace.focus", json!({"number": 3})),
        ("workspace.focus", json!({"workspace_id": 5})),
        ("window.focus", json!({"window_id": 7})),
        (
            "window.fullscreen",
            json!({"window_id": 6, "enabled": true}),
        ),
        (
            "window.fullscreen",
            json!({"window_id": 6, "enabled": false}),
        ),
    ]
}

/// Sends every action form as an unchecked frame and returns the error code of each answer.
/// The client library would not send an operation that `welcome.ops` lacks.
async fn error_codes_of_every_form(session: &mut Session) -> Vec<Value> {
    let mut answers = Vec::new();
    for (id, (op, args)) in (1..).zip(every_action_form()) {
        let answer = raw_exchange(session, &support::request_frame(id, op, &args)).await;
        assert_eq!((&answer["id"], &answer["ok"]), (&json!(id), &json!(false)));
        answers.push(answer["error"].clone());
    }
    answers
}

/// The default tree without the second window, as after the owner closed it.
fn tree_without_window_7() -> String {
    let mut tree: Value = serde_json::from_str(&fixtures::tree()).expect("fixture JSON");
    tree["nodes"][0]["nodes"][1]["nodes"] = json!([]);
    tree.to_string()
}

#[tokio::test]
async fn each_action_sends_exactly_its_one_command() {
    let harness = Harness::start().await;
    let profile = harness.pair(&[Scope::Control, Scope::StateRead]).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;
    let fullscreen = |enabled| Args::WindowFullscreen {
        window_id: 6,
        enabled,
    };
    let actions = [
        Args::WorkspaceFocus(WorkspaceTarget::Number(3)),
        Args::WorkspaceFocus(WorkspaceTarget::Id(5)),
        Args::WindowFocus { window_id: 7 },
        fullscreen(true),
        fullscreen(false),
    ];

    for action in &actions {
        let done = act(&mut session, action).await;
        assert_eq!(done, Ok(JsonObject::new()), "{action:?}");
    }

    let sent = [
        "workspace --no-auto-back-and-forth number 3",
        // Workspace 5 is addressed by its number, never by its name `2: web`.
        "workspace --no-auto-back-and-forth number 2",
        "[con_id=7] focus",
        "[con_id=6] fullscreen enable",
        "[con_id=6] fullscreen disable",
    ];
    assert_eq!(harness.sway().commands(), sent);
}

#[tokio::test]
async fn an_id_of_the_wrong_kind_or_of_a_closed_window_reaches_no_command() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;
    let workspace_as_window = Args::WindowFullscreen {
        window_id: 4,
        enabled: true,
    };
    let wrong_kind = [
        Args::WindowFocus { window_id: 4 },
        workspace_as_window,
        Args::WorkspaceFocus(WorkspaceTarget::Id(6)),
    ];
    for action in &wrong_kind {
        let code = refused_code(&mut session, action).await;
        assert_eq!(code, ErrorCode::NotFound, "{action:?}");
    }

    harness
        .sway()
        .set_reply(Request::GetTree, tree_without_window_7());
    let closed = json!({"change": "close"}).to_string();
    harness.sway().send_event(Event::Window, closed);
    next_desktop(&mut session, |desktop| desktop.windows.len() == 1).await;
    let stale = Args::WindowFocus { window_id: 7 };
    assert_eq!(
        refused_code(&mut session, &stale).await,
        ErrorCode::NotFound
    );

    assert_eq!(harness.sway().commands(), NO_COMMANDS);
    // The session is still good for an action on a window that exists.
    assert_eq!(
        act(&mut session, &FOCUS_WINDOW_6).await,
        Ok(JsonObject::new())
    );
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND]);
}

#[tokio::test]
async fn a_device_without_control_is_refused_every_action() {
    let harness = Harness::start().await;
    let profile = harness.pair(&[Scope::StateRead, Scope::StateTitles]).await;
    let mut session = harness.connect(&profile).await;
    assert_eq!(session.welcome().ops, ["ping", "state.subscribe"]);
    online_desktop(&mut session).await;

    let errors = error_codes_of_every_form(&mut session).await;

    for error in errors {
        assert_eq!(error["code"], "forbidden");
        assert_eq!(error["detail"], json!({"scope": "control"}));
    }
    assert_eq!(harness.sway().commands(), NO_COMMANDS);
}

#[tokio::test]
async fn a_read_only_daemon_refuses_every_action_and_takes_no_token() {
    let harness = Harness::start_with("read_only = true\n").await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    assert_eq!(session.welcome().ops, ["ping", "state.subscribe"]);
    online_desktop(&mut session).await;

    // Ten actions: more than the bucket holds, so a token per refusal would show.
    for _round in 0..2 {
        for error in error_codes_of_every_form(&mut session).await {
            assert_eq!(error["code"], "read_only");
            assert_eq!(error.get("detail"), None);
        }
    }

    assert_eq!(harness.sway().commands(), NO_COMMANDS);
}

#[tokio::test]
async fn the_switch_to_read_only_stops_the_actions_of_an_open_session() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;
    assert_eq!(
        act(&mut session, &FOCUS_WINDOW_6).await,
        Ok(JsonObject::new())
    );

    // What SIGUSR1 calls.
    harness.daemon.enter_read_only();

    // This session was offered the action at login, so the client library still sends it.
    assert_eq!(
        refused_code(&mut session, &FOCUS_WINDOW_6).await,
        ErrorCode::ReadOnly
    );
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND]);
    soon("ping", session.request(&Args::Ping))
        .await
        .expect("the session stays open");
    let mut later = harness.connect(&profile).await;
    assert_eq!(later.welcome().ops, ["ping", "state.subscribe"]);
    let (_rev, desktop) = online_desktop(&mut later).await;
    assert_eq!(desktop.windows.len(), 2, "reading still works");
}

#[tokio::test]
async fn an_action_is_refused_while_sway_is_gone_and_works_after_it_is_back() {
    let mut harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;

    harness.stop_sway();
    next_desktop(&mut session, |desktop| !desktop.sway.online).await;
    assert_eq!(
        refused_code(&mut session, &FOCUS_WINDOW_6).await,
        ErrorCode::SwayUnavailable
    );

    harness.restart_sway();
    next_desktop(&mut session, |desktop| desktop.sway.online).await;
    assert_eq!(
        act(&mut session, &FOCUS_WINDOW_6).await,
        Ok(JsonObject::new())
    );
    // The refused action did not wait in a queue for the new sway.
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND]);
}

/// Takes the 2 s sway timeout in real time, twice at most: once for the action and once for
/// a reconnect attempt that is under way when sway answers again.
#[tokio::test]
async fn a_sway_that_hangs_during_an_action_is_a_timeout_and_the_action_is_not_repeated() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;

    harness.sway().stop_answering();
    assert_eq!(
        refused_code(&mut session, &FOCUS_WINDOW_6).await,
        ErrorCode::Timeout
    );
    // Sway got the command; whether it ran is unknown to the daemon (spec 10.1).
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND]);
    next_desktop(&mut session, |desktop| !desktop.sway.online).await;
    assert_eq!(
        refused_code(&mut session, &FOCUS_WINDOW_6).await,
        ErrorCode::SwayUnavailable
    );

    harness.sway().resume_answering();
    next_desktop(&mut session, |desktop| desktop.sway.online).await;
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND]);
}

#[tokio::test]
async fn a_command_that_sway_refuses_is_a_sway_error_with_clean_text() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    online_desktop(&mut session).await;
    let hostile = json!([{"success": false, "error": "No\u{1b}[2J matching\u{202e} node"}]);
    harness
        .sway()
        .set_reply(Request::RunCommand, hostile.to_string());

    let refused = act(&mut session, &FOCUS_WINDOW_6).await;

    let error = refused.expect_err("sway refused");
    assert_eq!(error.code, ErrorCode::SwayError);
    assert_eq!(error.message, "No\u{fffd}[2J matching\u{fffd} node");
    assert!(!error.message.chars().any(is_control_or_format));
    for not_one_success in ["[]", r#"[{"success":true},{"success":true}]"#] {
        harness
            .sway()
            .set_reply(Request::RunCommand, not_one_success);
        assert_eq!(
            refused_code(&mut session, &FOCUS_WINDOW_6).await,
            ErrorCode::SwayError,
            "{not_one_success}"
        );
    }

    // A refusal is an answer: the link to sway stays, and so does the session.
    harness
        .sway()
        .set_reply(Request::RunCommand, r#"[{"success":true}]"#);
    assert_eq!(
        act(&mut session, &FOCUS_WINDOW_6).await,
        Ok(JsonObject::new())
    );
    assert_eq!(harness.sway().commands(), [FOCUS_WINDOW_6_COMMAND; 4]);
}
