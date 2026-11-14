//! Tests of the section 17 additions in `session.rs`: `bye scopes_changed`, `icon.get`, and
//! titles of scratchpad windows.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};
use swayconnect_protocol::fingerprint::encode_base64url;
use swayconnect_protocol::{IconHash, ScratchpadWindow};
use swayconnect_testkit::fixtures::{NOTES_APP_ID, NOTES_ICON_PNG};

use super::harness::{Daemon, EVERY_SCOPE, ping, published, subscribe};
use super::*;
use crate::devices::ScopeChange;
use crate::icons::raster::ChildRasteriser;
use crate::icons::{IconKey, IconSettings, IconsConfig};
use crate::paths::own_uid;

const SCRATCHPAD_TITLE: &str = "Hidden shopping list";

fn ops_of(welcome: &JsonObject) -> Vec<&str> {
    let ops = welcome["ops"].as_array().expect("ops");
    ops.iter().map(|op| op.as_str().expect("op name")).collect()
}

fn icon_get(id: u64, hashes: &[String]) -> Value {
    json!({"type": "req", "id": id, "op": "icon.get", "args": {"hashes": hashes}})
}

#[tokio::test]
async fn a_grant_ends_the_session_with_bye_scopes_changed_and_the_next_welcome_has_new_ops() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, welcome) = daemon.ready(&device).await;
    assert!(!ops_of(&welcome).contains(&"window.focus"));

    let control = BTreeSet::from([Scope::Control]);
    let prefix = &device.id.as_str()[..8];
    let security = &daemon.shared.security;
    let set = security
        .change_scopes(prefix, ScopeChange::Grant, &control)
        .expect("granted");
    assert!(set.changed);
    let bye = client.receive().await;
    let expected = json!({"type": "bye", "code": "scopes_changed",
        "message": "scopes of this device changed"});
    assert_eq!(Value::Object(bye), expected);

    let (_client, welcome) = daemon.ready(&device).await;
    assert_eq!(welcome["scopes"], json!(["state.read", "control"]));
    assert!(ops_of(&welcome).contains(&"window.focus"));
}

#[tokio::test]
async fn a_grant_that_changes_nothing_leaves_the_session_open() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    let held = BTreeSet::from([Scope::Control]);
    let prefix = &device.id.as_str()[..8];
    let security = &daemon.shared.security;
    let set = security
        .change_scopes(prefix, ScopeChange::Grant, &held)
        .expect("no change");
    assert!(!set.changed);
    client.send(&ping(1)).await;
    assert_eq!(client.receive().await["ok"], true);
}

#[tokio::test]
async fn without_an_icon_service_every_hash_is_missing() {
    let daemon = Daemon::start().await;
    let device = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, _) = daemon.ready(&device).await;
    let hash = IconHash::of(NOTES_ICON_PNG).to_string();
    client.send(&icon_get(1, std::slice::from_ref(&hash))).await;
    let response = client.receive().await;
    assert_eq!(response["result"], json!({"icons": [], "missing": [hash]}));
}

#[tokio::test]
async fn icon_get_answers_from_the_icon_service_in_request_order() {
    let mut daemon = Daemon::start().await;
    let dirs = &daemon.dirs;
    let settings = IconSettings {
        search_dirs: vec![dirs.data_home(), dirs.data_dirs()],
        cache_dir: None,
        uid: own_uid().expect("uid"),
        proc_root: dirs.root().join("proc"),
        config: IconsConfig::default(),
    };
    // The fixture PNG is small enough to be served as it is: no raster child runs.
    let rasteriser = ChildRasteriser::new(PathBuf::from("/nonexistent/swayconnectd"));
    let service = IconService::start(settings, rasteriser).expect("icon service");
    let notes = IconKey {
        app_id: Some(NOTES_APP_ID.to_owned()),
        ..IconKey::default()
    };
    let mut finished = service.finished();
    assert_eq!(service.icon_for(&notes, None), None, "queued, not ready");
    finished.wait_for(|jobs| *jobs >= 1).await.expect("worker");
    let produced = service.icon_for(&notes, None).expect("the fixture icon");
    assert_eq!(produced, IconHash::of(NOTES_ICON_PNG));
    daemon.shared.icons = Some(Arc::new(service));

    let device = daemon.paired_device(&[Scope::StateRead]).await;
    let (mut client, _) = daemon.ready(&device).await;
    let unknown = encode_base64url(&[0; 16]);
    client
        .send(&icon_get(3, &[unknown.clone(), produced.to_string()]))
        .await;
    let response = client.receive().await;
    let expected = json!({
        "icons": [{"hash": produced.to_string(), "png": encode_base64url(NOTES_ICON_PNG)}],
        "missing": [unknown],
    });
    assert_eq!(response["result"], expected);
}

fn published_with_scratchpad(rev: u64) -> Arc<Published> {
    let mut desktop = published(rev).desktop.clone();
    desktop.scratchpad = Some(vec![ScratchpadWindow {
        id: 9,
        app_id: Some("notes".to_owned()),
        title: Some(SCRATCHPAD_TITLE.to_owned()),
        icon: None,
    }]);
    Arc::new(Published { rev, desktop })
}

#[tokio::test]
async fn scratchpad_titles_reach_only_a_device_with_state_titles() {
    let daemon = Daemon::start().await;
    daemon.desktop.send_replace(published_with_scratchpad(2));
    let reader = daemon.paired_device(&[Scope::StateRead]).await;
    let titled = daemon.paired_device(&EVERY_SCOPE).await;
    let title_of = |response: &JsonObject| {
        response["result"]["states"][0]["data"]["scratchpad"][0]["title"].clone()
    };

    let (mut client, _) = daemon.ready(&reader).await;
    client.send(&subscribe(1)).await;
    assert_eq!(title_of(&client.receive().await), Value::Null);

    let (mut client, _) = daemon.ready(&titled).await;
    client.send(&subscribe(1)).await;
    assert_eq!(title_of(&client.receive().await), json!(SCRATCHPAD_TITLE));
}

#[tokio::test(start_paused = true)]
async fn icon_get_has_its_own_bucket_per_device_also_in_read_only_mode() {
    let daemon = Daemon::start().await;
    daemon.shared.read_only.store(true, Ordering::Relaxed);
    let device = daemon.paired_device(&EVERY_SCOPE).await;
    let (mut client, _) = daemon.ready(&device).await;
    let hashes = [encode_base64url(&[0; 16])];
    for id in 1..=4 {
        client.send(&icon_get(id, &hashes)).await;
        assert_eq!(client.receive().await["ok"], true, "token {id}");
    }
    client.send(&icon_get(5, &hashes)).await;
    let refused = client.receive().await;
    assert_eq!(refused["error"]["code"], "rate_limited");
    assert_eq!(refused["error"]["detail"], json!({"retry_after_ms": 500}));

    // The `actions` bucket of the device is still full.
    let security = &daemon.shared.security;
    for _ in 0..5 {
        assert_eq!(security.take_action_token(&device.id), Ok(()));
    }
    tokio::time::advance(Duration::from_millis(500)).await;
    client.send(&icon_get(6, &hashes)).await;
    assert_eq!(client.receive().await["ok"], true);
}
