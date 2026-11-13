//! The daemon keeps serving clients while sway is gone or hangs, and says so in the state
//! (`docs/PROTOCOL.md` 9.1, `docs/DESIGN.md` 4.3).

mod support;

use serde_json::json;
use support::{EVERY_SCOPE, Harness, next_desktop, online_desktop, soon};
use swayconnect_protocol::Desktop;
use swayconnect_protocol::ops::Args;
use swayconnect_testkit::Event;

fn assert_offline(desktop: &Desktop) {
    assert_eq!(desktop.sway.version, None);
    assert!(desktop.workspaces.is_empty() && desktop.windows.is_empty());
}

#[tokio::test]
async fn sway_going_away_and_coming_back_is_reported_to_a_subscriber() {
    let mut harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    let (first_rev, _desktop) = online_desktop(&mut session).await;

    harness.stop_sway();
    let (offline_rev, offline) = next_desktop(&mut session, |desktop| !desktop.sway.online).await;
    assert!(offline_rev > first_rev);
    assert_offline(&offline);
    soon("ping", session.request(&Args::Ping))
        .await
        .expect("the session outlives sway");

    harness.restart_sway();
    let (online_rev, online) = next_desktop(&mut session, |desktop| desktop.sway.online).await;
    assert!(online_rev > offline_rev);
    assert_eq!(online.workspaces.len(), 2);
    assert_eq!(online.windows.len(), 2);
}

#[tokio::test]
async fn a_sway_that_stops_answering_is_reported_offline_until_it_answers_again() {
    let harness = Harness::start().await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    let mut session = harness.connect(&profile).await;
    let (first_rev, _desktop) = online_desktop(&mut session).await;

    harness.sway().stop_answering();
    // The event makes the daemon ask for a tree that never comes.
    harness
        .sway()
        .send_event(Event::Window, json!({"change": "focus"}).to_string());
    let (offline_rev, offline) = next_desktop(&mut session, |desktop| !desktop.sway.online).await;
    assert!(offline_rev > first_rev);
    assert_offline(&offline);

    harness.sway().resume_answering();
    let (online_rev, online) = next_desktop(&mut session, |desktop| desktop.sway.online).await;
    assert!(online_rev > offline_rev);
    assert_eq!(online.windows.len(), 2);
    assert_eq!(harness.sway().commands(), Vec::<String>::new());
}
