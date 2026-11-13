//! `protocol/vectors/v1/handshake.json` and the server frames of the transcripts, compared
//! with what a running daemon sends (`docs/PROTOCOL.md` 15.3 to 15.9 and 15.15).
//!
//! A daemon draws its nonces, device ids and pairing secrets at random and counts `rev` from
//! its own start. The members that follow from them are left out of the comparison; every
//! other member must be equal, the `message` texts too. The client frames `pair` and `auth`
//! are sent with proofs over the nonces of the running connection.
//!
//! The transcripts are version 1 frames. The members that section 17 of the spec adds are
//! left out of the server frames, and `welcome.ops` and `welcome.topics` may list more than
//! the transcript: its entries must be there, in its order.

mod support;

use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{Value, json};
use support::vectors::{
    payload_of, payload_text, show_the_t3_desktop, transcript_frames, vector_file,
};
use support::wire::{self, Wire};
use support::{EVERY_SCOPE, Harness, Pairing, soon};
use swayconnect_protocol::proof::Context;
use swayconnect_protocol::{JsonObject, Message};
use swayconnect_testkit::{Event, FakeSway};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

const WINDOW_LIFETIME: Duration = Duration::from_secs(300);
/// No two runs of a daemon repeat these members.
const UNREPEATABLE_MEMBERS: [&str; 5] =
    ["nonce", "device_id", "server_proof", "confirm_code", "rev"];

/// The members that `docs/PROTOCOL.md` section 17 adds. Version 1 uses none of these names.
const V2_MEMBERS: [&str; 8] = [
    "known_scopes",
    "known_ops",
    "outputs",
    "scratchpad",
    "layouts_cut",
    "rect",
    "layout",
    "icon",
];
/// Lists of `welcome` that grow with the operations and topics of section 17.
const GROWING_LISTS: [&str; 2] = ["ops", "topics"];

/// `frame` without the members named in `dropped`, at any depth.
fn without(frame: &Value, dropped: &[&str]) -> Value {
    match frame {
        Value::Object(members) => {
            let kept = members
                .iter()
                .filter(|(name, _)| !dropped.contains(&name.as_str()));
            let kept = kept.map(|(name, member)| (name.clone(), without(member, dropped)));
            Value::Object(kept.collect())
        }
        Value::Array(items) => items.iter().map(|item| without(item, dropped)).collect(),
        scalar => scalar.clone(),
    }
}

/// `frame` without the members that no two runs of a daemon repeat.
fn repeatable(frame: &Value) -> Value {
    without(frame, &UNREPEATABLE_MEMBERS)
}

/// What version 1 defines of the server frame `sent`, with each growing list cut to the
/// entries of `expected`. A missing entry of `expected` makes the comparison fail.
fn v1_members(sent: &JsonObject, expected: &JsonObject) -> JsonObject {
    let Value::Object(mut v1) = without(&Value::Object(sent.clone()), &V2_MEMBERS) else {
        unreachable!("an object stays an object");
    };
    if v1.get("type").and_then(Value::as_str) != Some("welcome") {
        return v1;
    }
    for list in GROWING_LISTS {
        let wanted = expected.get(list).and_then(Value::as_array);
        if let (Some(wanted), Some(Value::Array(entries))) = (wanted, v1.get_mut(list)) {
            entries.retain(|entry| wanted.contains(entry));
        }
    }
    v1
}

/// What a transcript frame fixes for every daemon.
fn fixed_part(frame: JsonObject) -> Value {
    let mut fixed = repeatable(&Value::Object(frame));
    // `welcome.server.version` is the version of the daemon that runs.
    if let Some(server) = fixed.get_mut("server").and_then(Value::as_object_mut) {
        server.remove("version");
    }
    fixed
}

/// One connection to the daemon, and the transcript frames that are still to come on it.
struct Replay {
    wire: Wire<TlsStream<TcpStream>>,
    frames: VecDeque<Value>,
}

impl Replay {
    async fn connect(harness: &Harness, transcripts: &[&str]) -> Replay {
        let frames = transcripts.iter().copied().flat_map(transcript_frames);
        Replay {
            wire: wire::connect(harness.address()).await,
            frames: frames.collect(),
        }
    }

    fn next_frame_from(&mut self, sender: &str) -> Value {
        let frame = self.frames.pop_front().expect("the transcript goes on");
        assert_eq!(frame["from"], sender, "{frame}");
        frame
    }

    /// The client sends its next transcript frame, byte for byte.
    async fn client_sends_its_frame(&mut self) {
        let frame = self.next_frame_from("C");
        self.wire.send_payload(payload_text(&frame)).await;
    }

    /// The client sends `live` in place of its next transcript frame.
    async fn client_sends(&mut self, live: &Value) {
        self.next_frame_from("C");
        self.wire.send(live).await;
    }

    /// The daemon must send the next transcript frame. Returns what it sent.
    async fn server_sends_its_frame(&mut self) -> JsonObject {
        let expected = payload_of(&self.next_frame_from("S"));
        let sent = soon("a server frame", self.wire.receive()).await;
        let v1 = v1_members(&sent, &expected);
        assert_eq!(fixed_part(v1), fixed_part(expected));
        sent
    }

    /// `hello`, then `live_first_frame` in place of the transcript's `pair` or `auth`, then
    /// `welcome`.
    async fn handshake(&mut self, fp: [u8; 32], live_first_frame: impl FnOnce(&Context) -> Value) {
        let hello = self.server_sends_its_frame().await;
        let Ok(Message::Hello(hello)) = Message::decode(&hello) else {
            panic!("the first frame is not `hello`");
        };
        let context = Context {
            fp,
            server_nonce: hello.nonce,
            client_nonce: [7; 32],
        };
        self.client_sends(&live_first_frame(&context)).await;
        self.server_sends_its_frame().await;
    }

    fn assert_is_at_the_end(&self) {
        let left = &self.frames;
        assert!(left.is_empty(), "frames that were not replayed: {left:?}");
    }

    async fn ends_with_a_close(mut self) {
        self.assert_is_at_the_end();
        let after_last_frame = soon("close", self.wire.next()).await;
        assert_eq!(after_last_frame, None, "the server closes");
    }
}

/// T1 and T7 up to `welcome`, with the pending device shown to the owner.
async fn replay_until_pending(harness: &Harness, transcript: &str) -> (Replay, Pairing) {
    let mut pairing = harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await;
    let mut replay = Replay::connect(harness, &[transcript]).await;
    let secret = pairing.uri.secret;
    let pair = |context: &Context| wire::pair(&secret, context);
    replay.handshake(harness.daemon.fingerprint(), pair).await;
    let shown = soon("pending device", pairing.admin.pending_device()).await;
    shown.expect("admin answer").expect("a pending device");
    (replay, pairing)
}

fn show_focus_on(sway: &FakeSway, workspace_number: u32) {
    show_the_t3_desktop(sway, workspace_number);
    sway.send_event(Event::Workspace, json!({"change": "focus"}).to_string());
}

#[tokio::test]
async fn t1_the_daemon_sends_the_frames_of_a_confirmed_pairing() {
    let harness = Harness::start().await;
    let (mut replay, mut pairing) = replay_until_pending(&harness, "t1-pair-ok").await;

    pairing.answer(true).await;

    replay.server_sends_its_frame().await;
    replay.assert_is_at_the_end();
}

#[tokio::test]
async fn t7_the_daemon_sends_the_frames_of_a_refused_pairing() {
    let harness = Harness::start().await;
    let (mut replay, mut pairing) = replay_until_pending(&harness, "t7-pair-refused").await;

    pairing.answer(false).await;

    replay.server_sends_its_frame().await;
    replay.ends_with_a_close().await;
}

#[tokio::test]
async fn t2_and_t3_the_daemon_sends_the_frames_of_a_session() {
    let harness = Harness::start_prepared("", |sway| show_the_t3_desktop(sway, 1)).await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    // The daemon must have read the desktop from sway before the transcript subscribes.
    support::online_desktop(&mut harness.connect(&profile).await).await;
    let mut replay = Replay::connect(&harness, &["t2-auth-ok", "t3-session"]).await;
    let auth = |context: &Context| wire::auth(&profile.device_id, &profile.key, context);
    replay.handshake(harness.daemon.fingerprint(), auth).await;

    // `state.subscribe`, then `workspace.focus` with number 2.
    for _request in 0..2 {
        replay.client_sends_its_frame().await;
        replay.server_sends_its_frame().await;
    }
    assert_eq!(
        harness.sway().commands(),
        ["workspace --no-auto-back-and-forth number 2"]
    );
    show_focus_on(harness.sway(), 2);
    replay.server_sends_its_frame().await;
    // `window.focus` with an unknown id, then `ping`.
    for _request in 0..2 {
        replay.client_sends_its_frame().await;
        replay.server_sends_its_frame().await;
    }

    soon("daemon stop", harness.daemon.stop()).await;
    replay.server_sends_its_frame().await;
    replay.ends_with_a_close().await;
}

#[tokio::test]
async fn t4_and_t5_the_daemon_sends_the_reject_frames() {
    let harness = Harness::start().await;
    for transcript in ["t4-auth-failed", "t5-version"] {
        let mut replay = Replay::connect(&harness, &[transcript]).await;
        // Both transcripts begin after `hello`.
        soon("hello", replay.wire.receive()).await;

        replay.client_sends_its_frame().await;

        replay.server_sends_its_frame().await;
        replay.ends_with_a_close().await;
    }
}

#[tokio::test]
async fn t6_the_daemon_sends_the_frames_of_a_sway_that_went_away() {
    let mut harness = Harness::start_prepared("", |sway| show_the_t3_desktop(sway, 1)).await;
    let profile = harness.pair(&EVERY_SCOPE).await;
    support::online_desktop(&mut harness.connect(&profile).await).await;
    // The transcript begins in a session with a `desktop` subscription.
    let mut replay = Replay::connect(&harness, &["t6-sway-offline"]).await;
    let context = replay.wire.hello(harness.daemon.fingerprint()).await;
    let auth = wire::auth(&profile.device_id, &profile.key, &context);
    let subscribe =
        json!({"type": "req", "id": 1, "op": "state.subscribe", "args": {"topics": ["desktop"]}});
    for client_frame in [auth, subscribe] {
        replay.wire.send(&client_frame).await;
        soon("welcome, then the state", replay.wire.receive()).await;
    }

    harness.stop_sway();

    replay.server_sends_its_frame().await;
    replay.client_sends_its_frame().await;
    replay.server_sends_its_frame().await;
    replay.assert_is_at_the_end();
}

#[tokio::test]
async fn every_handshake_vector_gets_the_reject_code_of_the_spec_from_a_daemon() {
    let harness = Harness::start().await;
    let file = vector_file("handshake.json");
    let vectors = file.as_array().expect("an array of cases");
    assert!(
        vectors.len() >= 11,
        "the file holds the cases of spec 15.15"
    );
    for vector in vectors {
        let name = vector["name"].as_str().expect("name");
        // The window has its own secret, so no proof of the file is valid for it.
        let window = match vector["window_open"].as_bool().expect("window_open") {
            true => Some(harness.open_window(&EVERY_SCOPE, WINDOW_LIFETIME).await),
            false => None,
        };
        let mut client = wire::connect(harness.address()).await;
        soon("hello", client.receive()).await;

        client.send_payload(payload_text(vector)).await;

        let answer = soon("reject", client.receive()).await;
        assert_eq!(answer["type"], "reject", "{name}");
        assert_eq!(answer["code"], vector["expect"]["reject"], "{name}");
        let after_reject = soon("close", client.next()).await;
        assert_eq!(after_reject, None, "{name}: the server closes");
        if let Some(mut still_open) = window {
            // No refused frame closes the window (spec 5.4 step 4, 14 note 5).
            still_open.present_secret().await;
        }
    }
}
