//! Milestone M2b of `docs/V2_PLAN.md` against a running daemon: audio, media, apps and the
//! remote keyboard (`docs/PROTOCOL.md` 17.9 to 17.13), the scope checks of 17.2 and 17.16
//! (8a, 8b, 8c), the rate classes (17.14), read-only mode and the default pairing.
//! Every program the daemon may start is the testkit's `fake-proc` (see `v2_m2b/rig.rs`).

#[path = "v2_m2a/rig.rs"]
#[allow(
    dead_code,
    reason = "only `Gate` and `PairCommand` of the M2a rig are used here"
)]
mod m2a_rig;
#[path = "v2_m2b/requests.rs"]
mod requests;
#[path = "v2_m2b/rig.rs"]
mod rig;
mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use m2a_rig::{Gate, PairCommand};
use requests::{Bucket, UNKNOWN_ID, ask, audio, detail, drain, launch, media, op_name, press};
use requests::{raw_refusal, refused, refused_unoffered, topic_state, type_text};
use rig::{NOTIFY, PLAIN_ENTRY, RISKY_ENTRY, Rig, SYSTEMD_RUN, TERMINAL};
use serde_json::{Value, json};
use support::{online_desktop, soon};
use swayconnect_cli::client::Session;
use swayconnect_protocol::ops::{Args, TOPIC_APPS, TOPIC_AUDIO, TOPIC_MEDIA};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{Apps, Audio, AudioChange, ErrorCode, IconHash, InputTextResult};
use swayconnect_protocol::{JsonObject, MediaCommand, Scope};

const EVERY_M2B_SCOPE: [Scope; 10] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Control,
    Scope::Audio,
    Scope::AudioCapture,
    Scope::Media,
    Scope::Launch,
    Scope::LaunchRisky,
    Scope::InputKeys,
    Scope::InputText,
];
/// The owner's `swayconnectd pair` default (`docs/DESIGN.md` 11.8, D30): with M2b every one
/// of them is known. And the scopes that are only ever granted by name.
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
/// The focused window of the layout fixture.
const FOCUSED_WINDOW: u64 = 12;

/// One valid request per new operation, with the scope it needs.
fn new_operations() -> [(Args, Scope); 10] {
    [
        (audio(1, AudioChange::Volume(50)), Scope::Audio),
        (audio(1, AudioChange::Mute(true)), Scope::Audio),
        (audio(1, AudioChange::Default), Scope::Audio),
        (media(MediaCommand::PlayPause), Scope::Media),
        (media(MediaCommand::Next), Scope::Media),
        (media(MediaCommand::Previous), Scope::Media),
        (launch(PLAIN_ENTRY), Scope::Launch),
        (Args::LauncherOpen, Scope::Launch),
        (type_text(FOCUSED_WINDOW, "hello"), Scope::InputText),
        (press(FOCUSED_WINDOW, "Return"), Scope::InputKeys),
    ]
}

/// The ids of the one fake device as a sink and as a source, once pactl was read.
async fn sink_and_source(session: &mut Session) -> (u64, u64) {
    let read = |data: &JsonObject| Audio::decode(data).expect("audio state");
    let data = topic_state(session, TOPIC_AUDIO, |data| {
        let state = read(data);
        state.available && !state.sinks.is_empty() && !state.sources.is_empty()
    })
    .await;
    let state = read(&data);
    (state.sinks[0].id, state.sources[0].id)
}

/// Audit lines as `(op, result, text_len)`.
fn audit_summary(rig: &Rig) -> Vec<(String, String, Option<u64>)> {
    let field = |line: &Value, name: &str| line[name].as_str().unwrap_or("?").to_owned();
    let (_, lines) = rig.audit();
    lines
        .iter()
        .map(|line| {
            (
                field(line, "op"),
                field(line, "result"),
                line["text_len"].as_u64(),
            )
        })
        .collect()
}

fn summary(lines: &[(&str, &str, Option<u64>)]) -> Vec<(String, String, Option<u64>)> {
    let owned =
        |(op, result, len): &(&str, &str, Option<u64>)| (op.to_string(), result.to_string(), *len);
    lines.iter().map(owned).collect()
}

#[tokio::test]
async fn each_new_operation_and_topic_needs_its_scope_and_a_refusal_touches_nothing() {
    let rig = Rig::start(false).await;
    let (_, mut reader) = rig.device(&[Scope::StateRead, Scope::StateTitles]).await;
    for (id, (args, scope)) in (1..).zip(new_operations()) {
        assert!(!reader.welcome().ops.iter().any(|op| op == op_name(&args)));
        let error = refused_unoffered(&mut reader, id, &args, ErrorCode::Forbidden).await;
        assert_eq!(error["detail"]["scope"], scope.as_str());
    }
    let topics = [
        (TOPIC_AUDIO, Scope::Audio),
        (TOPIC_MEDIA, Scope::Media),
        (TOPIC_APPS, Scope::Launch),
    ];
    for (id, (topic, scope)) in (100..).zip(topics) {
        assert!(
            !reader
                .welcome()
                .topics
                .iter()
                .any(|offered| offered == topic)
        );
        let args = json!({"topics": [topic]});
        let error = raw_refusal(
            &mut reader,
            id,
            "state.subscribe",
            &args,
            ErrorCode::Forbidden,
        )
        .await;
        assert_eq!(error["detail"]["scope"], scope.as_str(), "{topic}");
    }
    rig.assert_untouched();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn a_write_to_a_source_other_than_muting_it_needs_audio_capture() {
    let rig = Rig::start(false).await;
    let notice = Gate::install(&rig.programs, NOTIFY);
    let (_, mut plain) = rig.device(&[Scope::StateRead, Scope::Audio]).await;
    let (sink, source) = sink_and_source(&mut plain).await;
    for change in [
        AudioChange::Volume(30),
        AudioChange::Mute(false),
        AudioChange::Default,
    ] {
        let error = refused(&mut plain, &audio(source, change), ErrorCode::Forbidden).await;
        assert_eq!(detail(&error, "scope"), "audio.capture", "{change:?}");
    }
    assert_eq!(
        rig.pactl_writes(),
        Vec::<Vec<String>>::new(),
        "8a refuses before the write"
    );
    let muted = ask(&mut plain, &audio(source, AudioChange::Mute(true))).await;
    assert!(
        muted.is_ok(),
        "muting a source needs only `audio`: {muted:?}"
    );
    let louder = ask(&mut plain, &audio(sink, AudioChange::Volume(30))).await;
    assert!(louder.is_ok(), "a sink needs only `audio`: {louder:?}");

    let scopes = [Scope::StateRead, Scope::Audio, Scope::AudioCapture];
    let (profile, mut capture) = rig.device(&scopes).await;
    let short_id = &profile.device_id.as_str()[..4];
    let text = format!("Device with remote control rights connected (device {short_id})");
    let shown = ["--app-name=swayconnect", "--", "swayconnect", text.as_str()];
    assert_eq!(
        notice.release().await,
        shown,
        "`audio.capture` is tier 2; the plain device got no connect notice"
    );
    let unmuted = ask(&mut capture, &audio(source, AudioChange::Mute(false))).await;
    assert!(unmuted.is_ok(), "{unmuted:?}");
    let text = format!("Microphone unmuted remotely (device {short_id})");
    let shown = ["--app-name=swayconnect", "--", "swayconnect", text.as_str()];
    assert_eq!(
        notice.release().await,
        shown,
        "a fixed-text notice on a remote unmute"
    );
    let written = [
        ["--", "set-source-mute", "test.device", "1"],
        ["--", "set-sink-volume", "test.device", "30%"],
        ["--", "set-source-mute", "test.device", "0"],
    ];
    assert_eq!(rig.pactl_writes(), written);
    let expected = summary(&[
        ("audio.set_volume", "forbidden", None),
        ("audio.set_mute", "forbidden", None),
        ("audio.set_default", "forbidden", None),
        ("audio.set_mute", "ok", None),
    ]);
    assert_eq!(audit_summary(&rig), expected, "tier 2 audio requests only");
    rig.daemon.stop().await;
}

#[tokio::test]
async fn a_risky_entry_needs_launch_risky_and_a_plain_one_only_launch() {
    let rig = Rig::start(false).await;
    let (_, mut plain) = rig.device(&[Scope::StateRead, Scope::Launch]).await;
    let has_both = |data: &JsonObject| Apps::decode(data).expect("apps state").apps.len() == 2;
    let apps = Apps::decode(&topic_state(&mut plain, TOPIC_APPS, has_both).await).expect("apps");
    let risky: Vec<(&str, bool)> = apps
        .apps
        .iter()
        .map(|app| (app.entry_id.as_str(), app.risky))
        .collect();
    assert_eq!(
        risky,
        [(PLAIN_ENTRY, false), (RISKY_ENTRY, true)],
        "a terminal entry is risky"
    );

    let error = refused(&mut plain, &launch(RISKY_ENTRY), ErrorCode::Forbidden).await;
    assert_eq!(detail(&error, "scope"), "launch.risky");
    assert_eq!(
        rig.programs.calls(SYSTEMD_RUN),
        Vec::<Vec<String>>::new(),
        "8b refuses before the start"
    );
    ask(&mut plain, &launch(PLAIN_ENTRY))
        .await
        .expect("a plain entry needs only `launch`");
    let (_, mut trusted) = rig
        .device(&[Scope::StateRead, Scope::Launch, Scope::LaunchRisky])
        .await;
    ask(&mut trusted, &launch(RISKY_ENTRY))
        .await
        .expect("a risky entry with `launch.risky`");

    let terminal = rig.programs.program(TERMINAL);
    let terminal = terminal.to_str().expect("UTF-8 path");
    let commands: Vec<Vec<String>> = rig
        .programs
        .calls(SYSTEMD_RUN)
        .into_iter()
        .map(|argv| {
            let separator = argv
                .iter()
                .position(|word| word == "--")
                .expect("`--` in argv");
            argv[separator + 1..].to_vec()
        })
        .collect();
    assert_eq!(
        commands,
        [
            vec!["/opt/notes", "--new"],
            vec![terminal, "-e", "/opt/term"]
        ]
    );
    let expected = summary(&[
        ("app.launch", "forbidden", None),
        ("app.launch", "ok", None),
        ("app.launch", "ok", None),
    ]);
    assert_eq!(audit_summary(&rig), expected);
    rig.daemon.stop().await;
}

#[tokio::test]
async fn each_new_rate_class_has_its_own_bucket() {
    let rig = Rig::start(false).await;
    let (_, mut session) = rig.device(&EVERY_M2B_SCOPE).await;
    online_desktop(&mut session).await;
    drain(&mut session, &launch("missing"), Bucket::LAUNCH).await;
    refused(&mut session, &Args::LauncherOpen, ErrorCode::RateLimited).await;
    let play = media(MediaCommand::PlayPause);
    refused(&mut session, &play, ErrorCode::NotFound).await;

    let volume = audio(UNKNOWN_ID, AudioChange::Volume(50));
    drain(&mut session, &volume, Bucket::CONTINUOUS).await;
    drain(&mut session, &press(UNKNOWN_ID, "a"), Bucket::INPUT).await;
    drain(
        &mut session,
        &type_text(UNKNOWN_ID, "a"),
        Bucket::INPUT_TEXT,
    )
    .await;
    // `media.play_pause` above took one token of the five.
    let actions = Bucket {
        burst: Bucket::ACTIONS.burst - 1,
        ..Bucket::ACTIONS
    };
    drain(&mut session, &media(MediaCommand::Next), actions).await;
    let mute = audio(UNKNOWN_ID, AudioChange::Mute(true));
    refused(&mut session, &mute, ErrorCode::RateLimited).await;

    let icons = Args::IconGet {
        hashes: vec![IconHash::of(b"none")],
    };
    let icons = ask(&mut session, &icons).await;
    assert!(
        icons.is_ok(),
        "the icons class has its own bucket: {icons:?}"
    );
    rig.assert_nothing_ran();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn input_text_also_spends_a_budget_of_characters_per_minute() {
    let rig = Rig::start(false).await;
    let (_, mut session) = rig.device(&[Scope::StateRead, Scope::InputText]).await;
    online_desktop(&mut session).await;
    let full = type_text(UNKNOWN_ID, &"a".repeat(1024));
    for _ in 0..4 {
        refused(&mut session, &full, ErrorCode::NotFound).await;
    }
    let one = type_text(UNKNOWN_ID, "a");
    refused(&mut session, &one, ErrorCode::RateLimited).await;
    // One token is back after a second; the 4096 characters stay spent for a minute.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let error = refused(&mut session, &one, ErrorCode::RateLimited).await;
    let retry_after = detail(&error, "retry_after_ms")
        .as_u64()
        .expect("retry_after_ms");
    assert!(
        (1001..=60_000).contains(&retry_after),
        "retry after {retry_after} ms"
    );
    rig.assert_nothing_ran();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn read_only_refuses_every_new_action_and_still_serves_the_new_topics() {
    let rig = Rig::start(true).await;
    let (_, mut session) = rig.device(&EVERY_M2B_SCOPE).await;
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
    let topics = [TOPIC_AUDIO, TOPIC_MEDIA, TOPIC_APPS];
    let states = soon("subscribe", session.subscribe(&topics)).await;
    assert_eq!(states.expect("subscribed").len(), topics.len());
    rig.assert_untouched();
    rig.daemon.stop().await;
}

#[tokio::test]
async fn input_is_refused_while_the_fake_proc_scan_reports_swaylock() {
    let rig = Rig::start(false).await;
    rig.add_process(LOCKER_PID, "swaylock");
    let scopes = [Scope::StateRead, Scope::InputText, Scope::InputKeys];
    let (_, mut session) = rig.device(&scopes).await;
    online_desktop(&mut session).await;
    for args in [
        type_text(FOCUSED_WINDOW, "hello"),
        press(FOCUSED_WINDOW, "Return"),
    ] {
        let error = refused(&mut session, &args, ErrorCode::Locked).await;
        assert_eq!(detail(&error, "delivered_chars"), 0, "{}", op_name(&args));
    }
    assert_eq!(
        rig.keyboard.parts(),
        0,
        "nothing reached the keyboard while locked"
    );

    rig.remove_process(LOCKER_PID);
    let typed = ask(&mut session, &type_text(FOCUSED_WINDOW, "hi")).await;
    let typed = InputTextResult::decode(&typed.expect("unlocked: typed")).expect("result");
    assert_eq!(typed.delivered_chars, 2);
    assert_eq!(rig.keyboard.parts(), 1);
    let expected = summary(&[
        ("input.text", "locked", Some(5)),
        ("input.key", "locked", None),
        ("input.text", "ok", Some(2)),
    ]);
    assert_eq!(audit_summary(&rig), expected);
    assert!(
        !rig.audit().0.contains("hello"),
        "typed text is never audited"
    );
    rig.daemon.stop().await;
}

#[tokio::test]
async fn a_default_pairing_holds_the_owners_whole_set_and_no_scope_granted_only_by_name() {
    let rig = Rig::start(false).await;
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
    for scope in OWNER_DEFAULT.iter().chain(&BY_NAME_ONLY) {
        assert!(known.iter().any(|name| name == scope), "{scope} not known");
    }
    let granted: BTreeSet<&str> = welcome.scopes.iter().map(|scope| scope.as_str()).collect();
    assert_eq!(
        granted,
        BTreeSet::from(OWNER_DEFAULT),
        "exactly the owner's default"
    );
    rig.daemon.stop().await;
}
