//! The watch task under paused time, against `FakeRunner` and the captured fixtures.

use swayconnect_protocol::{AudioChange, DeviceId, DeviceKind};

use super::super::tests::{PACTL, config, deps, exited};
use super::super::{AudioError, AudioService, Capture};
use super::*;
use crate::proc::Exit;
use crate::proc::fake::{CallKind, FakeRunner};

const SINKS: &[u8] = include_bytes!("../../../tests/fixtures/pactl/sinks.json");
const SOURCES: &[u8] = include_bytes!("../../../tests/fixtures/pactl/sources.json");
const DEFAULT_SINK: &[u8] = include_bytes!("../../../tests/fixtures/pactl/default-sink.txt");
const DEFAULT_SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/pactl/default-source.txt");
const CAPTURED_SINK: &str = "alsa_output.pci-0000_2b_00.4.analog-stereo";

struct Started {
    runner: FakeRunner,
    service: AudioService,
    snapshot: Receiver<Arc<Snapshot>>,
    stop: tokio::sync::watch::Sender<bool>,
}

fn queue_snapshot(runner: &FakeRunner, sinks: &[u8]) {
    for stdout in [sinks, SOURCES, DEFAULT_SINK, DEFAULT_SOURCE] {
        runner.reply_next_run(exited(0, stdout));
    }
}

/// A service whose first snapshot reads the captured fixtures.
async fn start() -> Started {
    let runner = FakeRunner::default();
    queue_snapshot(&runner, SINKS);
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let service = AudioService::spawn(&config(100), deps(&runner), shutdown);
    let mut snapshot = service.snapshot();
    snapshot.changed().await.expect("first snapshot");
    Started {
        runner,
        service,
        snapshot,
        stop,
    }
}

fn argv(words: &[&str]) -> Vec<String> {
    std::iter::once(PACTL)
        .chain(words.iter().copied())
        .map(str::to_owned)
        .collect()
}

#[tokio::test(start_paused = true)]
async fn subscribe_starts_then_a_snapshot_of_the_captured_machine() {
    let running = start().await;
    let audio = running.snapshot.borrow().audio().clone();
    assert!(audio.available);
    assert_eq!(audio.max_volume_pct, 100);
    assert_eq!(audio.sinks.len(), 1);
    assert_eq!(audio.sinks[0].id, 1);
    assert_eq!(audio.sinks[0].kind, DeviceKind::Speaker);
    assert_eq!(audio.sinks[0].volume_pct, 15);
    assert_eq!(audio.sources.len(), 1, "the monitor is hidden");
    assert_eq!(audio.sources[0].kind, DeviceKind::Microphone);
    assert_eq!(audio.default_sink, Some(1));
    assert_eq!(audio.default_source, Some(2));
    assert!(!audio.truncated);

    assert_eq!(
        running.runner.argvs(),
        [
            argv(&["subscribe"]),
            argv(&["-f", "json", "list", "sinks"]),
            argv(&["-f", "json", "list", "sources"]),
            argv(&["get-default-sink"]),
            argv(&["get-default-source"]),
        ]
    );
    let calls = running.runner.calls();
    assert_eq!(calls[0].kind, CallKind::SpawnLines);
    assert!(
        calls[1..]
            .iter()
            .all(|call| call.kind == CallKind::Run(PACTL_TIMEOUT))
    );
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_device_events_gives_one_snapshot_after_the_debounce() {
    let mut running = start().await;
    for line in [
        "Event 'new' on client #177",
        "Event 'remove' on client #177",
        "Event 'new' on sink-input #90",
    ] {
        running.runner.send_line(0, line);
    }
    sleep(Duration::from_secs(1)).await;
    assert_eq!(
        running.runner.calls().len(),
        5,
        "no snapshot for these events"
    );

    // The sink vanishes.
    queue_snapshot(&running.runner, b"[]");
    let sent = Instant::now();
    for line in [
        "Event 'remove' on sink #59",
        "Event 'change' on card #52",
        "Event 'change' on server #4294967295",
    ] {
        running.runner.send_line(0, line);
    }
    running.snapshot.changed().await.expect("second snapshot");
    assert!(sent.elapsed() >= DEBOUNCE);
    assert_eq!(
        running.runner.calls().len(),
        9,
        "one snapshot for the burst"
    );
    let audio = running.snapshot.borrow().audio().clone();
    assert!(audio.sinks.is_empty());
    assert_eq!(audio.default_sink, None);
    assert_eq!(audio.sources[0].id, 2, "the source keeps its id");

    let outcome = running
        .service
        .apply(
            &DeviceId::from_bytes([1; 16]),
            1,
            AudioChange::Mute(true),
            Capture::NotGranted,
        )
        .await;
    assert!(matches!(outcome, Err(AudioError::NotFound)), "{outcome:?}");
    assert_eq!(running.runner.calls().len(), 9, "nothing written");
}

#[tokio::test(start_paused = true)]
async fn an_ended_subscribe_is_respawned_with_a_growing_backoff() {
    let running = start().await;
    let runner = &running.runner;

    runner.end_child(0, Exit::Code(1));
    sleep(FIRST_BACKOFF - Duration::from_millis(1)).await;
    assert_eq!(runner.spawned_count(), 1);
    sleep(Duration::from_millis(2)).await;
    assert_eq!(runner.spawned_count(), 2, "respawned after 1 s");
    // Its snapshot read no list (the fake answers empty output): not reachable.
    assert!(!running.snapshot.borrow().audio().available);

    runner.end_child(1, Exit::Code(1));
    sleep(FIRST_BACKOFF * 2 - Duration::from_millis(1)).await;
    assert_eq!(runner.spawned_count(), 2);
    sleep(Duration::from_millis(2)).await;
    assert_eq!(runner.spawned_count(), 3, "respawned after 2 s");
}

#[tokio::test(start_paused = true)]
async fn the_backoff_stops_growing_at_its_maximum() {
    let running = start().await;
    let runner = &running.runner;
    let mut waits = Vec::new();
    for index in 0..7 {
        let ended = Instant::now();
        runner.end_child(index, Exit::Code(1));
        while runner.spawned_count() == index + 1 {
            sleep(Duration::from_millis(500)).await;
        }
        waits.push(ended.elapsed().as_secs());
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30]);
}

/// A sound-server restart after hours of uptime is respawned after the first backoff, not
/// after the one the earlier quick failures had grown to.
#[tokio::test(start_paused = true)]
async fn a_healthy_run_resets_the_backoff() {
    let running = start().await;
    let runner = &running.runner;
    for index in 0..2 {
        runner.end_child(index, Exit::Code(1));
        while runner.spawned_count() == index + 1 {
            sleep(Duration::from_millis(500)).await;
        }
    }
    // The backoff is now 4 s; the third child runs past a healthy run.
    sleep(HEALTHY_RUN + Duration::from_secs(1)).await;
    let ended = Instant::now();
    runner.end_child(2, Exit::Code(1));
    while runner.spawned_count() == 3 {
        sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(ended.elapsed().as_secs(), 1, "back to the first backoff");
}

#[tokio::test(start_paused = true)]
async fn a_subscribe_that_cannot_start_still_gets_a_snapshot_and_a_retry() {
    let runner = FakeRunner::default();
    runner.fail_next_spawn();
    queue_snapshot(&runner, SINKS);
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let service = AudioService::spawn(&config(100), deps(&runner), shutdown);
    let mut snapshot = service.snapshot();
    snapshot
        .changed()
        .await
        .expect("snapshot without subscribe");
    assert!(snapshot.borrow().audio().available);
    assert_eq!(runner.spawned_count(), 0);

    sleep(FIRST_BACKOFF + Duration::from_millis(1)).await;
    assert_eq!(runner.spawned_count(), 1, "retried after 1 s");
    let spawns = runner
        .calls()
        .iter()
        .filter(|call| call.kind == CallKind::SpawnLines)
        .count();
    assert_eq!(spawns, 2);
}

#[tokio::test(start_paused = true)]
async fn shutdown_kills_the_subscribe_child() {
    let running = start().await;
    running.stop.send_replace(true);
    sleep(Duration::from_millis(1)).await;
    assert!(running.runner.was_killed(0));
    sleep(MAX_BACKOFF * 2).await;
    assert_eq!(
        running.runner.spawned_count(),
        1,
        "no respawn after shutdown"
    );
}

#[tokio::test(start_paused = true)]
async fn a_write_names_the_device_by_its_pactl_name() {
    let running = start().await;
    running
        .service
        .apply(
            &DeviceId::from_bytes([1; 16]),
            1,
            AudioChange::Volume(30),
            Capture::NotGranted,
        )
        .await
        .expect("written");
    assert_eq!(
        running.runner.argvs().last(),
        Some(&argv(&["set-sink-volume", "--", CAPTURED_SINK, "30%"]))
    );
}
