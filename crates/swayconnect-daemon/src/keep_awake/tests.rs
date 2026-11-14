//! Paused time and `FakeRunner` only: no test starts a real program (`docs/DESIGN.md` 11.12).

use std::sync::atomic::{AtomicUsize, Ordering};

use swayconnect_protocol::{Output, Rect, Transform};

use super::*;
use crate::model;
use crate::notify::NotifyConfig;
use crate::proc::fake::{CallKind, FakeRunner};

const HOUR: Duration = Duration::from_secs(3600);
const MINUTE: Duration = Duration::from_secs(60);
const COOLDOWN: Duration = Duration::from_secs(900);
const INHIBIT: &str = "/fake/systemd-inhibit";
const SLEEP: &str = "/fake/sleep";
/// Enough turns of the scheduler for a spawned notice task to call the runner.
const SCHEDULER_TURNS: usize = 16;

#[derive(Default)]
struct FakePower {
    calls: AtomicUsize,
}

impl PowerOn for FakePower {
    fn power_on(&self) -> BoxFuture<'_, Result<(), ActionError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

struct Rig {
    keep_awake: KeepAwake,
    runner: FakeRunner,
    power: Arc<FakePower>,
    desktop: watch::Sender<Arc<Published>>,
    shutdown: watch::Sender<bool>,
}

fn config(extra: &str) -> KeepAwakeConfig {
    toml::from_str(&format!(
        "inhibit = \"{INHIBIT}\"\nsleep = \"{SLEEP}\"\n{extra}"
    ))
    .expect("config")
}

fn rig() -> Rig {
    rig_with_check(Duration::ZERO)
}

fn rig_with_check(start_check: Duration) -> Rig {
    let runner = FakeRunner::default();
    let power = Arc::new(FakePower::default());
    let notify: NotifyConfig =
        toml::from_str(r#"notify_send = "/fake/notify-send""#).expect("notify config");
    let (desktop, desktop_seen) = watch::channel(published(Vec::new()));
    let (shutdown, stop) = watch::channel(false);
    let deps = KeepAwakeDeps {
        runner: Arc::new(runner.clone()),
        notifier: Notifier::new(Arc::new(runner.clone()), &notify),
        desktop: desktop_seen,
        power: power.clone(),
        start_check,
    };
    Rig {
        keep_awake: KeepAwake::spawn(&config(""), deps, stop),
        runner,
        power,
        desktop,
        shutdown,
    }
}

fn published(outputs: Vec<Output>) -> Arc<Published> {
    let mut desktop = model::offline_desktop();
    desktop.outputs = Some(outputs);
    Arc::new(Published { rev: 1, desktop })
}

fn output(active: bool, power: bool) -> Output {
    Output {
        name: "DP-1".to_owned(),
        model: None,
        rect: Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
        },
        scale_milli: 1000,
        transform: Transform::Normal,
        active,
        power,
        current_workspace_id: None,
    }
}

fn device(byte: u8) -> DeviceId {
    DeviceId::from_bytes([byte; 16])
}

fn on(duration_s: Option<u64>) -> KeepAwakeArgs {
    KeepAwakeArgs::On { duration_s }
}

impl Rig {
    async fn set(&self, byte: u8, args: KeepAwakeArgs) -> Result<KeepAwakeResult, KeepAwakeError> {
        self.keep_awake.set(&device(byte), args).await
    }

    fn inhibitor_argvs(&self) -> Vec<Vec<String>> {
        self.runner
            .calls()
            .iter()
            .filter(|call| call.kind == CallKind::Spawn)
            .map(|call| call.argv())
            .collect()
    }

    /// The sleep seconds of the newest inhibitor.
    fn last_seconds(&self) -> String {
        self.inhibitor_argvs()
            .last()
            .and_then(|argv| argv.last().cloned())
            .expect("an inhibitor was started")
    }

    async fn notices(&self) -> Vec<Vec<String>> {
        for _ in 0..SCHEDULER_TURNS {
            tokio::task::yield_now().await;
        }
        self.runner
            .calls()
            .iter()
            .filter(|call| matches!(call.kind, CallKind::Run(_)))
            .map(|call| call.argv())
            .collect()
    }

    fn state(&self, byte: u8) -> KeepAwakeState {
        self.keep_awake
            .view()
            .borrow()
            .state_for(&device(byte), Instant::now())
    }

    async fn until_off(&self) {
        self.keep_awake
            .view()
            .wait_for(|view| view.hold.is_none())
            .await
            .expect("keep-awake runs");
    }
}

#[tokio::test(start_paused = true)]
async fn on_runs_the_configured_inhibitor_with_the_exact_argv() {
    let rig = rig();
    let result = rig.set(1, on(None)).await.expect("on");
    assert_eq!(result.remaining_ms, Some(3_600_000));
    rig.set(1, on(Some(60))).await.expect("replace");
    let inhibitor = |seconds: &str| {
        vec![
            INHIBIT.to_owned(),
            "--what=idle".to_owned(),
            "--mode=block".to_owned(),
            "--who=swayconnect".to_owned(),
            "--why=Kept awake from a paired device".to_owned(),
            SLEEP.to_owned(),
            seconds.to_owned(),
        ]
    };
    assert_eq!(rig.inhibitor_argvs(), [inhibitor("3600"), inhibitor("60")]);
}

#[tokio::test(start_paused = true)]
async fn a_replace_kills_the_old_child_and_keeps_the_continuous_count() {
    let rig = rig();
    rig.set(1, on(Some(7200))).await.expect("on");
    tokio::time::advance(Duration::from_secs(7000)).await;
    rig.set(1, on(Some(7200))).await.expect("replace");
    assert!(rig.runner.was_killed(0));
    assert_eq!(rig.last_seconds(), "7200");

    tokio::time::advance(Duration::from_secs(7200)).await;
    rig.until_off().await;
    rig.set(1, on(Some(7200))).await.expect("on with the rest");
    assert_eq!(rig.last_seconds(), "200", "14400 - 7000 - 7200");
}

#[tokio::test(start_paused = true)]
async fn off_kills_the_child_and_clears_the_state() {
    let rig = rig();
    rig.set(1, on(None)).await.expect("on");
    assert!(rig.state(1).active);
    let result = rig.set(2, KeepAwakeArgs::Off).await.expect("off");
    assert_eq!(result.remaining_ms, None);
    assert!(rig.runner.was_killed(0));
    let state = rig.state(1);
    assert_eq!(
        (state.active, state.remaining_ms, state.holder),
        (false, None, None)
    );
}

#[tokio::test(start_paused = true)]
async fn the_end_of_the_sleep_clears_the_state() {
    let rig = rig();
    rig.set(1, on(Some(60))).await.expect("on");
    rig.runner.end_child(0, Exit::Code(0));
    rig.until_off().await;
    assert!(!rig.runner.was_killed(0));
}

#[tokio::test(start_paused = true)]
async fn at_the_expiry_a_child_still_running_is_killed() {
    let rig = rig();
    rig.set(1, on(Some(60))).await.expect("on");
    let started = Instant::now();
    rig.until_off().await;
    assert_eq!(Instant::now() - started, MINUTE);
    assert!(rig.runner.was_killed(0));
}

#[tokio::test(start_paused = true)]
async fn toggling_every_minute_still_reaches_the_cap_at_four_hours_of_on_time() {
    let rig = rig();
    for _ in 0..240 {
        rig.set(1, on(None)).await.expect("on within the cap");
        tokio::time::advance(MINUTE).await;
        rig.set(1, KeepAwakeArgs::Off).await.expect("off");
        tokio::time::advance(MINUTE).await;
    }
    assert_eq!(
        rig.last_seconds(),
        "60",
        "the last start gets only the rest"
    );
    let refused = rig.set(1, on(None)).await.expect_err("cap");
    assert_eq!(refused.code(), ErrorCode::LimitReached);
    assert_eq!(refused.retry_after_ms(), Some(840_000));
    assert_eq!(rig.state(1).cooldown_remaining_ms, Some(840_000));
    assert_eq!(rig.runner.spawned_count(), 240);
}

#[tokio::test(start_paused = true)]
async fn only_an_off_period_of_the_cooldown_resets_the_count() {
    let rig = rig();
    rig.set(1, on(Some(7200))).await.expect("on");
    tokio::time::advance(Duration::from_secs(7200)).await;
    rig.until_off().await;
    rig.set(1, on(Some(3600))).await.expect("on again");
    tokio::time::advance(HOUR).await;
    rig.until_off().await;
    tokio::time::advance(COOLDOWN - Duration::from_secs(1)).await;
    rig.set(1, on(Some(7200))).await.expect("short break");
    assert_eq!(rig.last_seconds(), "3600", "the count was kept");

    tokio::time::advance(HOUR).await;
    rig.until_off().await;
    assert!(rig.set(1, on(None)).await.is_err(), "4 h of on-time");
    tokio::time::advance(COOLDOWN).await;
    rig.keep_awake
        .view()
        .wait_for(|view| view.cooldown_until.is_none())
        .await
        .expect("the cooldown ends in the view");
    rig.set(1, on(Some(7200))).await.expect("after the break");
    assert_eq!(rig.last_seconds(), "7200", "the count was reset");
}

#[tokio::test(start_paused = true)]
async fn a_duration_above_the_maximum_is_a_bad_request() {
    let rig = rig();
    let refused = rig.set(1, on(Some(7201))).await.expect_err("too long");
    assert_eq!(refused.code(), ErrorCode::BadRequest);
    assert_eq!(rig.runner.spawned_count(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_failed_spawn_is_spawn_failed_and_keeps_the_old_state() {
    let rig = rig();
    rig.runner.fail_next_spawn();
    let refused = rig.set(1, on(None)).await.expect_err("no program");
    assert_eq!(refused.code(), ErrorCode::SpawnFailed);
    assert!(!rig.state(1).active);
    assert!(rig.notices().await.is_empty());

    rig.set(1, on(None)).await.expect("on");
    rig.runner.fail_next_spawn();
    rig.set(1, on(Some(60))).await.expect_err("replace fails");
    assert!(!rig.runner.was_killed(0), "the running inhibitor stays");
    assert!(rig.state(1).active);
}

/// Runs `set` while the inhibitor it starts ends at once with `exit`.
async fn set_while_the_child_ends(
    rig: &Rig,
    args: KeepAwakeArgs,
    exit: Exit,
) -> Result<KeepAwakeResult, KeepAwakeError> {
    let index = rig.runner.spawned_count();
    let keep_awake = rig.keep_awake.clone();
    let request = tokio::spawn(async move { keep_awake.set(&device(1), args).await });
    // Yielding keeps this task runnable, so the paused clock does not jump past the check.
    while rig.runner.spawned_count() == index {
        tokio::task::yield_now().await;
    }
    rig.runner.end_child(index, exit);
    request.await.expect("request task")
}

#[tokio::test(start_paused = true)]
async fn an_inhibitor_that_ends_at_once_is_refused_and_the_old_one_stays() {
    let rig = rig_with_check(START_CHECK);
    let refused = set_while_the_child_ends(&rig, on(None), Exit::Code(1))
        .await
        .expect_err("refused by logind");
    assert_eq!(refused.code(), ErrorCode::BackendUnavailable);
    assert!(!rig.state(1).active);
    assert!(
        rig.notices().await.is_empty(),
        "no notice for a failed start"
    );

    rig.set(1, on(None)).await.expect("on");
    let replaced = set_while_the_child_ends(&rig, on(Some(7200)), Exit::Code(1)).await;
    assert_eq!(
        replaced.expect_err("replace refused").code(),
        ErrorCode::BackendUnavailable
    );
    assert!(!rig.runner.was_killed(1), "the working inhibitor stays");
    let state = rig.state(1);
    assert!(state.active);
    assert_eq!(
        state.remaining_ms,
        Some(3_599_500),
        "the old expiry, which counts from before its own 0.5 s check"
    );
    assert_eq!(rig.notices().await.len(), 1, "only the working start");
}

#[tokio::test(start_paused = true)]
async fn after_the_panic_switch_a_queued_or_later_on_is_read_only() {
    let rig = rig();
    rig.set(1, on(None)).await.expect("on");
    rig.keep_awake.close().await;
    assert!(rig.runner.was_killed(0));
    assert!(!rig.state(1).active);
    let refused = rig.set(1, on(None)).await.expect_err("closed");
    assert_eq!(refused.code(), ErrorCode::ReadOnly);
    assert_eq!(
        rig.runner.spawned_count(),
        1,
        "no inhibitor after the switch"
    );
}

#[tokio::test(start_paused = true)]
async fn the_holder_is_per_device_and_a_revoke_ends_only_its_hold() {
    let rig = rig();
    rig.set(1, on(None)).await.expect("on");
    tokio::time::advance(MINUTE).await;
    let mine = rig.state(1);
    assert_eq!(mine.holder, Some(Holder::ThisDevice));
    assert_eq!(mine.remaining_ms, Some(3_540_000));
    assert_eq!(rig.state(2).holder, Some(Holder::OtherDevice));

    rig.keep_awake.release_holder(&device(2)).await;
    assert!(rig.state(1).active);
    rig.keep_awake.release_holder(&device(1)).await;
    assert!(!rig.state(1).active);
    assert!(rig.runner.was_killed(0));
}

#[tokio::test(start_paused = true)]
async fn the_panic_switch_and_shutdown_end_it() {
    let rig = rig();
    rig.set(1, on(None)).await.expect("on");
    rig.keep_awake.release().await;
    assert!(rig.runner.was_killed(0));

    rig.set(2, on(None)).await.expect("on again");
    rig.shutdown.send_replace(true);
    rig.until_off().await;
    assert!(rig.runner.was_killed(1));
    let refused = rig.set(2, on(None)).await.expect_err("stopped");
    assert_eq!(refused.code(), ErrorCode::BackendUnavailable);
}

#[tokio::test(start_paused = true)]
async fn outputs_are_powered_on_only_when_an_active_one_is_off() {
    let rig = rig();
    for (outputs, expected) in [
        (vec![output(true, true)], 0),
        (vec![output(false, false)], 0),
        (vec![output(true, true), output(true, false)], 1),
    ] {
        rig.desktop.send_replace(published(outputs));
        rig.set(1, on(None)).await.expect("on");
        rig.set(1, KeepAwakeArgs::Off).await.expect("off");
        assert_eq!(rig.power.calls.swap(0, Ordering::SeqCst), expected);
    }
}

#[tokio::test(start_paused = true)]
async fn a_notice_shows_at_each_start_and_at_a_take_over() {
    let rig = rig();
    rig.set(1, on(None)).await.expect("on");
    rig.set(1, on(Some(60)))
        .await
        .expect("replace by the holder");
    assert_eq!(rig.notices().await.len(), 1);
    rig.set(2, on(None)).await.expect("take over");
    rig.set(2, KeepAwakeArgs::Off).await.expect("off");
    rig.set(2, on(None)).await.expect("start");
    let notices = rig.notices().await;
    assert_eq!(notices.len(), 3);
    let expected_body = format!("Keep awake on (device {})", &device(2).as_str()[..4]);
    assert_eq!(notices[2].last(), Some(&expected_body));
}

#[test]
fn the_config_checks_hold_at_load() {
    let defaults = config("");
    assert_eq!(
        (
            defaults.default_duration_s,
            defaults.max_duration_s,
            defaults.max_continuous_s,
            defaults.cooldown_s,
        ),
        (3600, 7200, 14_400, 900)
    );
    let system = KeepAwakeConfig::default();
    assert_eq!(
        system.inhibit.as_path(),
        std::path::Path::new(config::DEFAULT_INHIBIT)
    );
    assert_eq!(
        system.sleep.as_path(),
        std::path::Path::new(config::DEFAULT_SLEEP)
    );
    assert_eq!(
        toml::from_str::<KeepAwakeConfig>("").expect("empty"),
        system
    );

    for refused in [
        "cooldown_s = 59",
        "max_continuous_s = 7199",
        "max_duration_s = 59\ndefault_duration_s = 59\nmax_continuous_s = 59",
        "default_duration_s = 7201",
        "default_duration_s = 59",
        "max_continuous_s = 604801",
        "extra = 1",
    ] {
        assert!(
            toml::from_str::<KeepAwakeConfig>(refused).is_err(),
            "{refused}"
        );
    }
    assert!(toml::from_str::<KeepAwakeConfig>(r#"inhibit = "systemd-inhibit""#).is_err());
    let edge = config("cooldown_s = 60\nmax_continuous_s = 7200");
    assert_eq!((edge.cooldown_s, edge.max_continuous_s), (60, 7200));
}
