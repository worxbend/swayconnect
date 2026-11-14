//! Tests of `audit.rs`.

use std::fs::{self, Permissions};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use serde_json::{Value, json};

use super::*;
use crate::paths::Problem;

fn phone() -> DeviceId {
    DeviceId::from_bytes([7; 16])
}

/// The log path inside a not yet existing data directory, and the uid of the temp dir.
fn log_in(root: &tempfile::TempDir) -> (PathBuf, u32) {
    let uid = fs::metadata(root.path()).expect("metadata").uid();
    (root.path().join("data").join(FILE_NAME), uid)
}

/// Every line of the file as JSON, with `time` checked and removed.
fn written(path: &Path) -> Vec<Value> {
    let content = fs::read_to_string(path).expect("read");
    assert!(content.ends_with('\n'), "every line ends with a newline");
    content
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).expect("one JSON object per line");
            let time = value
                .as_object_mut()
                .and_then(|object| object.remove("time"));
            assert!(time.is_some_and(|time| time.as_u64().is_some_and(|secs| secs > 0)));
            value
        })
        .collect()
}

#[test]
fn the_file_is_created_0600_in_a_private_data_dir_and_appended_across_opens() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    let lock = Record::new(
        phone(),
        None,
        Op::Request(Operation::SessionLock),
        Outcome::Ok,
    );

    AuditLog::open(&path, uid)
        .expect("open")
        .record(&lock)
        .expect("write");
    AuditLog::open(&path, uid)
        .expect("reopen")
        .record(&lock)
        .expect("write");

    assert_eq!(fs::metadata(&path).expect("metadata").mode() & 0o777, 0o600);
    let data_dir = path.parent().expect("parent");
    assert_eq!(fs::metadata(data_dir).expect("dir").mode() & 0o777, 0o700);
    assert_eq!(written(&path).len(), 2);
}

#[test]
fn a_device_action_line_holds_ids_and_codes_only() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    let log = AuditLog::open(&path, uid).expect("open");
    let peer: SocketAddr = "192.168.1.20:40000".parse().expect("address");

    let close = Record {
        targets: vec![42],
        ..Record::new(
            phone(),
            Some(peer),
            Op::Request(Operation::WindowClose),
            Outcome::Error(ErrorCode::Forbidden),
        )
    };
    log.record(&close).expect("write");

    let expected = json!({
        "device": phone().as_str(),
        "peer": "192.168.1.20:40000",
        "op": "window.close",
        "targets": [42],
        "result": "forbidden",
    });
    assert_eq!(written(&path), vec![expected]);
}

#[test]
fn keep_awake_and_typing_lines_carry_a_duration_and_a_length_never_text() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    let log = AuditLog::open(&path, uid).expect("open");
    let keep_awake = Op::Request(Operation::KeepAwakeSet);

    let on = Record {
        duration_s: Some(3600),
        ..Record::new(phone(), None, keep_awake, Outcome::Ok)
    };
    let off = Record::new(phone(), None, keep_awake, Outcome::Ok);
    // M2b has no input op yet; the length member is what an input line adds.
    let typed = Record {
        text_len: Some(11),
        targets: vec![9],
        ..Record::new(phone(), None, keep_awake, Outcome::Ok)
    };
    for record in [&on, &off, &typed] {
        log.record(record).expect("write");
    }

    let lines = written(&path);
    assert_eq!(lines[0]["duration_s"], 3600);
    assert_eq!(
        lines[1],
        json!({"device": phone().as_str(), "op": "keep_awake.set", "result": "ok"})
    );
    assert_eq!(lines[2]["text_len"], 11);
    let mut members: Vec<&str> = lines[2]
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    members.sort_unstable();
    assert_eq!(members, ["device", "op", "result", "targets", "text_len"]);
}

#[test]
fn admin_lines_name_the_op_the_scopes_and_the_outcome() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    let log = AuditLog::open(&path, uid).expect("open");
    let scopes = BTreeSet::from([Scope::StateRead, Scope::IdleInhibit]);

    let cases = [
        (Op::Pair, Outcome::Ok, "pair", "ok"),
        (Op::Grant, Outcome::Ok, "devices.grant", "ok"),
        (Op::Deny, Outcome::Unchanged, "devices.deny", "unchanged"),
        (
            Op::Revoke,
            Outcome::NotStored,
            "devices.revoke",
            "not_stored",
        ),
    ];
    for (op, outcome, _, _) in cases {
        let record = Record {
            scopes: scopes.clone(),
            ..Record::new(phone(), None, op, outcome)
        };
        log.record(&record).expect("write");
    }

    let lines = written(&path);
    for (line, (_, _, op, result)) in lines.iter().zip(cases) {
        assert_eq!(line["op"], op);
        assert_eq!(line["result"], result);
        assert_eq!(line["scopes"], json!(["state.read", "idle.inhibit"]));
        assert!(line.get("peer").is_none(), "the admin socket has no peer");
    }
}

#[test]
fn clones_append_to_one_file_in_call_order() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    let first = AuditLog::open(&path, uid).expect("open");
    let second = first.clone();

    for (log, id) in [(&first, 1), (&second, 2), (&first, 3)] {
        let record = Record {
            targets: vec![id],
            ..Record::new(
                phone(),
                None,
                Op::Request(Operation::WindowMove),
                Outcome::Ok,
            )
        };
        log.record(&record).expect("write");
    }

    let ids: Vec<Value> = written(&path)
        .iter()
        .map(|line| line["targets"][0].clone())
        .collect();
    assert_eq!(ids, [json!(1), json!(2), json!(3)]);
}

#[test]
fn a_symlink_is_refused_and_never_followed() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    ensure_private_dir(path.parent().expect("parent"), uid).expect("data dir");
    let target = root.path().join("elsewhere.jsonl");
    std::os::unix::fs::symlink(&target, &path).expect("symlink");

    assert!(AuditLog::open(&path, uid).is_err());
    assert!(!target.exists(), "the dangling target was not created");
}

#[test]
fn a_log_readable_by_others_is_refused_for_writing_and_reading() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    AuditLog::open(&path, uid).expect("open");
    fs::set_permissions(&path, Permissions::from_mode(0o644)).expect("chmod");

    let is_loose = |error: PathError| {
        matches!(
            error,
            PathError::Refused {
                problem: Problem::LooseMode,
                ..
            }
        )
    };
    assert!(AuditLog::open(&path, uid).is_err_and(is_loose));
    assert!(newest_lines_for_terminal(&path, uid, 5).is_err_and(is_loose));
}

#[test]
fn newest_lines_cover_missing_empty_fewer_and_more_than_asked() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    assert_eq!(
        newest_lines_for_terminal(&path, uid, 5).expect("missing"),
        Vec::<String>::new()
    );

    let log = AuditLog::open(&path, uid).expect("open");
    assert!(
        newest_lines_for_terminal(&path, uid, 5)
            .expect("empty")
            .is_empty()
    );
    for id in 1..=3 {
        let record = Record {
            targets: vec![id],
            ..Record::new(
                phone(),
                None,
                Op::Request(Operation::WindowClose),
                Outcome::Ok,
            )
        };
        log.record(&record).expect("write");
    }

    let count = |asked| {
        newest_lines_for_terminal(&path, uid, asked)
            .expect("read")
            .len()
    };
    assert_eq!(count(0), 0);
    assert_eq!(count(5), 3);
    let newest_two = newest_lines_for_terminal(&path, uid, 2).expect("read");
    assert!(newest_two[0].contains("\"targets\":[2]"));
    assert!(newest_two[1].contains("\"targets\":[3]"));
}

#[test]
fn printed_lines_are_escaped_for_the_terminal() {
    let root = tempfile::tempdir().expect("temp dir");
    let (path, uid) = log_in(&root);
    AuditLog::open(&path, uid).expect("open");
    // The daemon never writes these; a line planted by another program of the owner must not
    // drive the terminal (ESC sequence, right-to-left override, invalid UTF-8).
    let planted = b"{\"op\":\"\x1b[2J\xe2\x80\xaeevil\xff\"}\n";
    fs::write(&path, planted).expect("plant");

    let lines = newest_lines_for_terminal(&path, uid, 5).expect("read");
    assert_eq!(lines, ["{\"op\":\"\\u{1b}[2J\\u{202e}evil\u{fffd}\"}"]);
}
