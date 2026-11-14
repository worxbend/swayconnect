//! Milestone M2a of `docs/V2_PLAN.md` against a running daemon: window and session ops
//! (`docs/PROTOCOL.md` 17.6, 17.7), keep awake and topic `session` (17.8), rate classes
//! (17.14), read-only mode, the audit log (`docs/DESIGN.md` 11.8) and the default pairing.
//! Every program the daemon may start is the testkit's `fake-proc` (see `v2_m2a/rig.rs`).

#[path = "v2_m2a/rig.rs"]
mod rig;
mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use rig::{Gate, INHIBIT, LOCK, NOTIFY, PairCommand, Rig};
use serde_json::Value;
use support::{online_desktop, refusal, soon};
use swayconnect_cli::client::{ClientError, Event, Session};
use swayconnect_protocol::ops::{Args, ContainerLayout, KeepAwake, TOPIC_SESSION, WorkspaceTarget};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{ErrorBody, ErrorCode, Holder, IconHash, JsonObject, Scope};
use swayconnect_protocol::{KeepAwakeResult, LockResult, Session as SessionState};
use swayconnect_testkit::{FakeProcMode, FakeSway, Request, fixtures};

/// A released inhibitor ends by itself after this, should a failed test leave it behind.
const INHIBITOR_LIFETIME: Duration = Duration::from_secs(30);
const M2A_SCOPES: [Scope; 5] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Control,
    Scope::IdleInhibit,
    Scope::Destructive,
];
/// The owner's `swayconnectd pair` default (`docs/DESIGN.md` 11.8, D30) and the scopes that
/// are only ever granted by name.
const OWNER_DEFAULT: [&str; 9] = [
    "state.read",
    "state.titles",
    "control",
    "audio",
    "media",
    "idle.inhibit",
    "launch",
    "input.keys",
    "input.text",
];
const BY_NAME_ONLY: [&str; 3] = ["destructive", "audio.capture", "launch.risky"];
const LOCKER_PID: u32 = 4242;

/// One valid request per new operation, with the scope it needs.
fn new_operations() -> [(Args, Scope); 8] {
    let on = KeepAwake::On {
        duration_s: Some(600),
    };
    [
        (window_move(6, 5), Scope::Control),
        (
            Args::WindowFloating {
                window_id: 6,
                enabled: true,
            },
            Scope::Control,
        ),
        (
            Args::WindowLayout {
                window_id: 8,
                layout: ContainerLayout::Tabbed,
            },
            Scope::Control,
        ),
        (Args::WindowScratchpad { window_id: 13 }, Scope::Control),
        (Args::ScratchpadShow { window_id: 16 }, Scope::Control),
        (Args::SessionLock, Scope::Control),
        (Args::WindowClose { window_id: 6 }, Scope::Destructive),
        (Args::KeepAwakeSet(on), Scope::IdleInhibit),
    ]
}

fn window_move(window_id: u64, workspace_id: u64) -> Args {
    let target = WorkspaceTarget::Id(workspace_id);
    Args::WindowMove { window_id, target }
}

fn keep_awake(duration_s: Option<u64>) -> Args {
    Args::KeepAwakeSet(KeepAwake::On { duration_s })
}

fn op_name(args: &Args) -> &'static str {
    args.operation().as_str()
}

async fn ask(session: &mut Session, args: &Args) -> Result<JsonObject, ClientError> {
    soon(op_name(args), session.request(args)).await
}

async fn refused(session: &mut Session, args: &Args, code: ErrorCode) -> ErrorBody {
    let error = refusal(ask(session, args).await);
    assert_eq!(error.code, code, "{}: {error:?}", op_name(args));
    error
}

/// [`refused`] for an op that `welcome.ops` leaves out: the client library will not send it,
/// so it goes as a raw frame. Returns the `error` member of the response.
async fn refused_unoffered(session: &mut Session, id: u64, args: &Args, code: ErrorCode) -> Value {
    let request = args.to_req(id);
    let members = request
        .args
        .unwrap_or_else(|| Value::Object(JsonObject::new()));
    let frame = support::request_frame(id, &request.op, &members);
    let answer = support::raw_exchange(session, &frame).await;
    assert_eq!(answer["id"], id, "{answer:?}");
    assert_eq!(
        answer["error"]["code"],
        code.as_str(),
        "{}: {answer:?}",
        request.op
    );
    answer["error"].clone()
}

async fn lock(session: &mut Session) -> bool {
    let result = ask(session, &Args::SessionLock)
        .await
        .expect("session.lock");
    LockResult::decode(&result)
        .expect("lock result")
        .already_locked
}

/// Subscribes to `session` and returns the first state that satisfies `wanted`.
async fn session_state(
    session: &mut Session,
    wanted: impl Fn(&SessionState) -> bool,
) -> SessionState {
    let states = soon("subscribe", session.subscribe(&[TOPIC_SESSION])).await;
    let [first] = states.expect("subscribed").try_into().expect("one state");
    let first = SessionState::decode(&first.data).expect("session state");
    if wanted(&first) {
        return first;
    }
    next_session_state(session, wanted).await
}

async fn next_session_state(
    session: &mut Session,
    wanted: impl Fn(&SessionState) -> bool,
) -> SessionState {
    loop {
        let event = soon("state.changed", session.next_event()).await;
        let Ok(Event::State(state)) = event else {
            panic!("not a state event: {event:?}");
        };
        let state = SessionState::decode(&state.data).expect("session state");
        if wanted(&state) {
            return state;
        }
    }
}

/// The layout fixture with `TEST-2` powered off, as after swayidle's `output * power off`.
fn second_output_off(sway: &FakeSway) {
    let mut tree: Value = serde_json::from_str(&fixtures::layout_tree()).expect("tree JSON");
    let outputs = tree["nodes"].as_array_mut().expect("outputs");
    let panel = outputs.iter_mut().find(|output| output["name"] == "TEST-2");
    panel.expect("output TEST-2")["power"] = Value::Bool(false);
    sway.set_reply(Request::GetTree, tree.to_string());
    sway.set_reply(Request::GetWorkspaces, fixtures::layout_workspaces());
}

fn layout_tree(sway: &FakeSway) {
    sway.set_reply(Request::GetTree, fixtures::layout_tree());
    sway.set_reply(Request::GetWorkspaces, fixtures::layout_workspaces());
}

#[tokio::test]
async fn each_new_operation_needs_its_scope_and_a_refusal_touches_nothing() {
    let rig = Rig::start(false, layout_tree).await;
    let (_, mut reader) = rig.device(&[Scope::StateRead, Scope::StateTitles]).await;
    for (id, (args, scope)) in (1..).zip(new_operations()) {
        assert!(!reader.welcome().ops.iter().any(|op| op == op_name(&args)));
        let error = refused_unoffered(&mut reader, id, &args, ErrorCode::Forbidden).await;
        assert_eq!(error["detail"]["scope"], scope.as_str());
    }
    let (_, mut controller) = rig.device(&[Scope::StateRead, Scope::Control]).await;
    let tier_two = [
        (Args::WindowClose { window_id: 6 }, Scope::Destructive),
        (keep_awake(None), Scope::IdleInhibit),
    ];
    for (id, (args, scope)) in (1..).zip(tier_two) {
        let error = refused_unoffered(&mut controller, id, &args, ErrorCode::Forbidden).await;
        assert_eq!(error["detail"]["scope"], scope.as_str());
    }
    rig.assert_untouched();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn read_only_refuses_every_new_action_and_still_serves_the_session_topic() {
    let rig = Rig::start(true, layout_tree).await;
    let (_, mut session) = rig.device(&M2A_SCOPES).await;
    let welcome = session.welcome().clone();
    assert!(welcome.read_only);
    let known_ops = welcome.known_ops.expect("known_ops");
    for (id, (args, _scope)) in (1..).zip(new_operations()) {
        let op = op_name(&args);
        assert!(
            !welcome.ops.iter().any(|offered| offered == op),
            "{op} offered"
        );
        assert!(known_ops.iter().any(|known| known == op), "{op} not known");
        refused_unoffered(&mut session, id, &args, ErrorCode::ReadOnly).await;
    }
    // The first state can come before the first scan of the fake `/proc`.
    let state = session_state(&mut session, |state| state.locked.is_some()).await;
    assert!(!state.keep_awake.active);
    assert_eq!(state.locked, Some(false), "the fake /proc holds no locker");
    rig.assert_untouched();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn the_new_operations_share_the_actions_bucket_and_icons_keep_their_own() {
    let rig = Rig::start(false, layout_tree).await;
    let (_, mut session) = rig.device(&M2A_SCOPES).await;
    online_desktop(&mut session).await;
    let window_ops = [
        (
            window_move(6, 5),
            "[con_id=6] move container to workspace number 2",
        ),
        (
            Args::WindowFloating {
                window_id: 6,
                enabled: true,
            },
            "[con_id=6] floating enable",
        ),
        (
            Args::WindowLayout {
                window_id: 8,
                layout: ContainerLayout::Tabbed,
            },
            "[con_id=8] layout tabbed",
        ),
        (
            Args::WindowScratchpad { window_id: 13 },
            "[con_id=13] move scratchpad",
        ),
        (
            Args::ScratchpadShow { window_id: 16 },
            "[con_id=16] scratchpad show",
        ),
    ];
    let mut expected: Vec<String> = Vec::new();
    for (args, command) in &window_ops {
        let result = ask(&mut session, args).await;
        assert_eq!(result.expect("a full bucket serves 5"), JsonObject::new());
        expected.push((*command).to_owned());
    }
    assert_eq!(
        rig.sway.commands(),
        expected,
        "one sway command per op, in its fixed text"
    );

    // The bucket refills 2 per second, so a slow machine may serve a few more closes.
    let close = Args::WindowClose { window_id: 6 };
    let mut closed = 0;
    let error = loop {
        assert!(closed < 10, "the actions bucket never ran empty");
        match ask(&mut session, &close).await {
            Ok(_) => closed += 1,
            Err(failure) => break refusal(Err(failure)),
        }
    };
    assert_eq!(error.code, ErrorCode::RateLimited);
    let retry_after = error.detail.expect("detail")["retry_after_ms"]
        .as_u64()
        .expect("retry_after_ms");
    assert!(
        (1..=500).contains(&retry_after),
        "one token refills in 500 ms: {retry_after}"
    );
    refused(&mut session, &keep_awake(None), ErrorCode::RateLimited).await;
    refused(&mut session, &Args::SessionLock, ErrorCode::RateLimited).await;

    let icons = ask(
        &mut session,
        &Args::IconGet {
            hashes: vec![IconHash::of(b"none")],
        },
    )
    .await;
    assert!(
        icons.is_ok(),
        "the icons class has its own bucket: {icons:?}"
    );
    let kills = rig.sway.commands().len() - expected.len();
    assert_eq!(kills, closed, "a refused close sends nothing");
    assert_eq!(
        rig.request_lines().len(),
        closed,
        "one audit line per served close only"
    );
    assert_eq!(rig.programs.calls(INHIBIT), Vec::<Vec<String>>::new());
    rig.daemon.stop().await;
}

#[tokio::test]
async fn keep_awake_outlives_its_client_and_the_panic_switch_ends_it() {
    let rig = Rig::start(false, second_output_off).await;
    rig.programs
        .set_mode(INHIBIT, FakeProcMode::Sleep(INHIBITOR_LIFETIME));
    let inhibitor = Gate::install(&rig.programs, INHIBIT);
    let notice = Gate::install(&rig.programs, NOTIFY);
    let (profile, mut first) = rig.device(&M2A_SCOPES).await;
    let short_id = &profile.device_id.as_str()[..4];
    let connected_text = format!("Device with remote control rights connected (device {short_id})");
    let connected = [
        "--app-name=swayconnect",
        "--",
        "swayconnect",
        &connected_text,
    ];
    assert_eq!(
        notice.release().await,
        connected,
        "`idle.inhibit` is tier 2, so the connect shows a notice"
    );
    online_desktop(&mut first).await;

    let result = ask(&mut first, &keep_awake(Some(600)))
        .await
        .expect("keep awake on");
    assert_eq!(
        KeepAwakeResult::decode(&result)
            .expect("result")
            .remaining_ms,
        Some(600_000)
    );
    let shown = [
        "--app-name=swayconnect",
        "--",
        "swayconnect",
        &format!("Keep awake on (device {short_id})"),
    ];
    assert_eq!(
        notice.release().await,
        shown,
        "a fixed-text notice with the short id"
    );
    assert_eq!(rig.sway.commands(), ["output * power on"], "TEST-2 was off");
    soon("close", first.close()).await.expect("closed");

    let mut second = rig.connect(&profile).await;
    assert_eq!(notice.release().await, connected, "every connect");
    let state = session_state(&mut second, |_| true).await;
    assert!(
        state.keep_awake.active,
        "a disconnect does not end keep-awake"
    );
    assert_eq!(state.keep_awake.holder, Some(Holder::ThisDevice));
    let sleep = rig.programs.program("sleep");
    let sleep = sleep.to_str().expect("UTF-8 path");
    let inhibit_argv = [
        "--what=idle",
        "--mode=block",
        "--who=swayconnect",
        "--why=Kept awake from a paired device",
        sleep,
        "600",
    ];
    assert_eq!(inhibitor.release().await, inhibit_argv);

    rig.daemon.enter_read_only();
    next_session_state(&mut second, |state| !state.keep_awake.active).await;
    refused(&mut second, &keep_awake(None), ErrorCode::ReadOnly).await;
    rig.daemon.stop().await;
}

#[tokio::test]
async fn every_keep_awake_change_and_close_leaves_one_audit_line() {
    let rig = Rig::start(false, layout_tree).await;
    // Each inhibitor waits at the gate until the daemon kills it.
    let _inhibitor = Gate::install(&rig.programs, INHIBIT);
    let (profile, mut session) = rig.device(&M2A_SCOPES).await;
    online_desktop(&mut session).await;

    ask(&mut session, &keep_awake(Some(600))).await.expect("on");
    ask(&mut session, &keep_awake(None)).await.expect("replace");
    let off = ask(&mut session, &Args::KeepAwakeSet(KeepAwake::Off))
        .await
        .expect("off");
    assert_eq!(
        KeepAwakeResult::decode(&off).expect("result").remaining_ms,
        None
    );
    let too_short = Args::KeepAwakeSet(KeepAwake::On {
        duration_s: Some(59),
    });
    refused(&mut session, &too_short, ErrorCode::BadRequest).await;
    ask(&mut session, &Args::WindowClose { window_id: 6 })
        .await
        .expect("close");

    let lines = rig.request_lines();
    let summary: Vec<(&str, Option<u64>, &str)> = lines
        .iter()
        .map(|line| {
            (
                line["op"].as_str().unwrap_or("?"),
                line["duration_s"].as_u64(),
                line["result"].as_str().unwrap_or("?"),
            )
        })
        .collect();
    let expected = [
        ("keep_awake.set", Some(600), "ok"),
        ("keep_awake.set", Some(3600), "ok"),
        ("keep_awake.set", None, "ok"),
        ("window.close", None, "ok"),
    ];
    assert_eq!(summary, expected, "a refusal at step 6 is not audited");
    for line in &lines {
        assert_eq!(line["device"], profile.device_id.as_str());
        let peer = line["peer"].as_str().expect("peer");
        assert!(peer.starts_with("127.0.0.1:"), "{peer}");
    }
    assert_eq!(lines[3]["targets"], serde_json::json!([6]));
    assert_eq!(rig.sway.commands(), ["[con_id=6] kill"]);
    rig.daemon.stop().await;
}

#[tokio::test]
async fn session_lock_starts_one_lock_command_and_none_while_the_screen_is_locked() {
    let rig = Rig::start(false, layout_tree).await;
    let lock_command = Gate::install(&rig.programs, LOCK);
    rig.add_process(LOCKER_PID, "swaylock");
    let (_, mut session) = rig.device(&[Scope::StateRead, Scope::Control]).await;
    session_state(&mut session, |state| state.locked == Some(true)).await;
    assert!(lock(&mut session).await, "locked: nothing starts");

    rig.remove_process(LOCKER_PID);
    next_session_state(&mut session, |state| state.locked == Some(false)).await;
    // Had the first call started the command, it would still wait at the gate here.
    assert!(
        !lock(&mut session).await,
        "unlocked: the lock command starts"
    );
    assert!(
        lock(&mut session).await,
        "its child still runs: no second one"
    );
    assert_eq!(
        lock_command.release().await,
        Vec::<String>::new(),
        "argv from [session] lock"
    );
    rig.daemon.stop().await;
}

#[tokio::test]
async fn a_default_pairing_holds_the_owners_set_and_no_scope_granted_only_by_name() {
    let rig = Rig::start(false, |_| {}).await;
    let mut pair = PairCommand::start(&rig.dirs);
    let uri = PairingUri::parse(&pair.line_with("swayconnect://pair").await).expect("URI");
    let connector = support::connector();
    let pending = soon("pair", connector.pair(&uri, "phone", None))
        .await
        .expect("pending");
    pair.line_with("code: ").await;
    pair.answer("y\n");
    let profile = soon("confirmed", pending.confirmed())
        .await
        .expect("confirmed");
    pair.line_with("paired: ").await;

    let session = rig.connect(&profile).await;
    let welcome = session.welcome();
    let known = welcome.known_scopes.as_deref().expect("known_scopes");
    let granted: BTreeSet<&str> = welcome.scopes.iter().map(|scope| scope.as_str()).collect();
    let expected: BTreeSet<&str> = OWNER_DEFAULT
        .into_iter()
        .filter(|scope| known.iter().any(|name| name == scope))
        .collect();
    assert_eq!(
        granted, expected,
        "every known scope of the owner's default, nothing more"
    );
    assert!(granted.contains("idle.inhibit"));
    assert!(BY_NAME_ONLY.iter().all(|scope| !granted.contains(scope)));
    rig.daemon.stop().await;
}
