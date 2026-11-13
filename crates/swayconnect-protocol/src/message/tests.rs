use serde_json::{Value, json};

use super::*;

const NONCE: &str = "oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8";
const PROOF: &str = "o1AVbXOYmXPduQKXp6_NJPrlOnX8jICYQ-zp9yWhUuQ";
const DEVICE_ID: &str = "wMHCw8TFxsfIycrLzM3Ozw";

fn decode(value: Value) -> Result<Message, DecodeError> {
    let Value::Object(object) = value else {
        panic!("test message is not an object");
    };
    Message::decode(&object)
}

fn invalid(member: &'static str) -> Result<Message, DecodeError> {
    Err(DecodeError::Invalid(InvalidMember(member)))
}

fn auth() -> Value {
    json!({"type": "auth", "protocol": 1, "device_id": DEVICE_ID, "nonce": NONCE, "proof": PROOF})
}

fn pair() -> Value {
    json!({"type": "pair", "protocol": 1, "nonce": NONCE, "proof": PROOF, "device_name": "Pixel 9"})
}

fn welcome() -> Value {
    json!({
        "type": "welcome", "protocol": 1, "device_id": DEVICE_ID, "server_proof": PROOF,
        "scopes": ["state.read"], "ops": ["ping"], "topics": ["desktop"], "read_only": false,
        "server": {"name": "desk", "version": "0.1.0"},
        "limits": {"max_frame_c2s": 16384, "max_frame_s2c": 1048576, "idle_timeout_ms": 90000}
    })
}

fn with(mut message: Value, member: &str, value: Value) -> Value {
    message[member] = value;
    message
}

#[test]
fn type_errors_are_told_apart() {
    assert_eq!(decode(json!({"id": 1})), Err(DecodeError::NoType));
    assert_eq!(decode(json!({"type": 5})), Err(DecodeError::NoType));
    assert_eq!(decode(json!({"type": null})), Err(DecodeError::NoType));
    assert_eq!(
        decode(json!({"type": "note"})),
        Err(DecodeError::UnknownType)
    );
    assert_eq!(decode(json!({"type": "req", "op": "ping"})), invalid("id"));
}

#[test]
fn null_is_a_wrong_type_in_every_optional_member() {
    assert_eq!(
        decode(with(auth(), "client", Value::Null)),
        invalid("client")
    );
    assert_eq!(
        decode(with(pair(), "client", Value::Null)),
        invalid("client")
    );
    assert_eq!(
        decode(with(pair(), "platform", Value::Null)),
        invalid("platform")
    );
    assert_eq!(
        decode(with(welcome(), "confirm_code", Value::Null)),
        invalid("confirm_code")
    );
    let failed = json!({"type": "res", "id": 1, "ok": false,
        "error": {"code": "internal", "message": "m", "detail": null}});
    assert_eq!(decode(failed), invalid("detail"));
}

#[test]
fn device_id_must_be_canonical_base64url_of_16_bytes() {
    for bad in [
        "wMHCw8TFxsfIycrLzM3Ozx",
        "wMHCw8TFxsfIycrLzM3Ozw==",
        "wMHCw8TFxsfIycrLzM3O",
        "",
    ] {
        assert_eq!(
            decode(with(auth(), "device_id", json!(bad))),
            invalid("device_id"),
            "{bad}"
        );
        assert!(DeviceId::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(
        DeviceId::parse(DEVICE_ID).map(|id| id.as_str().to_owned()),
        Ok(DEVICE_ID.to_owned())
    );
    let from_bytes = DeviceId::from_bytes([
        0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD, 0xCE,
        0xCF,
    ]);
    assert_eq!(from_bytes.as_str(), DEVICE_ID);
}

#[test]
fn int_has_no_fraction_no_exponent_no_sign_and_fits_53_bits() {
    let bad_ints = [
        "1.0",
        "1e0",
        "-1",
        "\"1\"",
        "true",
        "null",
        "9007199254740992",
    ];
    for bad in bad_ints {
        let text = format!(r#"{{"type":"req","id":{bad},"op":"ping"}}"#);
        let object: JsonObject = serde_json::from_str(&text).expect("test JSON");
        assert_eq!(Message::decode(&object), invalid("id"), "{bad}");
    }
    let largest = json!({"type": "req", "id": 9_007_199_254_740_991_u64, "op": "ping"});
    assert!(decode(largest).is_ok());
}

#[test]
fn a_protocol_number_over_u32_is_a_valid_int_outside_every_range() {
    let Ok(Message::Auth(auth)) = decode(with(auth(), "protocol", json!(5_000_000_000_u64))) else {
        panic!("a large protocol number must decode");
    };
    assert_eq!(auth.protocol, u32::MAX);
}

#[test]
fn bytes32_members_are_checked() {
    for member in ["nonce", "proof"] {
        for bad in [json!("AAAA"), json!(5), json!(format!("{NONCE}="))] {
            assert_eq!(decode(with(auth(), member, bad)), invalid(member));
        }
    }
}

#[test]
fn hello_needs_min_at_most_max() {
    let hello = |min: u32, max: u32| {
        decode(json!({"type": "hello", "protocol": {"min": min, "max": max}, "nonce": NONCE}))
    };
    assert!(hello(1, 1).is_ok());
    assert!(hello(1, 3).is_ok());
    assert_eq!(hello(2, 1), invalid("protocol"));
}

#[test]
fn req_keeps_args_of_any_type_for_dispatch() {
    let Ok(Message::Req(absent)) = decode(json!({"type": "req", "id": 1, "op": "ping"})) else {
        panic!("req must decode");
    };
    assert_eq!(absent.args, None);
    let null_args = json!({"type": "req", "id": 1, "op": "ping", "args": null});
    let Ok(Message::Req(null_args)) = decode(null_args) else {
        panic!("req with null args must decode");
    };
    assert_eq!(null_args.args, Some(Value::Null));
}

#[test]
fn res_has_exactly_one_of_result_and_error() {
    let error = json!({"code": "internal", "message": "m"});
    assert_eq!(
        decode(json!({"type": "res", "id": 1, "ok": true})),
        invalid("result")
    );
    assert_eq!(
        decode(json!({"type": "res", "id": 1, "ok": false})),
        invalid("error")
    );
    let both = json!({"type": "res", "id": 1, "ok": true, "result": {}, "error": error});
    assert_eq!(decode(both.clone()), invalid("error"));
    assert_eq!(decode(with(both, "ok", json!(false))), invalid("result"));
    assert_eq!(
        decode(json!({"type": "res", "id": 1, "ok": 1, "result": {}})),
        invalid("ok")
    );
}

#[test]
fn unknown_codes_and_scopes_follow_the_receiver_rules() {
    let res = json!({"type": "res", "id": 1, "ok": false,
        "error": {"code": "quota_exceeded", "message": "m"}});
    let Ok(Message::Res(res)) = decode(res) else {
        panic!("res must decode");
    };
    assert_eq!(res.error.map(|error| error.code), Some(ErrorCode::Unknown));

    let bye = decode(json!({"type": "bye", "code": "maintenance", "message": "m"}));
    assert!(matches!(
        bye,
        Ok(Message::Bye(Bye {
            code: ByeCode::Shutdown,
            ..
        }))
    ));

    let reject = decode(json!({"type": "reject", "code": "banned", "message": "m"}));
    assert!(matches!(
        reject,
        Ok(Message::Reject(Reject {
            code: RejectCode::BadMessage,
            ..
        }))
    ));

    let scopes = json!(["state.read", "media.control", "control"]);
    let Ok(Message::Welcome(welcome)) = decode(with(welcome(), "scopes", scopes)) else {
        panic!("welcome must decode");
    };
    assert_eq!(welcome.scopes, vec![Scope::StateRead, Scope::Control]);
}

#[test]
fn welcome_members_are_validated() {
    assert!(decode(with(welcome(), "confirm_code", json!("261988"))).is_ok());
    for bad in ["26198", "2619888", "26198a", "\u{1b}[2J00"] {
        assert_eq!(
            decode(with(welcome(), "confirm_code", json!(bad))),
            invalid("confirm_code")
        );
    }
    let hostile_name = json!({"name": "desk\u{202e}", "version": "0.1.0"});
    assert_eq!(
        decode(with(welcome(), "server", hostile_name)),
        invalid("name")
    );
    assert_eq!(
        decode(with(welcome(), "ops", json!(["ping", 5]))),
        invalid("ops")
    );
    assert_eq!(
        decode(with(welcome(), "read_only", json!("no"))),
        invalid("read_only")
    );
}

#[test]
fn unknown_members_are_ignored() {
    assert!(decode(with(auth(), "future", json!({"x": 1}))).is_ok());
}

#[test]
fn debug_output_has_no_nonce_and_no_proof() {
    let nonce_bytes = format!("{:?}", [0xA0_u8, 0xA1, 0xA2]);
    let nonce_start = nonce_bytes.trim_end_matches(']');
    for message in [
        auth(),
        pair(),
        welcome(),
        json!({"type": "hello", "protocol": {"min": 1, "max": 1}, "nonce": NONCE}),
    ] {
        let debug = format!("{:?}", decode(message).expect("valid message"));
        assert!(!debug.contains(nonce_start), "{debug}");
        assert!(
            !debug.contains("nonce") && !debug.contains("proof"),
            "{debug}"
        );
    }
}
