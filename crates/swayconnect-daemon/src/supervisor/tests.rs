use std::io;

use serde_json::Value;
use swayconnect_testkit::{FakeSway, Request, TestDirs, fixtures};
use tokio::time::Instant;

use super::*;

const TEST_DEADLINE: Duration = Duration::from_secs(10);

fn offline_state() -> watch::Sender<Arc<Published>> {
    let initial = Published {
        rev: 0,
        desktop: model::offline_desktop(),
    };
    watch::channel(Arc::new(initial)).0
}

/// A desk that nobody sends an action to and nobody subscribes to.
fn desk_of(state: &watch::Sender<Arc<Published>>) -> Desk {
    Desk {
        state: state.clone(),
        windows_inhibiting: watch::channel(0).0,
        subscribers: watch::channel(0).1,
        actions: mpsc::channel(1).1,
        icons: None,
        icon_jobs: None,
    }
}

fn desktop_with_version(version: &str) -> Desktop {
    let mut desktop = model::offline_desktop();
    desktop.sway.online = true;
    desktop.sway.version = Some(version.to_owned());
    desktop
}

/// The default tree with another title on the first window.
fn tree_with_first_title(title: &str) -> String {
    let mut tree: Value = serde_json::from_str(&fixtures::tree()).expect("fixture is JSON");
    tree["nodes"][0]["nodes"][0]["nodes"][0]["name"] = title.into();
    tree.to_string()
}

/// The default tree with the first window inhibiting idle.
fn tree_with_first_window_inhibiting() -> String {
    let mut tree: Value = serde_json::from_str(&fixtures::tree()).expect("fixture is JSON");
    tree["nodes"][0]["nodes"][0]["nodes"][0]["inhibit_idle"] = true.into();
    tree.to_string()
}

struct Supervised {
    // Dropping the sender stops the task, so it lives as long as the test needs it.
    shutdown: watch::Sender<bool>,
    state: watch::Receiver<Arc<Published>>,
    windows_inhibiting: watch::Receiver<u32>,
    subscribers: watch::Sender<usize>,
}

impl Supervised {
    fn start(target: SwayTarget) -> Supervised {
        let (shutdown, stop) = watch::channel(false);
        let publisher = offline_state();
        let state = publisher.subscribe();
        let (inhibit_publisher, windows_inhibiting) = watch::channel(0);
        let (subscribers, subscriber_count) = watch::channel(0);
        let desk = Desk {
            state: publisher,
            windows_inhibiting: inhibit_publisher,
            subscribers: subscriber_count,
            actions: mpsc::channel(1).1,
            icons: None,
            icon_jobs: None,
        };
        tokio::spawn(run(target, stop, desk));
        Supervised {
            shutdown,
            state,
            windows_inhibiting,
            subscribers,
        }
    }

    /// Started against the fake sway in `dirs`, after its first snapshot.
    async fn online(dirs: &TestDirs) -> Supervised {
        let mut supervised = Supervised::start(SwayTarget::Explicit(dirs.sway_socket()));
        assert!(supervised.next_publish().await.desktop.sway.online);
        supervised
    }

    fn has_published(&self) -> bool {
        self.state.has_changed().expect("supervisor runs")
    }

    async fn next_publish(&mut self) -> Arc<Published> {
        tokio::time::timeout(TEST_DEADLINE, self.state.changed())
            .await
            .expect("a publish in time")
            .expect("supervisor runs");
        Arc::clone(&self.state.borrow_and_update())
    }
}

#[test]
fn the_retry_delay_doubles_from_250_ms_to_5_s_and_resets() {
    let mut retry = RetryDelay::new();

    let delays_ms: Vec<u128> = (0..7).map(|_| retry.next().as_millis()).collect();
    assert_eq!(delays_ms, [250, 500, 1000, 2000, 4000, 5000, 5000]);

    retry.reset();
    assert_eq!(retry.next(), FIRST_RETRY_DELAY);
}

#[test]
fn publishing_an_equal_desktop_changes_nothing() {
    let state = offline_state();
    let mut reader = state.subscribe();

    publish_if_changed(&state, model::offline_desktop());
    assert!(!reader.has_changed().expect("sender lives"));
    assert_eq!(reader.borrow().rev, 0);

    publish_if_changed(&state, desktop_with_version("1"));
    publish_if_changed(&state, desktop_with_version("1"));
    assert!(reader.has_changed().expect("sender lives"));
    assert_eq!(reader.borrow_and_update().rev, 1);

    publish_if_changed(&state, desktop_with_version("2"));
    assert_eq!(reader.borrow().rev, 2);
}

#[tokio::test(start_paused = true)]
async fn failed_connects_are_retried_with_backoff() {
    let state = offline_state();
    let reader = state.subscribe();
    let started = Instant::now();
    let mut attempts_ms = Vec::new();
    let failing_connect = || {
        attempts_ms.push(started.elapsed().as_millis());
        std::future::ready(Err(Disconnect::NoSocket))
    };

    let mut desk = desk_of(&state);
    let supervised = supervise(failing_connect, &mut desk);
    let stopped = tokio::time::timeout(Duration::from_secs(20), supervised).await;

    assert!(stopped.is_err(), "the supervisor never gives up");
    assert_eq!(attempts_ms, [0, 250, 750, 1750, 3750, 7750, 12750, 17750]);
    assert_eq!(reader.borrow().rev, 0, "offline to offline is no publish");
}

#[tokio::test(start_paused = true)]
async fn losing_sway_publishes_offline_once_and_restarts_the_backoff() {
    let state = offline_state();
    let mut reader = state.subscribe();
    let started = Instant::now();
    let mut attempts_ms = Vec::new();
    // Stands in for a session that was online and then lost its connection.
    let online_on_third_attempt = || {
        attempts_ms.push(started.elapsed().as_millis());
        if attempts_ms.len() == 3 {
            publish_if_changed(&state, desktop_with_version("1"));
        }
        std::future::ready(Err(Disconnect::Exiting))
    };

    let mut desk = desk_of(&state);
    let supervised = supervise(online_on_third_attempt, &mut desk);
    let stopped = tokio::time::timeout(Duration::from_secs(2), supervised).await;

    assert!(stopped.is_err());
    assert_eq!(attempts_ms, [0, 250, 750, 1000, 1500]);
    let published = reader.borrow_and_update();
    assert_eq!(published.rev, 2, "online, then offline");
    assert!(!published.desktop.sway.online);
    assert!(published.desktop.workspaces.is_empty() && published.desktop.windows.is_empty());
}

#[tokio::test]
async fn a_snapshot_is_published_only_when_it_differs_and_rev_grows_by_one() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::start(SwayTarget::Explicit(dirs.sway_socket()));

    let first = supervised.next_publish().await;
    assert_eq!(first.rev, 1);
    assert!(first.desktop.sway.online);
    assert_eq!(first.desktop.workspaces.len(), 2);
    assert_eq!(first.desktop.windows.len(), 2);

    // Same replies: this refresh must not publish, so the next publish is rev 2.
    sway.send_event(swayconnect_testkit::Event::Window, "{}");
    sway.set_reply(Request::GetTree, tree_with_first_title("Changed title"));
    sway.send_event(swayconnect_testkit::Event::Window, "{}");

    let second = supervised.next_publish().await;
    assert_eq!(second.rev, 2);
    assert_eq!(
        second.desktop.windows[0].title.as_deref(),
        Some("Changed title")
    );
}

#[tokio::test]
async fn eof_publishes_offline_and_the_supervisor_connects_again() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::start(SwayTarget::Explicit(dirs.sway_socket()));
    let online = supervised.next_publish().await;

    sway.close_connections();

    let offline = supervised.next_publish().await;
    assert_eq!(offline.rev, online.rev + 1);
    assert!(offline.desktop == model::offline_desktop());
    let back = supervised.next_publish().await;
    assert_eq!(back.rev, online.rev + 2);
    assert!(back.desktop == online.desktop);
}

#[tokio::test]
async fn a_shutdown_event_publishes_offline() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::start(SwayTarget::Explicit(dirs.sway_socket()));
    supervised.next_publish().await;

    sway.send_event(swayconnect_testkit::Event::Shutdown, "{}");

    assert!(!supervised.next_publish().await.desktop.sway.online);
}

#[tokio::test]
async fn an_explicit_target_opens_that_path_only() {
    let dirs = TestDirs::new();
    // A live fake sway next to the configured path must not be picked up.
    let _decoy = FakeSway::start(&dirs.runtime_dir(), &[]);
    let configured = dirs.runtime_dir().join("configured.sock");

    let missing = connect(SwayTarget::Explicit(configured.clone())).await;

    assert!(matches!(
        missing,
        Err(Disconnect::Sway(swayconnect_sway::Error::Io(error)))
            if error.kind() == io::ErrorKind::NotFound
    ));

    let sway = FakeSway::start_at(&configured, &dirs.runtime_dir(), &[]);
    sway.set_reply(Request::GetVersion, r#"{"human_readable":"configured"}"#);
    let link = connect(SwayTarget::Explicit(configured)).await;

    let version = link.map(|link| link.version.human_readable).ok();
    assert_eq!(version.as_deref(), Some("configured"));
}

#[tokio::test]
async fn the_task_ends_on_shutdown() {
    let dirs = TestDirs::new();
    let _sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::start(SwayTarget::Explicit(dirs.sway_socket()));
    supervised.next_publish().await;

    supervised.shutdown.send(true).expect("supervisor listens");

    // The task owns the only sender of the state.
    let closed = tokio::time::timeout(TEST_DEADLINE, supervised.state.changed()).await;
    assert!(matches!(closed, Ok(Err(_))));
}

#[test]
fn a_json_error_is_logged_without_the_reply_text() {
    let json_error = serde_json::from_str::<u64>(r#""a window title""#).expect_err("not a u64");
    assert!(json_error.to_string().contains("a window title"));

    let logged = Disconnect::Sway(json_error.into()).to_string();

    assert!(!logged.contains("a window title"));
    assert!(logged.contains("line 1"));
}

#[test]
fn sway_error_text_is_escaped_for_the_log() {
    let refused = swayconnect_sway::Error::CommandFailed("bad\u{1b}[2J".to_owned());

    let logged = Disconnect::Sway(refused).to_string();

    assert!(logged.contains("bad\\u{1b}[2J"));
}

#[tokio::test(start_paused = true)]
async fn without_a_subscriber_the_tree_is_not_polled() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let supervised = Supervised::online(&dirs).await;
    sway.set_reply(Request::GetTree, tree_with_first_title("Resized"));

    tokio::time::sleep(POLL_INTERVAL * 5).await;

    assert!(!supervised.has_published());
}

#[tokio::test(start_paused = true)]
async fn a_subscriber_starts_the_poll_and_its_leaving_stops_it() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::online(&dirs).await;
    sway.set_reply(Request::GetTree, tree_with_first_title("Resized"));
    let subscribed_at = Instant::now();

    supervised.subscribers.send_replace(1);

    let polled = supervised.next_publish().await;
    assert_eq!(subscribed_at.elapsed(), POLL_INTERVAL);
    assert_eq!(polled.desktop.windows[0].title.as_deref(), Some("Resized"));

    supervised.subscribers.send_replace(0);
    sway.set_reply(Request::GetTree, tree_with_first_title("Resized again"));
    tokio::time::sleep(POLL_INTERVAL * 5).await;

    assert!(!supervised.has_published());
}

#[tokio::test(start_paused = true)]
async fn the_poll_publishes_once_per_real_change_and_never_for_an_equal_tree() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::online(&dirs).await;
    let rev_before = supervised.state.borrow().rev;
    supervised.subscribers.send_replace(1);

    tokio::time::sleep(POLL_INTERVAL * 5).await;
    assert!(!supervised.has_published(), "five polls of an equal tree");

    sway.set_reply(Request::GetTree, tree_with_first_title("Resized"));
    tokio::time::sleep(POLL_INTERVAL * 5).await;

    let published = supervised.state.borrow_and_update();
    assert_eq!(published.rev, rev_before + 1, "five polls, one change");
    assert_eq!(
        published.desktop.windows[0].title.as_deref(),
        Some("Resized")
    );
}

#[tokio::test(start_paused = true)]
async fn a_binding_event_refreshes_without_a_subscriber() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    let mut supervised = Supervised::online(&dirs).await;
    sway.set_reply(Request::GetTree, tree_with_first_title("Resized"));
    let sent_at = Instant::now();

    sway.send_event(swayconnect_testkit::Event::Binding, "{}");

    let refreshed = supervised.next_publish().await;
    assert_eq!(sent_at.elapsed(), REFRESH_DELAY);
    assert_eq!(
        refreshed.desktop.windows[0].title.as_deref(),
        Some("Resized")
    );
}

#[tokio::test(start_paused = true)]
async fn the_inhibit_count_follows_the_tree_and_is_zero_offline() {
    let dirs = TestDirs::new();
    let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
    sway.set_reply(Request::GetTree, tree_with_first_window_inhibiting());
    let mut supervised = Supervised::online(&dirs).await;
    assert_eq!(*supervised.windows_inhibiting.borrow_and_update(), 1);

    drop(sway);

    let offline = supervised.next_publish().await;
    assert!(!offline.desktop.sway.online);
    assert_eq!(*supervised.windows_inhibiting.borrow(), 0);
}

#[test]
fn inhibiting_windows_are_counted_in_tiled_and_floating_children() {
    let tree: Node = serde_json::from_str(
        r#"{"id":1,"type":"root","nodes":[{"id":2,"type":"workspace",
            "nodes":[{"id":3,"type":"con","inhibit_idle":true},
                     {"id":4,"type":"con","nodes":[{"id":5,"type":"con","inhibit_idle":true},
                                                   {"id":6,"type":"con","inhibit_idle":false}]}],
            "floating_nodes":[{"id":7,"type":"floating_con","inhibit_idle":true},
                              {"id":8,"type":"floating_con"}]}]}"#,
    )
    .expect("tree is JSON");

    assert_eq!(count_inhibiting(&tree), 3);
}
