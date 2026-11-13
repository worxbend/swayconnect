//! Replays every file of `protocol/vectors/v1` (`docs/PROTOCOL.md` section 15).

use base64::Engine as _;
use serde_json::Value;
use swayconnect_protocol::fingerprint::{decode_base64url, encode_base64url, fingerprint};
use swayconnect_protocol::frame::{FrameDecoder, FrameError, encode_frame};
use swayconnect_protocol::limits::MAX_FRAME_S2C_BYTES;
use swayconnect_protocol::ops::{self, Operation, TopicState};
use swayconnect_protocol::proof::{self, Context};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{DecodeError, Desktop, DeviceId, JsonObject, Message};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1");
const TRANSCRIPT_COUNT: usize = 7;

fn load(file: &str) -> Value {
    let path = format!("{VECTORS}/{file}");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("member `{member}` is not a string in {value}"))
}

fn entries<'a>(value: &'a Value, what: &str) -> &'a [Value] {
    let entries = value
        .as_array()
        .unwrap_or_else(|| panic!("{what} is not an array"));
    assert!(!entries.is_empty(), "{what} is empty");
    entries
}

fn hex(digits: &str) -> Vec<u8> {
    assert!(digits.len().is_multiple_of(2), "odd hex length: {digits}");
    (0..digits.len())
        .step_by(2)
        .map(|start| u8::from_str_radix(&digits[start..start + 2], 16).expect("hex digits"))
        .collect()
}

fn hex32(value: &Value, member: &str) -> [u8; 32] {
    hex(text(value, member))
        .try_into()
        .expect("32 bytes of hex")
}

fn payload_object(vector: &Value) -> JsonObject {
    serde_json::from_str(text(vector, "payload")).expect("payload is a JSON object")
}

#[test]
fn fingerprint_file() {
    let file = load("fingerprint.json");
    for entry in entries(&file, "fingerprint.json") {
        let certificate = base64::engine::general_purpose::STANDARD
            .decode(text(entry, "cert_der_b64"))
            .expect("standard base64");
        let fp = fingerprint(&certificate);
        assert_eq!(fp, hex32(entry, "fp_hex"));
        assert_eq!(encode_base64url(&fp), text(entry, "fp_b64url"));
        assert_eq!(decode_base64url::<32>(text(entry, "fp_b64url")), Ok(fp));
    }
}

#[test]
fn proofs_file() {
    let file = load("proofs.json");
    let (inputs, expect) = (&file["inputs"], &file["expect"]);
    let secret = hex32(inputs, "s");
    let device_id = DeviceId::parse(text(inputs, "device_id")).expect("canonical device id");
    let context = |connection: &str| Context {
        fp: hex32(inputs, "fp"),
        server_nonce: hex32(&inputs[connection], "server_nonce"),
        client_nonce: hex32(&inputs[connection], "client_nonce"),
    };
    let (pairing, later) = (context("pairing"), context("later"));

    let pair_proof = proof::pair_proof(&secret, &pairing);
    assert_eq!(pair_proof, hex32(expect, "pair_proof"));
    let key = proof::device_key(&secret, &pairing);
    assert_eq!(key, hex32(expect, "k"));
    assert_eq!(
        proof::confirm_mac(&key, &pairing),
        hex32(expect, "confirm_mac")
    );
    assert_eq!(
        proof::confirm_code(&key, &pairing),
        text(expect, "confirm_code")
    );
    let server_proof_pairing = proof::server_proof(&key, &pairing, &device_id);
    assert_eq!(server_proof_pairing, hex32(expect, "server_proof_pairing"));
    let client_proof = proof::client_proof(&key, &later, &device_id);
    assert_eq!(client_proof, hex32(expect, "client_proof_later"));
    let server_proof_later = proof::server_proof(&key, &later, &device_id);
    assert_eq!(server_proof_later, hex32(expect, "server_proof_later"));

    assert!(proof::verify_pair_proof(&secret, &pairing, &pair_proof));
    assert!(proof::verify_client_proof(
        &key,
        &later,
        &device_id,
        &client_proof
    ));
    assert!(proof::verify_server_proof(
        &key,
        &pairing,
        &device_id,
        &server_proof_pairing
    ));
    assert!(proof::verify_server_proof(
        &key,
        &later,
        &device_id,
        &server_proof_later
    ));

    // A proof of another connection, of the other side, or of all zero bytes never verifies.
    assert!(!proof::verify_pair_proof(&secret, &later, &pair_proof));
    assert!(!proof::verify_pair_proof(&secret, &pairing, &[0; 32]));
    assert!(!proof::verify_client_proof(
        &key,
        &later,
        &device_id,
        &server_proof_later
    ));
    assert!(!proof::verify_server_proof(
        &key,
        &later,
        &device_id,
        &server_proof_pairing
    ));

    // Spec 15.11: a client that saw another certificate computes a proof the server refuses.
    let relayed = Context {
        fp: [0; 32],
        ..later
    };
    let relayed_proof = proof::client_proof(&key, &relayed, &device_id);
    assert!(!proof::verify_client_proof(
        &key,
        &later,
        &device_id,
        &relayed_proof
    ));
}

#[test]
fn pairing_uri_file() {
    let file = load("pairing-uri.json");
    for vector in entries(&file["valid"], "valid") {
        let uri = text(vector, "uri");
        let parsed = PairingUri::parse(uri).unwrap_or_else(|error| panic!("{uri}: {error}"));
        assert_eq!(vector["v"], 1, "{uri}");
        assert_eq!(parsed.host, text(vector, "host"), "{uri}");
        assert_eq!(vector["port"], parsed.port, "{uri}");
        assert_eq!(parsed.fp, hex32(vector, "fp_hex"), "{uri}");
        assert_eq!(parsed.secret, hex32(vector, "secret_hex"), "{uri}");
        assert_eq!(parsed.name.as_deref(), vector["name"].as_str(), "{uri}");
        assert_eq!(
            PairingUri::parse(&parsed.to_uri_string()),
            Ok(parsed),
            "{uri}"
        );
    }
    for vector in entries(&file["invalid"], "invalid") {
        let (uri, reason) = (text(vector, "uri"), text(vector, "reason"));
        assert!(
            PairingUri::parse(uri).is_err(),
            "accepted ({reason}): {uri}"
        );
    }
}

#[test]
fn frames_file() {
    let file = load("frames.json");
    for vector in entries(&file, "frames.json") {
        let name = text(vector, "name");
        let limit = u32::try_from(vector["limit"].as_u64().expect("limit")).expect("limit");
        let mut decoder = FrameDecoder::new(limit);
        let mut frames = Vec::new();
        let mut failure = None;
        for read in entries(&vector["input_hex"], "input_hex") {
            decoder.push(&hex(read.as_str().expect("hex chunk")));
            loop {
                match decoder.next_frame() {
                    Ok(Some(frame)) => frames.push(frame),
                    Ok(None) => break,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
        }
        match &vector["expect"] {
            Value::String(kind) => {
                let expected = match kind.as_str() {
                    "bad" => FrameError::Bad,
                    "too_large" => FrameError::TooLarge,
                    other => panic!("{name}: unknown expectation {other}"),
                };
                assert_eq!(failure, Some(expected), "{name}");
                assert!(frames.is_empty(), "{name}");
            }
            expected => {
                let expected: Vec<JsonObject> = entries(expected, "expect")
                    .iter()
                    .map(|payload| serde_json::from_str(payload.as_str().expect("payload")))
                    .collect::<Result<_, _>>()
                    .expect("expected payloads are JSON objects");
                assert_eq!(failure, None, "{name}");
                assert_eq!(frames, expected, "{name}");
            }
        }
    }
}

#[test]
fn handshake_file() {
    let secret = hex32(&load("proofs.json")["inputs"], "s");
    let file = load("handshake.json");
    for vector in entries(&file, "handshake.json") {
        let name = text(vector, "name");
        let decoded = Message::decode(&payload_object(vector));
        match (text(&vector["expect"], "reject"), decoded) {
            ("bad_message", decoded) => assert!(decoded.is_err(), "{name}: {decoded:?}"),
            ("unsupported_version", Ok(Message::Auth(auth))) => {
                assert_ne!(auth.protocol, 1, "{name}");
            }
            ("pairing_failed", Ok(Message::Pair(pair))) => {
                assert_eq!(pair.protocol, 1, "{name}");
                let context = t1_context(&pair.nonce);
                assert!(
                    !proof::verify_pair_proof(&secret, &context, &pair.proof),
                    "{name}"
                );
            }
            (code, decoded) => panic!("{name}: expected {code}, decode gave {decoded:?}"),
        }
    }
}

/// The context of transcript T1 with the client nonce of a `pair` frame.
fn t1_context(client_nonce: &[u8; 32]) -> Context {
    let inputs = &load("proofs.json")["inputs"];
    Context {
        fp: hex32(inputs, "fp"),
        server_nonce: hex32(&inputs["pairing"], "server_nonce"),
        client_nonce: *client_nonce,
    }
}

/// The protocol crate owns the decode step and the argument check of each request case.
/// Scope, read-only, rate limit and sway state are replayed by the daemon tests.
#[test]
fn requests_file() {
    let file = load("requests.json");
    for vector in entries(&file, "requests.json") {
        let name = text(vector, "name");
        let decoded = Message::decode(&payload_object(vector));
        let expect = &vector["expect"];
        if expect == "ignored" {
            assert_eq!(decoded, Err(DecodeError::UnknownType), "{name}");
        } else if expect.get("bye").is_some() {
            assert_eq!(expect["bye"], "protocol_error", "{name}");
            let is_protocol_error = match &decoded {
                Ok(message) => !matches!(message, Message::Req(_)),
                Err(error) => *error != DecodeError::UnknownType,
            };
            assert!(is_protocol_error, "{name}: {decoded:?}");
        } else {
            let Ok(Message::Req(req)) = decoded else {
                panic!("{name}: not a req: {decoded:?}");
            };
            let error = expect.get("error").and_then(Value::as_str);
            let Some(operation) = Operation::from_wire(&req.op) else {
                assert_eq!(error, Some("unknown_op"), "{name}");
                continue;
            };
            assert_ne!(error, Some("unknown_op"), "{name}");
            let args = operation.decode_args(req.args.as_ref());
            assert_eq!(
                args.is_err(),
                error == Some("bad_request"),
                "{name}: {args:?}"
            );
            if let Some(scope) = expect["detail"].get("scope") {
                let required = operation.required_scope().map(|scope| scope.as_str());
                assert_eq!(scope.as_str(), required, "{name}");
            }
        }
    }
}

#[test]
fn transcript_files() {
    let directory = format!("{VECTORS}/transcripts");
    let mut files: Vec<String> = std::fs::read_dir(&directory)
        .expect("transcripts directory")
        .map(|entry| entry.expect("directory entry").file_name())
        .map(|name| name.into_string().expect("UTF-8 file name"))
        .collect();
    files.sort();
    assert_eq!(files.len(), TRANSCRIPT_COUNT, "{files:?}");
    for file in files {
        let transcript = load(&format!("transcripts/{file}"));
        for frame in entries(&transcript["frames"], "frames") {
            replay_frame(&file, frame);
        }
    }
}

/// Decode, then encode to an equal JSON value, through the frame codec.
fn replay_frame(file: &str, frame: &Value) {
    let payload = text(frame, "payload");
    let length = u32::try_from(payload.len()).expect("payload length");
    let mut decoder = FrameDecoder::new(MAX_FRAME_S2C_BYTES);
    decoder.push(&length.to_be_bytes());
    decoder.push(payload.as_bytes());
    let object = decoder
        .next_frame()
        .unwrap_or_else(|error| panic!("{file}: {error}: {payload}"))
        .unwrap_or_else(|| panic!("{file}: incomplete frame: {payload}"));

    let message = Message::decode(&object).unwrap_or_else(|error| panic!("{file}: {error}"));
    let expected_sender = match &message {
        Message::Pair(_) | Message::Auth(_) | Message::Req(_) => "C",
        _ => "S",
    };
    assert_eq!(text(frame, "from"), expected_sender, "{file}: {payload}");

    let encoded = message.encode();
    assert_eq!(encoded, object, "{file}");
    let wire = encode_frame(&encoded, MAX_FRAME_S2C_BYTES).expect("frame fits");
    let mut second = FrameDecoder::new(MAX_FRAME_S2C_BYTES);
    second.push(&wire);
    assert_eq!(second.next_frame(), Ok(Some(object)), "{file}");

    replay_typed_payload(file, &message);
}

/// The typed layers inside `req`, `res` and `evt` also decode and encode to an equal value.
fn replay_typed_payload(file: &str, message: &Message) {
    match message {
        Message::Req(req) => {
            let operation = Operation::from_wire(&req.op).expect("transcript operation is known");
            let args = operation
                .decode_args(req.args.as_ref())
                .unwrap_or_else(|error| panic!("{file}: {error}"));
            assert_eq!(&args.to_req(req.id), req, "{file}");
        }
        Message::Res(res) => {
            let states = res
                .result
                .as_ref()
                .filter(|result| result.contains_key("states"));
            if let Some(result) = states {
                let states = ops::decode_subscribe_result(result).expect("subscribe result");
                assert_eq!(&ops::encode_subscribe_result(&states), result, "{file}");
                states.iter().for_each(|state| replay_desktop(file, state));
            }
        }
        Message::Evt(evt) if evt.event == ops::EVENT_STATE_CHANGED => {
            let state = TopicState::decode(&evt.data).expect("state.changed data");
            assert_eq!(state.encode(), evt.data, "{file}");
            replay_desktop(file, &state);
        }
        Message::Evt(evt) => {
            assert_eq!(evt.event, ops::EVENT_PAIRING_CONFIRMED, "{file}");
            assert!(evt.data.is_empty(), "{file}");
        }
        _ => {}
    }
}

fn replay_desktop(file: &str, state: &TopicState) {
    assert_eq!(state.topic, ops::TOPIC_DESKTOP, "{file}");
    let desktop = Desktop::decode(&state.data).unwrap_or_else(|error| panic!("{file}: {error}"));
    assert_eq!(desktop.encode(), state.data, "{file}");
}
