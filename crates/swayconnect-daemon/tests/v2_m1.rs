//! Milestone M1 of `docs/V2_PLAN.md` against a running daemon and the fake sway: the layout
//! of `docs/PROTOCOL.md` 17.4, the 2 s poll, `icon.get` (17.5) and `bye` `scopes_changed`
//! after `swayconnectd devices grant` (17.15).

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use support::{Harness, assert_bye, next_desktop, soon, subscribed_desktop};
use swayconnect_cli::client::{ClientError, Session};
use swayconnect_daemon::devices::ScopeChange;
use swayconnect_protocol::icon::{IconGetResult, IconHash};
use swayconnect_protocol::limits::ICON_BURST;
use swayconnect_protocol::ops::Args;
use swayconnect_protocol::{ByeCode, Desktop, ErrorCode, Layout, Scope, Tile};
use swayconnect_testkit::{FakeSway, Request, fixtures};

/// The supervisor reads the tree this often while a device subscribes (`docs/DESIGN.md` 11.5).
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Time for one tree read and one frame on loopback, on a busy test machine.
const DELIVERY_SLACK: Duration = Duration::from_secs(1);
/// Workspace `1` on `TEST-1`, with window 6 `notes` on its left.
const FIRST_WORKSPACE: u64 = 4;
const NOTES_WINDOW: u64 = 6;
/// Basis points of window 6's width: 1280 of 2560 px, then 1024 after the resize.
const NOTES_WIDTH_BP: u16 = 5000;
const NOTES_WIDTH_RESIZED_BP: u16 = 4000;
/// More than the `icons` bucket can refill while the test sends.
const MAX_ICON_REQUESTS: u32 = 20;

fn show_layout_tree(sway: &FakeSway) {
    sway.set_reply(Request::GetTree, fixtures::layout_tree());
    sway.set_reply(Request::GetWorkspaces, fixtures::layout_workspaces());
}

fn notes_hash() -> IconHash {
    IconHash::of(fixtures::NOTES_ICON_PNG)
}

fn layout_of(desktop: &Desktop, workspace_id: u64) -> Option<&Layout> {
    let workspace = desktop.workspaces.iter().find(|ws| ws.id == workspace_id)?;
    workspace.layout.as_ref()?.as_ref()
}

/// The basis-point rect of window `id`, searched in `tile` and below.
fn window_rect(tile: &Tile, id: u64) -> Option<[u16; 4]> {
    match tile {
        Tile::Window { id: found, r } => (*found == id).then_some(*r),
        Tile::Container(container) => container
            .children
            .iter()
            .find_map(|child| window_rect(child, id)),
    }
}

fn notes_width(desktop: &Desktop) -> Option<u16> {
    let tiled = layout_of(desktop, FIRST_WORKSPACE)?.tiled.as_ref()?;
    window_rect(tiled, NOTES_WINDOW).map(|[_x, _y, width, _h]| width)
}

fn has_notes_icon(desktop: &Desktop) -> bool {
    let notes = desktop.windows.iter().find(|w| w.id == NOTES_WINDOW);
    notes.is_some_and(|window| window.icon == Some(Some(notes_hash())))
}

async fn icon_get(
    session: &mut Session,
    hashes: &[IconHash],
) -> Result<IconGetResult, ClientError> {
    let args = Args::IconGet {
        hashes: hashes.to_vec(),
    };
    let result = soon("icon.get", session.request(&args)).await?;
    Ok(IconGetResult::decode(&result).expect("a valid icon.get result"))
}

/// A device whose scopes are `scopes`, connected.
async fn device_with(harness: &Harness, scopes: &[Scope]) -> Session {
    let profile = harness.pair(scopes).await;
    harness.connect(&profile).await
}

#[tokio::test]
async fn a_subscriber_gets_outputs_layouts_and_the_scratchpad() {
    let harness = Harness::start_prepared("", show_layout_tree).await;
    let mut session = device_with(&harness, &[Scope::StateRead]).await;

    let (_rev, desktop) = subscribed_desktop(&mut session, |d| d.sway.online).await;

    let outputs = desktop.outputs.as_deref().expect("outputs");
    let names: Vec<&str> = outputs.iter().map(|output| output.name.as_str()).collect();
    assert_eq!(
        names,
        ["TEST-1", "TEST-2"],
        "sway-internal outputs are left out"
    );
    assert_eq!(outputs[1].scale_milli, 1250);
    let first = layout_of(&desktop, FIRST_WORKSPACE).expect("layout of workspace 4");
    assert_eq!(
        first.size,
        [2560, 1410],
        "the workspace area without the bar"
    );
    assert_eq!(notes_width(&desktop), Some(NOTES_WIDTH_BP));
    assert_eq!(first.floating.len(), 1, "window 13 floats");
    let web = layout_of(&desktop, 5).expect("layout of workspace 5");
    assert_eq!(web.fullscreen, Some(7));
    let scratchpad = desktop.scratchpad.as_deref().expect("scratchpad");
    let hidden: Vec<u64> = scratchpad.iter().map(|window| window.id).collect();
    assert_eq!(hidden, [16]);
    assert!(desktop.windows.iter().all(|window| window.id != 16));
}

#[tokio::test]
async fn a_resize_without_an_event_reaches_a_subscriber_within_one_poll() {
    let harness = Harness::start_prepared("", show_layout_tree).await;
    let mut session = device_with(&harness, &[Scope::StateRead]).await;
    let first = subscribed_desktop(&mut session, |d| notes_width(d).is_some()).await;
    assert_eq!(notes_width(&first.1), Some(NOTES_WIDTH_BP));

    // Sway sends no event for a resize; only the poll can see it.
    harness
        .sway()
        .set_reply(Request::GetTree, fixtures::layout_tree_resized());

    let resized = next_desktop(&mut session, |d| {
        notes_width(d) == Some(NOTES_WIDTH_RESIZED_BP)
    });
    let within_one_poll = tokio::time::timeout(POLL_INTERVAL + DELIVERY_SLACK, resized).await;
    let (rev, _desktop) = within_one_poll.expect("the resized layout within one poll");
    assert!(rev > first.0);
}

#[tokio::test]
async fn icon_get_serves_a_known_hash_and_lists_an_unknown_one_as_missing() {
    let harness = Harness::start_with_icons("", show_layout_tree).await;
    let mut session = device_with(&harness, &[Scope::StateRead]).await;
    // The first state can be published before the icon job ends; a later one carries it.
    let (_rev, desktop) = subscribed_desktop(&mut session, has_notes_icon).await;
    let same_app = desktop.windows.iter().find(|window| window.id == 15);
    assert_eq!(same_app.map(|w| w.icon), Some(Some(Some(notes_hash()))));
    let unknown = IconHash::of(b"no such icon");

    let result = icon_get(&mut session, &[notes_hash(), unknown]).await;

    let result = result.expect("icon.get answered");
    let [icon] = result.icons.as_slice() else {
        panic!("one icon, got {}", result.icons.len());
    };
    assert_eq!(icon.hash, notes_hash());
    assert_eq!(
        icon.png,
        fixtures::NOTES_ICON_PNG,
        "a small PNG is served as it is"
    );
    icon.verify().expect("the PNG matches its hash");
    assert_eq!(result.missing, [unknown]);
}

#[tokio::test]
async fn icon_get_works_read_only_without_control_and_has_its_own_rate_class() {
    let harness = Harness::start_with_icons("read_only = true\n", show_layout_tree).await;
    let mut session = device_with(&harness, &[Scope::StateRead]).await;
    let welcome = session.welcome();
    assert!(welcome.read_only);
    assert!(welcome.ops.iter().any(|op| op == "icon.get"));
    assert!(!welcome.ops.iter().any(|op| op == "workspace.focus"));
    let known_ops = welcome.known_ops.as_deref().expect("known_ops");
    assert!(known_ops.iter().any(|op| op == "workspace.focus"));
    subscribed_desktop(&mut session, has_notes_icon).await;

    let mut served = 0;
    let refusal = loop {
        assert!(
            served < MAX_ICON_REQUESTS,
            "the icons bucket never ran empty"
        );
        match icon_get(&mut session, &[notes_hash()]).await {
            Ok(result) => {
                assert_eq!(result.icons.len(), 1, "read-only mode still serves icons");
                served += 1;
            }
            Err(ClientError::Failed(error)) => break error,
            Err(other) => panic!("unexpected failure: {other:?}"),
        }
    };

    assert!(
        served >= ICON_BURST,
        "a full bucket serves {ICON_BURST}, served {served}"
    );
    assert_eq!(refusal.code, ErrorCode::RateLimited);
    let detail = refusal.detail.expect("detail");
    let retry_after = detail["retry_after_ms"].as_u64().expect("retry_after_ms");
    assert!(
        (1..=500).contains(&retry_after),
        "one token refills in 500 ms: {retry_after}"
    );
    // The session itself is fine; only the `icons` class ran empty.
    soon("ping", session.request(&Args::Ping))
        .await
        .expect("pong");
}

#[tokio::test]
async fn a_grant_ends_the_session_with_scopes_changed_and_the_next_one_has_the_new_ops() {
    let harness = Harness::start().await;
    let profile = harness.pair(&[Scope::StateRead]).await;
    let mut session = harness.connect(&profile).await;
    assert!(
        !session
            .welcome()
            .ops
            .iter()
            .any(|op| op == "workspace.focus")
    );
    let prefix = &profile.device_id.as_str()[..8];

    let mut admin = harness.admin().await;
    let control = BTreeSet::from([Scope::Control]);
    let grant = admin.change_scopes(prefix, ScopeChange::Grant, &control);
    let changed = soon("devices grant", grant).await.expect("granted");

    assert!(changed.changed);
    assert_eq!(changed.device_id, profile.device_id.as_str());
    assert!(changed.scopes.iter().any(|scope| scope == "control"));
    assert_bye(
        soon("bye", session.next_event()).await,
        ByeCode::ScopesChanged,
    );
    let again = harness.connect(&profile).await;
    assert!(again.welcome().scopes.contains(&Scope::Control));
    assert!(again.welcome().ops.iter().any(|op| op == "workspace.focus"));
}
