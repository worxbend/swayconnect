//! Writes: step 8, the capture rule, argv, errors and the unmute notice, and a replay of
//! the `requests` of `protocol/vectors/v1/ext/audio.json` from step 8 on.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;
use swayconnect_protocol::ops::{Args, Operation};
use swayconnect_protocol::{Audio, JsonObject, Message};

use super::snapshot::Target;
use super::*;
use crate::notify::NotifyConfig;
use crate::proc::fake::FakeRunner;
use crate::proc::{Captured, Exit, Finished};

pub(super) const PACTL: &str = "/fake/pactl";
const NOTIFY_SEND: &str = "/fake/notify-send";
const VECTORS: &str = include_str!("../../../../protocol/vectors/v1/ext/audio.json");
/// Lets a spawned notice task reach the runner.
const SCHEDULER_TURNS: usize = 8;

pub(super) fn config(max_volume_pct: u16) -> AudioConfig {
    AudioConfig {
        pactl: ProgramPath::new(PathBuf::from(PACTL)).expect("absolute"),
        max_volume_pct: MaxVolumePct::try_from(max_volume_pct).expect("in range"),
    }
}

pub(super) fn deps(runner: &FakeRunner) -> AudioDeps {
    let notify = NotifyConfig {
        notify_send: ProgramPath::new(PathBuf::from(NOTIFY_SEND)).expect("absolute"),
    };
    AudioDeps {
        runner: Arc::new(runner.clone()),
        notifier: Notifier::new(Arc::new(runner.clone()), &notify),
    }
}

pub(super) fn exited(code: i32, stdout: &[u8]) -> Result<Finished, ProcError> {
    Ok(Finished {
        exit: Exit::Code(code),
        stdout: Captured {
            bytes: stdout.to_vec(),
            truncated: false,
        },
        stderr: Captured::default(),
    })
}

/// A service on a fixed snapshot, without the watch task.
fn service_on(runner: &FakeRunner, snapshot: Snapshot, max_volume_pct: u16) -> AudioService {
    let deps = deps(runner);
    let config = config(max_volume_pct);
    let (_sender, snapshot) = tokio::sync::watch::channel(Arc::new(snapshot));
    AudioService {
        shared: Arc::new(Shared {
            runner: deps.runner,
            notifier: deps.notifier,
            pactl: config.pactl,
            max_volume_pct,
        }),
        snapshot,
    }
}

fn device() -> DeviceId {
    DeviceId::from_bytes([7; 16])
}

/// The vectors' server state `main` with its pactl names.
fn vector_snapshot(file: &Value) -> Snapshot {
    let main = &file["server_states"]["main"];
    let state: &JsonObject = main["state"].as_object().expect("state");
    let audio = Audio::decode(state).expect("audio state");
    let direction_of = |id: u64| {
        if audio.sources.iter().any(|device| device.id == id) {
            Direction::Source
        } else {
            Direction::Sink
        }
    };
    let targets: HashMap<u64, Target> = main["names"]
        .as_object()
        .expect("names")
        .iter()
        .map(|(id, name)| {
            let id: u64 = id.parse().expect("numeric id");
            let target = Target {
                direction: direction_of(id),
                name: name.as_str().expect("name").to_owned(),
            };
            (id, target)
        })
        .collect();
    Snapshot { audio, targets }
}

fn pactl_calls(runner: &FakeRunner) -> Vec<Vec<String>> {
    runner
        .argvs()
        .into_iter()
        .filter(|argv| argv[0] == PACTL)
        .map(|argv| argv[1..].to_vec())
        .collect()
}

async fn notice_shown(runner: &FakeRunner) -> bool {
    for _ in 0..SCHEDULER_TURNS {
        tokio::task::yield_now().await;
    }
    runner.argvs().iter().any(|argv| argv[0] == NOTIFY_SEND)
}

/// The cases that reach step 8: the earlier steps belong to the dispatcher and are replayed
/// in `swayconnect-protocol/tests/ext_m2b_requests.rs`.
fn reaches_step_8(vector: &Value) -> bool {
    let expect = &vector["expect"];
    let early = matches!(
        expect["error"].as_str(),
        Some("bad_request" | "read_only" | "rate_limited")
    ) || (expect["error"] == "forbidden"
        && expect["detail"]["scope"] != "audio.capture");
    let payload: Value =
        serde_json::from_str(vector["payload"].as_str().expect("payload")).expect("payload JSON");
    payload["op"]
        .as_str()
        .is_some_and(|op| op.starts_with("audio."))
        && !early
}

#[tokio::test]
async fn request_vectors_from_step_8_on() {
    let file: Value = serde_json::from_str(VECTORS).expect("vector file");
    let max_volume_pct =
        u16::try_from(file["config"]["max_volume_pct"].as_u64().expect("max")).expect("u16");
    let mut replayed = 0;
    for vector in file["requests"].as_array().expect("requests") {
        let name = vector["name"].as_str().expect("name");
        if !reaches_step_8(vector) {
            continue;
        }
        let object: JsonObject =
            serde_json::from_str(vector["payload"].as_str().expect("payload")).expect("object");
        let Ok(Message::Req(req)) = Message::decode(&object) else {
            panic!("{name}: not a req");
        };
        let operation = Operation::from_wire(&req.op).expect("known op");
        let Ok(Args::Audio { device_id, change }) = operation.decode_args(req.args.as_ref()) else {
            panic!("{name}: args");
        };
        let runner = FakeRunner::default();
        match vector["backend"].as_str() {
            Some("ok") => runner.reply_next_run(exited(0, b"")),
            Some("fails") => runner.reply_next_run(exited(1, b"Failure: Internal error\n")),
            Some("hangs") => runner.reply_next_run(Err(ProcError::TimedOut(PACTL_TIMEOUT))),
            other => panic!("{name}: backend {other:?}"),
        }
        let capture = if vector["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .any(|scope| scope == "audio.capture")
        {
            Capture::Granted
        } else {
            Capture::NotGranted
        };
        let service = service_on(&runner, vector_snapshot(&file), max_volume_pct);

        let outcome = service.apply(&device(), device_id, change, capture).await;

        match vector["expect"]["error"].as_str() {
            None => assert!(outcome.is_ok(), "{name}: {outcome:?}"),
            Some(code) => {
                let error = outcome.expect_err(name);
                assert_eq!(error.code().as_str(), code, "{name}");
                let scope = vector["expect"]["detail"]["scope"].as_str();
                assert_eq!(error.scope().map(|scope| scope.as_str()), scope, "{name}");
            }
        }
        let expected: Vec<Vec<String>> =
            serde_json::from_value(vector["pactl"].clone()).expect("pactl argvs");
        assert_eq!(pactl_calls(&runner), expected, "{name}");
        assert_eq!(
            notice_shown(&runner).await,
            vector["notice"] == true,
            "{name}: notice"
        );
        replayed += 1;
    }
    assert!(replayed >= 20, "only {replayed} cases replayed");
}

#[tokio::test]
async fn an_id_that_is_not_in_the_state_is_not_found_and_runs_nothing() {
    let file: Value = serde_json::from_str(VECTORS).expect("vector file");
    let runner = FakeRunner::default();
    let service = service_on(&runner, vector_snapshot(&file), 100);
    let outcome = service
        .apply(&device(), 999, AudioChange::Mute(true), Capture::Granted)
        .await;
    assert!(matches!(outcome, Err(AudioError::NotFound)), "{outcome:?}");

    let unavailable = service_on(&runner, Snapshot::unavailable(100), 100);
    let outcome = unavailable
        .apply(&device(), 1, AudioChange::Default, Capture::Granted)
        .await;
    assert!(matches!(outcome, Err(AudioError::NotFound)), "{outcome:?}");
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn a_failed_start_of_pactl_is_backend_unavailable() {
    let file: Value = serde_json::from_str(VECTORS).expect("vector file");
    let runner = FakeRunner::default();
    runner.reply_next_run(Err(ProcError::Spawn {
        program: PathBuf::from(PACTL),
        source: std::io::ErrorKind::NotFound.into(),
    }));
    let service = service_on(&runner, vector_snapshot(&file), 100);
    let error = service
        .apply(&device(), 1, AudioChange::Volume(10), Capture::NotGranted)
        .await
        .expect_err("spawn failed");
    assert_eq!(error.code(), ErrorCode::BackendUnavailable);
}

#[tokio::test]
async fn the_volume_sent_never_passes_the_server_maximum() {
    let file: Value = serde_json::from_str(VECTORS).expect("vector file");
    let runner = FakeRunner::default();
    let service = service_on(&runner, vector_snapshot(&file), 100);
    assert_eq!(service.max_volume_pct(), 100);
    // The dispatcher refuses 150 at step 6; a caller that skipped it still gets the clamp.
    service
        .apply(&device(), 1, AudioChange::Volume(150), Capture::NotGranted)
        .await
        .expect("written");
    assert_eq!(
        pactl_calls(&runner),
        [[
            "--",
            "set-sink-volume",
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
            "100%"
        ]]
    );
}

#[tokio::test]
async fn the_unmute_notice_has_fixed_text_and_the_short_id() {
    let file: Value = serde_json::from_str(VECTORS).expect("vector file");
    let runner = FakeRunner::default();
    let service = service_on(&runner, vector_snapshot(&file), 100);
    service
        .apply(&device(), 2, AudioChange::Mute(false), Capture::Granted)
        .await
        .expect("unmuted");
    assert!(notice_shown(&runner).await);
    let notice = runner
        .argvs()
        .into_iter()
        .find(|argv| argv[0] == NOTIFY_SEND)
        .expect("notice");
    let short_id: String = device().as_str().chars().take(4).collect();
    assert_eq!(
        notice.last().map(String::as_str),
        Some(format!("Microphone unmuted remotely (device {short_id})").as_str())
    );
}
