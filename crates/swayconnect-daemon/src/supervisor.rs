//! Sway connect, subscribe, refresh, reconnect. Written by slice S6b.
//!
//! One task owns both sway connections and the published snapshot (`docs/DESIGN.md` 4.3).
//! Slice S6d added the action queue: the same task sends every `RUN_COMMAND`.
//! Slice D-POLL added the 2 s poll while clients subscribe and the idle-inhibit count
//! (`docs/DESIGN.md` 11.5, D23). Slice D-INT.1 added the window icons: the tree is read again
//! when the icon service finishes a job.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use swayconnect_protocol::Desktop;
use swayconnect_protocol::text::escape_for_terminal;
use swayconnect_sway::{Connection, Event, EventStream, Node, SwayTarget, Version, resolve};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinError;
use tracing::{debug, info, warn};

use crate::icons::{IconKey, IconService};
use crate::sway_handle::{ActionError, ActionRequest, Published};
use crate::{actions, model};

/// Events come in bursts; one refresh after the burst is enough.
const REFRESH_DELAY: Duration = Duration::from_millis(50);
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
/// Sway sends no event for a resize, so the tree is read again this often while a client
/// subscribes to the desktop.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

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

/// What this task publishes, the actions it is asked to run and how many clients
/// subscribe to the desktop.
pub(crate) struct Desk {
    pub(crate) state: watch::Sender<Arc<Published>>,
    /// Windows that keep the desktop awake by themselves (sway `inhibit_idle`).
    pub(crate) windows_inhibiting: watch::Sender<u32>,
    pub(crate) subscribers: watch::Receiver<usize>,
    pub(crate) actions: mpsc::Receiver<ActionRequest>,
    /// `None`: every window icon is `null`.
    pub(crate) icons: Option<Arc<IconService>>,
    /// [`IconService::finished`] of `icons`.
    pub(crate) icon_jobs: Option<watch::Receiver<u64>>,
}

/// Why the snapshot is read again.
enum Trigger {
    Event,
    Poll,
    /// The icon service finished a job, so an icon may be ready now.
    IconJob,
}

/// Runs until `shutdown` turns `true` or closes, or until nobody reads the state any more.
pub(crate) async fn run(target: SwayTarget, mut shutdown: watch::Receiver<bool>, mut desk: Desk) {
    let state = desk.state.clone();
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
        desk.windows_inhibiting
            .send_if_modified(|count| set_if_changed(count, 0));
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
        refresh(&link, desk).await?;
        if let Trigger::Event = serve_until_change(&mut link, desk).await? {
            wait_for_burst_end(&mut link.events).await?;
        }
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

/// Runs the queued actions, one at a time, until sway reports a change or a poll is due.
async fn serve_until_change(link: &mut Link, desk: &mut Desk) -> Result<Trigger, Disconnect> {
    let poll = poll_due(&mut desk.subscribers);
    tokio::pin!(poll);
    let icon_job = icon_job_finished(desk.icon_jobs.as_mut());
    tokio::pin!(icon_job);
    loop {
        tokio::select! {
            event = link.events.next() => return check_event(event?).map(|()| Trigger::Event),
            () = &mut poll => return Ok(Trigger::Poll),
            () = &mut icon_job => return Ok(Trigger::IconJob),
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

/// Ends once `POLL_INTERVAL` has passed with at least one subscriber all the time. Never
/// ends while nobody subscribes.
async fn poll_due(subscribers: &mut watch::Receiver<usize>) {
    loop {
        if subscribers.wait_for(|count| *count > 0).await.is_err() {
            // No handle is left that could add a subscriber.
            return std::future::pending().await;
        }
        let interval = tokio::time::sleep(POLL_INTERVAL);
        tokio::pin!(interval);
        loop {
            tokio::select! {
                () = &mut interval => return,
                changed = subscribers.changed() => {
                    if changed.is_err() || *subscribers.borrow_and_update() == 0 {
                        break;
                    }
                }
            }
        }
    }
}

/// Ends when the icon service finishes a job. Never ends without a service.
async fn icon_job_finished(finished: Option<&mut watch::Receiver<u64>>) {
    let has_finished = match finished {
        Some(finished) => finished.changed().await.is_ok(),
        None => false,
    };
    if !has_finished {
        // No service, or its worker has stopped: no job will finish.
        std::future::pending::<()>().await;
    }
}

async fn refresh(link: &Link, desk: &Desk) -> Result<(), swayconnect_sway::Error> {
    let tree = link.commands.tree().await?;
    let workspaces = link.commands.workspaces().await?;
    let desktop = match &desk.icons {
        // Never waits: an unknown app is queued and its icon stays `null` until the job ends.
        Some(service) => {
            let icon_of = |node: &Node| {
                // The pid feeds the flatpak cgroup hint; sway reports it as a signed integer.
                let pid = node.pid.and_then(|pid| u32::try_from(pid).ok());
                service.icon_for(&IconKey::of_window(node), pid)
            };
            model::online_desktop_with_icons(&tree, &workspaces, &link.version, &icon_of)
        }
        None => model::online_desktop(&tree, &workspaces, &link.version),
    };
    publish_if_changed(&desk.state, desktop);
    let inhibiting = count_inhibiting(&tree);
    desk.windows_inhibiting
        .send_if_modified(|count| set_if_changed(count, inhibiting));
    Ok(())
}

/// Sway sets `inhibit_idle` on views only, so every marked node is one window.
fn count_inhibiting(node: &Node) -> u32 {
    let own = u32::from(node.inhibit_idle == Some(true));
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .map(count_inhibiting)
        .fold(own, u32::saturating_add)
}

fn set_if_changed(current: &mut u32, new: u32) -> bool {
    let is_changed = *current != new;
    *current = new;
    is_changed
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
        Event::Workspace | Event::Output | Event::Window | Event::Binding | Event::Other(_) => {
            Ok(())
        }
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
mod tests;
