//! Sway connect, subscribe, refresh, reconnect. Written by slice S6b.
//!
//! One task owns both sway connections and the published snapshot (`docs/DESIGN.md` 4.3).
//! Slice S6d added the action queue: the same task sends every `RUN_COMMAND`.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use swayconnect_protocol::Desktop;
use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_sway::{Connection, Event, EventStream, SwayTarget, Version, resolve};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinError;
use tracing::{debug, info, warn};

use crate::sway_handle::{ActionError, ActionRequest, Published};
use crate::{actions, model};

/// Events come in bursts; one refresh after the burst is enough.
const REFRESH_DELAY: Duration = Duration::from_millis(50);
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Why there is no sway connection.
#[derive(Debug, thiserror::Error)]
enum Disconnect {
    #[error("no sway socket found")]
    NoSocket,
    #[error("the socket lookup stopped: {0}")]
    Lookup(#[from] JoinError),
    #[error("sway is exiting")]
    Exiting,
    #[error("{}", loggable(.0))]
    Sway(#[from] swayconnect_sway::Error),
}

/// A JSON error can quote the reply, and the reply holds window titles: only its place is
/// kept. Other errors can carry sway's own text, which is escaped.
fn loggable(error: &swayconnect_sway::Error) -> String {
    match error {
        swayconnect_sway::Error::Json(json) => format!(
            "sway reply is not the expected JSON ({:?} error at line {}, column {})",
            json.classify(),
            json.line(),
            json.column()
        ),
        other => escape_for_terminal(&other.to_string()),
    }
}

/// Both connections to one sway, and what does not change while they live.
struct Link {
    commands: Connection,
    events: EventStream,
    version: Version,
}

/// Wait before the next connect attempt: 250 ms, doubling to 5 s.
struct RetryDelay(Duration);

impl RetryDelay {
    fn new() -> RetryDelay {
        RetryDelay(FIRST_RETRY_DELAY)
    }

    fn next(&mut self) -> Duration {
        let delay = self.0;
        self.0 = (delay * 2).min(MAX_RETRY_DELAY);
        delay
    }

    fn reset(&mut self) {
        self.0 = FIRST_RETRY_DELAY;
    }
}

/// The snapshot this task publishes and the actions it is asked to run.
struct Desk {
    state: watch::Sender<Arc<Published>>,
    actions: mpsc::Receiver<ActionRequest>,
}

/// Runs until `shutdown` turns `true` or closes, or until nobody reads the state any more.
pub(crate) async fn run(
    target: SwayTarget,
    mut shutdown: watch::Receiver<bool>,
    state: watch::Sender<Arc<Published>>,
    actions: mpsc::Receiver<ActionRequest>,
) {
    let mut desk = Desk {
        state: state.clone(),
        actions,
    };
    // Dropping the other branch drops both sway connections; nothing is resumed afterwards.
    tokio::select! {
        _ = shutdown.wait_for(|stop| *stop) => {}
        () = state.closed() => {}
        never = supervise(|| connect(target.clone()), &mut desk) => match never {},
    }
}

/// `connect` is a parameter so that a test can count attempts without a socket.
async fn supervise<C, F>(mut connect: C, desk: &mut Desk) -> Infallible
where
    C: FnMut() -> F,
    F: Future<Output = Result<Link, Disconnect>>,
{
    let mut retry = RetryDelay::new();
    loop {
        let Err(reason) = follow_sway(&mut connect, desk).await;
        let state = &desk.state;
        let was_online = state.borrow().desktop.sway.online;
        if was_online {
            retry.reset();
            info!(%reason, "sway disconnected");
        } else {
            debug!(%reason, "sway is not reachable");
        }
        publish_if_changed(state, model::offline_desktop());
        refusing_actions(&mut desk.actions, tokio::time::sleep(retry.next())).await;
    }
}

async fn follow_sway<C, F>(connect: &mut C, desk: &mut Desk) -> Result<Infallible, Disconnect>
where
    C: FnMut() -> F,
    F: Future<Output = Result<Link, Disconnect>>,
{
    let mut link = refusing_actions(&mut desk.actions, connect()).await?;
    let version = escape_for_terminal(&link.version.human_readable);
    info!(%version, "sway connected");
    loop {
        refresh(&link, &desk.state).await?;
        serve_until_event(&mut link, desk).await?;
        wait_for_burst_end(&mut link.events).await?;
    }
}

/// Runs `work` to its end. An action that arrives meanwhile is refused at once, so that
/// none waits in the queue and runs late, after a reconnect.
async fn refusing_actions<T>(
    actions: &mut mpsc::Receiver<ActionRequest>,
    work: impl Future<Output = T>,
) -> T {
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return output,
            Some(request) = actions.recv() => {
                answer(request, Err(ActionError::SwayUnavailable));
            }
        }
    }
}

fn answer(request: ActionRequest, outcome: Result<(), ActionError>) {
    // A failed send means the session stopped waiting; there is nobody left to tell.
    let _ = request.reply.send(outcome);
}

/// Runs the queued actions, one at a time, until sway reports a change.
async fn serve_until_event(link: &mut Link, desk: &mut Desk) -> Result<(), Disconnect> {
    loop {
        tokio::select! {
            event = link.events.next() => return check_event(event?),
            Some(request) = desk.actions.recv() => {
                perform(&link.commands, &desk.state, request).await?;
            }
        }
    }
}

/// The only caller of `RUN_COMMAND`. The target is checked against the published snapshot
/// first, so an id the clients cannot see is never sent (`docs/DESIGN.md` 4.4).
async fn perform(
    commands: &Connection,
    state: &watch::Sender<Arc<Published>>,
    request: ActionRequest,
) -> Result<(), Disconnect> {
    let command = match actions::command_for(request.action, &state.borrow().desktop) {
        Ok(command) => command,
        Err(refusal) => {
            answer(request, Err(refusal));
            return Ok(());
        }
    };
    let Err(error) = commands.run(command).await else {
        answer(request, Ok(()));
        return Ok(());
    };
    let failure = actions::failure(&error);
    // After these two the connection is closed and must be rebuilt.
    let is_link_lost = matches!(failure, ActionError::Timeout | ActionError::SwayUnavailable);
    answer(request, Err(failure));
    if is_link_lost {
        return Err(Disconnect::Sway(error));
    }
    warn!(reason = %loggable(&error), ?command, "sway did not accept a command");
    Ok(())
}

/// Resolves the socket again on every attempt: a restarted sway has a new path.
async fn connect(target: SwayTarget) -> Result<Link, Disconnect> {
    // `resolve` reads the file system and may try a blocking connect.
    let socket = tokio::task::spawn_blocking(move || resolve(&target))
        .await?
        .ok_or(Disconnect::NoSocket)?;
    let commands = Connection::connect(&socket).await?;
    // Subscribed before the first snapshot, so no change between the two is missed.
    let events = EventStream::open(&socket).await?;
    let version = commands.version().await?;
    Ok(Link {
        commands,
        events,
        version,
    })
}

async fn refresh(
    link: &Link,
    state: &watch::Sender<Arc<Published>>,
) -> Result<(), swayconnect_sway::Error> {
    let tree = link.commands.tree().await?;
    let workspaces = link.commands.workspaces().await?;
    let desktop = model::online_desktop(&tree, &workspaces, &link.version);
    publish_if_changed(state, desktop);
    Ok(())
}

/// Returns `REFRESH_DELAY` after the first event. Events are triggers only.
async fn wait_for_burst_end(events: &mut EventStream) -> Result<(), Disconnect> {
    let burst_end = tokio::time::sleep(REFRESH_DELAY);
    tokio::pin!(burst_end);
    loop {
        tokio::select! {
            () = &mut burst_end => return Ok(()),
            event = events.next() => check_event(event?)?,
        }
    }
}

fn check_event(event: Event) -> Result<(), Disconnect> {
    match event {
        Event::Shutdown => Err(Disconnect::Exiting),
        Event::Workspace | Event::Output | Event::Window | Event::Other(_) => Ok(()),
    }
}

fn publish_if_changed(state: &watch::Sender<Arc<Published>>, desktop: Desktop) {
    state.send_if_modified(|published| {
        if published.desktop == desktop {
            return false;
        }
        *published = Arc::new(Published {
            rev: published.rev + 1,
            desktop,
        });
        true
    });
}

#[cfg(test)]
mod tests {
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

    /// A desk that nobody sends an action to.
    fn desk_of(state: &watch::Sender<Arc<Published>>) -> Desk {
        Desk {
            state: state.clone(),
            actions: mpsc::channel(1).1,
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

    struct Supervised {
        // Dropping the sender stops the task, so it lives as long as the test needs it.
        shutdown: watch::Sender<bool>,
        state: watch::Receiver<Arc<Published>>,
    }

    impl Supervised {
        fn start(target: SwayTarget) -> Supervised {
            let (shutdown, stop) = watch::channel(false);
            let publisher = offline_state();
            let state = publisher.subscribe();
            tokio::spawn(run(target, stop, publisher, mpsc::channel(1).1));
            Supervised { shutdown, state }
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
}
