//! The stage 1 end-to-end test, steps 1 to 7 (`docs/IMPLEMENTATION_PLAN.md` section 4).

mod support;

use serde_json::{Value, json};
use support::{EVERY_SCOPE, Harness, assert_bye, assert_rejected, connector, soon};
use swayconnect_daemon::admin::PairingResult;
use swayconnect_protocol::ops::{Args, WorkspaceTarget};
use swayconnect_protocol::{ByeCode, ErrorCode, JsonObject, RejectCode, Scope};
use swayconnect_testkit::{Event, Request, fixtures};

const WINDOW_LIFETIME: std::time::Duration = std::time::Duration::from_secs(300);
const FOCUS_WORKSPACE_3: &str = "workspace --no-auto-back-and-forth number 3";

fn parse(reply: &str) -> Value {
    serde_json::from_str(reply).expect("fixture JSON")
}

/// The default replies with the focus moved from workspace 4, window 6 to workspace 5,
/// window 7.
fn replies_with_focus_on_the_second_workspace() -> (String, String) {
    let mut workspaces = parse(&fixtures::workspaces());
    workspaces[0]["focused"] = json!(false);
    workspaces[0]["visible"] = json!(false);
    workspaces[1]["focused"] = json!(true);
    workspaces[1]["visible"] = json!(true);
    let mut tree = parse(&fixtures::tree());
    let output = &mut tree["nodes"][0]["nodes"];
    output[0]["nodes"][0]["focused"] = json!(false);
    output[1]["nodes"][0]["focused"] = json!(true);
    (workspaces.to_string(), tree.to_string())
}

#[tokio::test]
async fn pair_subscribe_follow_a_change_act_and_get_revoked() {
    // Step 1.
    let harness = Harness::start().await;
    assert!(harness.address().ip().is_loopback());

    // Step 2. `present_secret` compares the code of the admin side with the client's.
    let mut pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let pending = pairing.present_secret().await;
    pairing.answer(true).await;
    let profile = soon("pairing.confirmed", pending.confirmed())
        .await
        .expect("pairing confirmed");
    let PairingResult::Paired(paired) = pairing.result().await else {
        panic!("the owner said yes");
    };
    assert_eq!(paired.device_id, profile.device_id.as_str());
    assert_eq!(paired.scopes, ["state.read", "state.titles", "control"]);
    let mut session = harness.connect(&profile).await;
    assert_eq!(session.welcome().server.name, "desk");
    assert_eq!(session.welcome().topics, ["desktop", "session"]);

    // Step 3.
    let (first_rev, desktop) = support::online_desktop(&mut session).await;
    let workspaces: Vec<_> = desktop
        .workspaces
        .iter()
        .map(|workspace| (workspace.id, workspace.name.as_str(), workspace.focused))
        .collect();
    assert_eq!(workspaces, [(4, "1", true), (5, "2: web", false)]);
    let windows: Vec<_> = desktop
        .windows
        .iter()
        .map(|window| (window.id, window.workspace_id, window.focused))
        .collect();
    assert_eq!(windows, [(6, 4, true), (7, 5, false)]);
    assert_eq!(
        desktop.windows[0].title.as_deref(),
        Some("Packing list for the lake trip")
    );

    // Step 4.
    let (workspaces, tree) = replies_with_focus_on_the_second_workspace();
    harness.sway().set_reply(Request::GetWorkspaces, workspaces);
    harness.sway().set_reply(Request::GetTree, tree);
    harness
        .sway()
        .send_event(Event::Workspace, json!({"change": "focus"}).to_string());
    let (rev, desktop) = support::next_desktop(&mut session, |_| true).await;
    assert!(rev > first_rev, "rev {rev} after {first_rev}");
    let focused_workspaces: Vec<u64> = desktop
        .workspaces
        .iter()
        .filter(|workspace| workspace.focused)
        .map(|workspace| workspace.id)
        .collect();
    assert_eq!(focused_workspaces, [5]);
    let focused_windows: Vec<u64> = desktop
        .windows
        .iter()
        .filter(|window| window.focused)
        .map(|window| window.id)
        .collect();
    assert_eq!(focused_windows, [7]);
    assert_eq!(harness.sway().commands(), Vec::<String>::new());

    // Step 5.
    let focus = Args::WorkspaceFocus(WorkspaceTarget::Number(3));
    let done = soon("workspace.focus", session.request(&focus)).await;
    assert_eq!(done.expect("the action succeeds"), JsonObject::new());
    assert_eq!(harness.sway().commands(), [FOCUS_WORKSPACE_3]);

    // Step 6.
    let reader = harness.pair(&[Scope::StateRead]).await;
    let mut reader = harness.connect(&reader).await;
    let request = support::request_frame(1, "workspace.focus", &json!({"number": 3}));
    let refused = support::raw_exchange(&mut reader, &request).await;
    assert_eq!((&refused["id"], &refused["ok"]), (&json!(1), &json!(false)));
    assert_eq!(refused["error"]["code"], ErrorCode::Forbidden.as_str());
    assert_eq!(refused["error"]["detail"], json!({"scope": "control"}));
    assert_eq!(harness.sway().commands(), [FOCUS_WORKSPACE_3]);

    // Step 7.
    harness.revoke(&profile).await;
    assert_bye(soon("bye", session.next_event()).await, ByeCode::Revoked);
    assert_rejected(
        soon("reconnect", connector().connect(&profile)).await,
        RejectCode::AuthFailed,
    );
}
