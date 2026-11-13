//! Tests of `devices.rs`.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::time::Duration;

use super::*;

const KEY: [u8; 32] = [0xAB; 32];
const KEY_TEXT: &str = "q6urq6urq6urq6urq6urq6urq6urq6urq6urq6urq6s";

fn device(id_byte: u8) -> Device {
    Device {
        id: DeviceId::from_bytes([id_byte; 16]),
        name: "phone".to_owned(),
        platform: Some("android".to_owned()),
        client: None,
        key: Secret::new(KEY),
        scopes: BTreeSet::from([Scope::StateRead, Scope::Control]),
        created_at: 1_800_000_000,
    }
}

fn store_in(root: &tempfile::TempDir) -> (DeviceStore, PathBuf, u32) {
    let file = root.path().join("devices.json");
    let uid = fs::metadata(root.path()).expect("metadata").uid();
    let store = DeviceStore::load(file.clone(), uid).expect("empty store");
    (store, file, uid)
}

fn format_error(file: PathBuf, uid: u32, content: &str) -> String {
    write_atomic(&file, content.as_bytes()).expect("write");
    match DeviceStore::load(file, uid) {
        Err(StoreError::Format { reason, .. }) => reason,
        other => panic!("expected a format error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_stored_device_survives_a_reload_in_the_documented_format() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, file, uid) = store_in(&root);
    assert!(!file.exists());
    let (revoked, _receiver) = watch::channel(false);
    store.insert(device(1), revoked).expect("insert");

    let text = fs::read_to_string(&file).expect("file");
    let json: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(json["version"], 1);
    assert_eq!(json["devices"][0]["key"], KEY_TEXT);
    assert_eq!(json["devices"][0]["scopes"][1], "control");
    assert_eq!(fs::metadata(&file).expect("metadata").mode() & 0o777, 0o600);

    let mut reloaded = DeviceStore::load(file, uid).expect("reload");
    let id = DeviceId::from_bytes([1; 16]);
    let entry = reloaded.get_mut(&id).expect("device");
    assert_eq!(entry.device.key.bytes(), &KEY);
    assert_eq!(entry.device.created_at, 1_800_000_000);
    assert_eq!(reloaded.list()[0].connections, 0);
    assert_eq!(reloaded.list()[0].name, "phone");
}

#[tokio::test]
async fn revoke_signals_receivers_created_before_and_after_it() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, file, uid) = store_in(&root);
    let (revoked, mut before) = watch::channel(false);
    store.insert(device(1), revoked).expect("insert");
    store
        .insert(device(2), watch::channel(false).0)
        .expect("insert");
    let id = DeviceId::from_bytes([1; 16]);
    assert_eq!(store.list()[0].connections, 1);

    assert!(store.revoke(&id).expect("revoke"));

    assert!(!store.contains(&id));
    before.changed().await.expect("the revoke is a change");
    assert!(*before.borrow());
    let after = before.clone();
    assert!(*after.borrow());
    assert!(!store.revoke(&id).expect("second revoke"));
    let reloaded = DeviceStore::load(file, uid).expect("reload");
    assert!(!reloaded.contains(&id));
    assert!(reloaded.contains(&DeviceId::from_bytes([2; 16])));
}

#[tokio::test]
async fn a_failed_write_adds_nothing_and_closes_the_revoke_channel() {
    let root = tempfile::tempdir().expect("temp dir");
    let uid = fs::metadata(root.path()).expect("metadata").uid();
    let missing_dir = root.path().join("missing").join("devices.json");
    let mut store = DeviceStore::load(missing_dir, uid).expect("empty store");
    let (revoked, mut receiver) = watch::channel(false);

    assert!(store.insert(device(1), revoked).is_err());

    assert!(store.list().is_empty());
    assert!(receiver.changed().await.is_err());
}

#[tokio::test]
async fn a_bad_file_is_refused_without_echoing_the_key() {
    let root = tempfile::tempdir().expect("temp dir");
    let (_, file, uid) = store_in(&root);
    let entry = |id: &str, key: &str, scope: &str| {
        format!(
            r#"{{"id":"{id}","name":"n","platform":null,"client":null,"key":"{key}","scopes":["{scope}"],"created_at":1}}"#
        )
    };
    let id = DeviceId::from_bytes([1; 16]);
    let good = entry(id.as_str(), KEY_TEXT, "control");
    let cases = [
        (r#"{"version":2,"devices":[]}"#.to_owned(), "version"),
        (
            format!(r#"{{"version":1,"devices":[{good},{good}]}}"#),
            "twice",
        ),
        (
            format!(
                r#"{{"version":1,"devices":[{}]}}"#,
                entry("short", KEY_TEXT, "control")
            ),
            "invalid id",
        ),
        (
            format!(
                r#"{{"version":1,"devices":[{}]}}"#,
                entry(id.as_str(), "SECRETKEY", "control")
            ),
            "invalid key",
        ),
        (
            format!(
                r#"{{"version":1,"devices":[{}]}}"#,
                entry(id.as_str(), KEY_TEXT, "root")
            ),
            "unknown scope",
        ),
        (
            r#"{"version":1,"devices":[],"extra":1}"#.to_owned(),
            "extra",
        ),
    ];
    for (content, expected) in cases {
        let reason = format_error(file.clone(), uid, &content);
        assert!(reason.contains(expected), "{reason}");
        assert!(!reason.contains("SECRETKEY") && !reason.contains(KEY_TEXT));
    }
}

#[test]
fn debug_output_redacts_the_key() {
    let text = format!("{:?}", device(1));
    assert!(text.contains("<redacted>"));
    assert!(!text.contains("171") && !text.to_lowercase().contains("ab, ab"));
}

fn device_with_id(bytes: [u8; 16]) -> Device {
    let mut device = device(0);
    device.id = DeviceId::from_bytes(bytes);
    device
}

fn grant(
    store: &mut DeviceStore,
    prefix: &str,
    change: ScopeChange,
    scopes: &[Scope],
) -> Result<ScopesSet, ChangeError> {
    store.change_scopes(prefix, change, &scopes.iter().copied().collect())
}

#[tokio::test]
async fn grant_and_deny_write_the_file_then_signal_the_new_scopes() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, file, uid) = store_in(&root);
    store
        .insert(device(1), watch::channel(false).0)
        .expect("insert");
    let id = DeviceId::from_bytes([1; 16]);
    let mut watch = store.scope_watch(&id).expect("stored device");
    let prefix = &id.as_str()[..MIN_PREFIX_CHARS];

    let granted = grant(
        &mut store,
        prefix,
        ScopeChange::Grant,
        &[Scope::StateTitles],
    )
    .expect("grant");

    let expected = BTreeSet::from([Scope::StateRead, Scope::StateTitles, Scope::Control]);
    assert_eq!(granted.device_id, id);
    assert_eq!(granted.scopes, expected);
    assert!(granted.changed);
    assert!(watch.has_changed().expect("open"));
    assert_eq!(*watch.borrow_and_update(), expected);
    let mut reloaded = DeviceStore::load(file.clone(), uid).expect("reload");
    let entry = reloaded.get_mut(&id).expect("device");
    assert_eq!(entry.device.scopes, expected);
    assert_eq!(*entry.scopes_signal.borrow(), expected);

    let denied = grant(
        &mut store,
        id.as_str(),
        ScopeChange::Deny,
        &[Scope::Control, Scope::StateTitles],
    )
    .expect("deny");
    assert_eq!(denied.scopes, BTreeSet::from([Scope::StateRead]));
    assert_eq!(*watch.borrow_and_update(), denied.scopes);
    assert_eq!(store.list()[0].scopes, ["state.read"]);
    let reloaded = DeviceStore::load(file, uid).expect("reload");
    assert_eq!(reloaded.list()[0].scopes, ["state.read"]);
}

#[tokio::test]
async fn a_change_that_changes_nothing_writes_and_signals_nothing() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, file, _) = store_in(&root);
    store
        .insert(device(1), watch::channel(false).0)
        .expect("insert");
    let id = DeviceId::from_bytes([1; 16]);
    let watch = store.scope_watch(&id).expect("stored device");
    fs::remove_file(&file).expect("remove");

    let granted = grant(
        &mut store,
        id.as_str(),
        ScopeChange::Grant,
        &[Scope::Control],
    )
    .expect("grant");
    let denied = grant(
        &mut store,
        id.as_str(),
        ScopeChange::Deny,
        &[Scope::StateTitles],
    )
    .expect("deny");

    assert!(!granted.changed && !denied.changed);
    assert_eq!(granted.scopes, device(1).scopes);
    assert!(!watch.has_changed().expect("open"));
    assert!(!file.exists(), "nothing was written");
}

#[tokio::test]
async fn a_prefix_must_name_exactly_one_device() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, _, _) = store_in(&root);
    let mut near = [7; 16];
    near[15] = 8;
    for bytes in [[7; 16], near, [1; 16]] {
        store
            .insert(device_with_id(bytes), watch::channel(false).0)
            .expect("insert");
    }
    let first = DeviceId::from_bytes([7; 16]);
    let second = DeviceId::from_bytes(near);
    let shared = &first.as_str()[..MIN_PREFIX_CHARS];
    assert!(second.as_str().starts_with(shared));
    let grant_titles = |store: &mut DeviceStore, prefix: &str| {
        grant(store, prefix, ScopeChange::Grant, &[Scope::StateTitles])
    };

    for bad in ["", "Bwc", "Bwc!", "Bwc=", "Bwc Bwc", "Bwcé"] {
        let outcome = grant_titles(&mut store, bad);
        assert!(
            matches!(outcome, Err(ChangeError::BadPrefix)),
            "{bad:?}: {outcome:?}"
        );
    }
    let outcome = grant_titles(&mut store, "zzzz");
    assert!(matches!(outcome, Err(ChangeError::NotFound)), "{outcome:?}");
    match grant_titles(&mut store, shared) {
        Err(ChangeError::Clash(ids)) => {
            assert_eq!(ids, [first.as_str(), second.as_str()]);
            let message = ChangeError::Clash(ids).to_string();
            assert!(message.contains(first.as_str()) && message.contains(second.as_str()));
        }
        other => panic!("expected a clash, got {other:?}"),
    }
    let longer = &second.as_str()[..second.as_str().len() - 1];
    assert!(!first.as_str().starts_with(longer));
    let set = grant_titles(&mut store, longer).expect("unique prefix");
    assert_eq!(set.device_id, second);
    assert!(store.get_mut(&first).expect("first").device.scopes.len() == 2);
}

#[tokio::test]
async fn a_failed_write_changes_no_scope_and_sends_no_signal() {
    let root = tempfile::tempdir().expect("temp dir");
    let (mut store, file, _) = store_in(&root);
    store
        .insert(device(1), watch::channel(false).0)
        .expect("insert");
    let id = DeviceId::from_bytes([1; 16]);
    let watch = store.scope_watch(&id).expect("stored device");
    // `write_atomic` replaces the file through a rename; a directory in its place fails it.
    fs::remove_file(&file).expect("remove");
    fs::create_dir(&file).expect("directory in the way");

    let outcome = grant(
        &mut store,
        id.as_str(),
        ScopeChange::Grant,
        &[Scope::StateTitles],
    );

    assert!(matches!(outcome, Err(ChangeError::Store(_))), "{outcome:?}");
    assert_eq!(store.list()[0].scopes, ["state.read", "control"]);
    assert!(!watch.has_changed().expect("open"));
}

#[tokio::test]
async fn scopes_changed_sees_a_change_made_before_the_session_subscribed() {
    let held = BTreeSet::from([Scope::StateRead]);
    let (signal, mut watch) = watch::channel(held.clone());
    let wait = Duration::from_millis(50);

    assert!(
        tokio::time::timeout(wait, scopes_changed(&mut watch, &held))
            .await
            .is_err()
    );

    signal.send_replace(BTreeSet::from([Scope::StateRead, Scope::Control]));
    let mut late = signal.subscribe();
    tokio::time::timeout(wait, scopes_changed(&mut late, &held))
        .await
        .expect("a value that differs from the held scopes resolves at once");

    drop(signal);
    let mut gone = watch::channel(held.clone()).1;
    assert!(
        tokio::time::timeout(wait, scopes_changed(&mut gone, &held))
            .await
            .is_err(),
        "a closed channel is left to the revoke signal"
    );
}
