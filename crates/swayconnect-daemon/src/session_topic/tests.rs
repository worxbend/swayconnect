//! A fake `/proc` in a temp dir and `FakeRunner` only (`docs/DESIGN.md` 11.12).

use std::path::Path;

use swayconnect_protocol::Holder;

use super::*;
use crate::keep_awake::Hold;
use crate::lockstate::add_fake_process;
use crate::proc::Exit;
use crate::proc::fake::FakeRunner;

const OWNER: u32 = 1000;
const SWAYLOCK_PID: u32 = 4321;
const LOCK_SCRIPT: &str = "/fake/lock.sh";

fn view() -> KeepAwakeView {
    KeepAwakeView {
        hold: None,
        cooldown_until: None,
        default_duration_s: 3600,
        max_duration_s: 7200,
        max_continuous_s: 14_400,
    }
}

fn scanner(root: &Path) -> Arc<ProcScanner> {
    Arc::new(ProcScanner::new(
        root.to_owned(),
        OWNER,
        &SessionConfig::default(),
    ))
}

fn device(byte: u8) -> DeviceId {
    DeviceId::from_bytes([byte; 16])
}

struct Rig {
    proc_root: tempfile::TempDir,
    topic: SessionTopic,
    keep_awake: watch::Sender<KeepAwakeView>,
    windows_inhibiting: watch::Sender<u32>,
    _shutdown: watch::Sender<bool>,
}

fn rig() -> Rig {
    let proc_root = tempfile::tempdir().expect("temp dir");
    let (keep_awake, keep_awake_seen) = watch::channel(view());
    let (windows_inhibiting, inhibiting_seen) = watch::channel(0);
    let (shutdown, stop) = watch::channel(false);
    let sources = SessionSources {
        keep_awake: keep_awake_seen,
        windows_inhibiting: inhibiting_seen,
        scanner: scanner(proc_root.path()),
    };
    Rig {
        topic: SessionTopic::spawn(sources, stop),
        proc_root,
        keep_awake,
        windows_inhibiting,
        _shutdown: shutdown,
    }
}

impl Rig {
    async fn until(&self, wanted: impl Fn(&SessionFacts) -> bool) -> Arc<PublishedSession> {
        let mut state = self.topic.state();
        let published = state
            .wait_for(|published| wanted(&published.facts))
            .await
            .expect("the topic task runs");
        Arc::clone(&published)
    }

    fn lock_screen(&self) {
        add_fake_process(self.proc_root.path(), SWAYLOCK_PID, "swaylock", OWNER);
    }

    fn unlock_screen(&self) {
        std::fs::remove_dir_all(self.proc_root.path().join(SWAYLOCK_PID.to_string()))
            .expect("remove the fake locker");
    }
}

#[tokio::test(start_paused = true)]
async fn lock_state_is_polled_only_while_session_has_subscribers() {
    let rig = rig();
    rig.lock_screen();
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    assert_eq!(rig.topic.state().borrow().facts.locked, None, "no scan yet");

    let interest = rig.topic.watch_session();
    rig.until(|facts| facts.locked == Some(true)).await;

    rig.unlock_screen();
    let before = Instant::now();
    rig.until(|facts| facts.locked == Some(false)).await;
    assert!(Instant::now() - before <= LOCK_POLL, "within one poll");

    drop(interest);
    rig.until(|facts| facts.locked.is_none()).await;
    let rev = rig.topic.state().borrow().rev;
    rig.lock_screen();
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    let published = rig.topic.state().borrow().clone();
    assert_eq!((published.rev, published.facts.locked), (rev, None));
}

#[tokio::test(start_paused = true)]
async fn an_unreadable_proc_is_unknown() {
    let rig = rig();
    std::fs::remove_dir(rig.proc_root.path()).expect("remove the fake /proc");
    let _interest = rig.topic.watch_session();
    tokio::time::advance(LOCK_POLL * 3).await;
    tokio::task::yield_now().await;
    assert_eq!(rig.topic.state().borrow().facts.locked, None);
}

#[tokio::test(start_paused = true)]
async fn each_source_change_publishes_one_new_rev() {
    let rig = rig();
    let first = rig.topic.state().borrow().rev;
    rig.windows_inhibiting.send_replace(2);
    let published = rig.until(|facts| facts.windows_inhibiting == 2).await;
    assert_eq!(published.rev, first + 1);

    let expires_at = Instant::now() + Duration::from_secs(60);
    rig.keep_awake.send_modify(|view| {
        view.hold = Some(Hold {
            holder: device(1),
            expires_at,
        });
    });
    let published = rig.until(|facts| facts.keep_awake.hold.is_some()).await;
    assert_eq!(published.rev, first + 2);

    rig.windows_inhibiting.send_replace(2);
    tokio::task::yield_now().await;
    assert_eq!(
        rig.topic.state().borrow().rev,
        first + 2,
        "equal state, no publish"
    );
}

#[test]
fn the_state_is_built_per_device() {
    let now = Instant::now();
    let mut keep_awake = view();
    keep_awake.hold = Some(Hold {
        holder: device(1),
        expires_at: now + Duration::from_secs(90),
    });
    let facts = SessionFacts {
        keep_awake,
        windows_inhibiting: 1,
        locked: Some(false),
    };
    let mine = facts.state_for(&device(1), now);
    assert_eq!(mine.keep_awake.holder, Some(Holder::ThisDevice));
    assert_eq!(mine.keep_awake.remaining_ms, Some(90_000));
    assert_eq!((mine.windows_inhibiting, mine.locked), (1, Some(false)));
    assert_eq!((mine.input, mine.launcher), (None, None));
    let theirs = facts.state_for(&device(2), now);
    assert_eq!(theirs.keep_awake.holder, Some(Holder::OtherDevice));
}

fn locker(runner: &FakeRunner, proc_root: &Path) -> Locker {
    let config: SessionConfig =
        toml::from_str(&format!("lock = [\"{LOCK_SCRIPT}\", \"--now\"]")).expect("config");
    Locker::new(Arc::new(runner.clone()), &config, scanner(proc_root)).expect("configured")
}

#[test]
fn without_a_lock_command_the_op_is_not_offered() {
    let root = tempfile::tempdir().expect("temp dir");
    let runner: Arc<dyn Runner> = Arc::new(FakeRunner::default());
    assert!(Locker::new(runner, &SessionConfig::default(), scanner(root.path())).is_none());
}

#[tokio::test]
async fn one_lock_child_at_a_time() {
    let root = tempfile::tempdir().expect("temp dir");
    let runner = FakeRunner::default();
    let locker = locker(&runner, root.path());

    assert!(!locker.lock().await.expect("lock").already_locked);
    assert!(locker.lock().await.expect("lock").already_locked);
    assert_eq!(runner.argvs(), [[LOCK_SCRIPT, "--now"]]);

    runner.end_child(0, Exit::Code(0));
    assert!(!locker.lock().await.expect("lock").already_locked);
    assert_eq!(runner.spawned_count(), 2);
}

#[tokio::test]
async fn a_locked_screen_starts_nothing() {
    let root = tempfile::tempdir().expect("temp dir");
    add_fake_process(root.path(), SWAYLOCK_PID, "swaylock", OWNER);
    let runner = FakeRunner::default();
    assert!(
        locker(&runner, root.path())
            .lock()
            .await
            .expect("lock")
            .already_locked
    );
    assert_eq!(runner.spawned_count(), 0);
}

#[tokio::test]
async fn unknown_lock_state_starts_the_lock_command() {
    let root = tempfile::tempdir().expect("temp dir");
    let runner = FakeRunner::default();
    let locker = locker(&runner, &root.path().join("missing"));
    assert!(!locker.lock().await.expect("lock").already_locked);
    assert_eq!(runner.spawned_count(), 1);
}

#[tokio::test]
async fn a_failed_start_is_spawn_failed() {
    let root = tempfile::tempdir().expect("temp dir");
    let runner = FakeRunner::default();
    runner.fail_next_spawn();
    let error = locker(&runner, root.path())
        .lock()
        .await
        .expect_err("no program");
    assert_eq!(error.code(), ErrorCode::SpawnFailed);
}
