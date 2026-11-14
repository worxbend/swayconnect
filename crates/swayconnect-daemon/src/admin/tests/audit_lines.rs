//! Audit lines written by the admin socket (`docs/DESIGN.md` 11.8): one per pairing, scope
//! change and revoke.

use serde_json::{Value, json};

use super::*;

fn audit_lines(paths: &Paths) -> Vec<Value> {
    let content = fs::read_to_string(audit_file(paths)).expect("audit log");
    content
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect()
}

/// `op`, `result`, `scopes` and `peer` of each line; `device` must name `device`.
fn summary(lines: &[Value], device: &DeviceId) -> Vec<Value> {
    let pick = |line: &Value| {
        assert_eq!(line["device"], device.as_str());
        json!([line["op"], line["result"], line["scopes"], line["peer"]])
    };
    lines.iter().map(pick).collect()
}

#[tokio::test]
async fn pairing_scope_changes_and_revoke_each_write_one_line() {
    let fixture = Fixture::start();
    assert_eq!(mode_of(&audit_file(&fixture.paths)), 0o600);
    let device = fixture.paired_device().await;
    let mut client = fixture.client().await;
    let control = BTreeSet::from([Scope::Control]);

    client
        .change_scopes(device.as_str(), ScopeChange::Grant, &control)
        .await
        .expect("grant");
    client
        .change_scopes(device.as_str(), ScopeChange::Grant, &control)
        .await
        .expect("grant again");
    client
        .change_scopes(device.as_str(), ScopeChange::Deny, &control)
        .await
        .expect("deny");
    // A refused change names no device and writes nothing.
    let refused = client
        .change_scopes("zzzz", ScopeChange::Grant, &control)
        .await;
    assert!(refused.is_err());
    assert!(client.revoke(&device).await.expect("revoke"));
    assert!(!client.revoke(&device).await.expect("second revoke"));

    let lines = audit_lines(&fixture.paths);
    let both = json!(["state.read", "control"]);
    let read = json!(["state.read"]);
    assert_eq!(
        summary(&lines, &device),
        [
            json!(["pair", "ok", read, "127.0.0.1:50000"]),
            json!(["devices.grant", "ok", both, null]),
            json!(["devices.grant", "unchanged", both, null]),
            json!(["devices.deny", "ok", read, null]),
            json!(["devices.revoke", "ok", null, null]),
        ]
    );
    let text = fs::read_to_string(audit_file(&fixture.paths)).expect("audit log");
    assert!(!text.contains("Pixel"), "the device name is not recorded");
}

#[tokio::test]
async fn a_refused_pairing_writes_no_line() {
    let fixture = Fixture::start();
    let mut client = fixture.client().await;
    let uri = client.open_pairing(&request(None)).await.expect("window");
    // Held until the answer: dropping it would end the pairing as `client_gone`.
    let _admitted = fixture.present(&uri).expect("pair");
    client
        .pending_device()
        .await
        .expect("reply")
        .expect("pending");
    client.answer(false).await.expect("answer");
    assert_eq!(
        client.pairing_result().await.expect("result"),
        PairingResult::Refused
    );

    assert!(audit_lines(&fixture.paths).is_empty());
}

#[tokio::test]
async fn a_loose_audit_log_stops_the_admin_socket_from_starting() {
    let (_root, paths) = temp_paths();
    let uid = own_uid().expect("uid");
    AuditLog::open(&audit_file(&paths), uid).expect("open");
    fs::set_permissions(audit_file(&paths), Permissions::from_mode(0o640)).expect("chmod");

    let bound = AdminServer::bind(&paths);
    let is_loose = matches!(
        bound,
        Err(StartError::Path(PathError::Refused {
            problem: Problem::LooseMode,
            ..
        }))
    );
    assert!(is_loose, "a loose audit log is refused");
    assert!(!paths.admin_socket().exists(), "no socket is left behind");
}
