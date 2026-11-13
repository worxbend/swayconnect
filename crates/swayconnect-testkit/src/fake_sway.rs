//! A fake sway IPC server on a Unix socket (`docs/DESIGN.md` 9.1).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, watch};
use tokio::task::{JoinHandle, JoinSet};

use crate::frame::{read_frame, write_frame};
use crate::guard::checked_socket_path;
use crate::{SWAY_SOCKET_NAME, fixtures};

const EVENT_BIT: u32 = 0x8000_0000;
const EVENT_BACKLOG: usize = 64;
const SUBSCRIBE_REFUSED: &str = r#"{"success":false}"#;

/// The message types the fake answers. Every other type gets no reply, as in sway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Request {
    RunCommand = 0,
    GetWorkspaces = 1,
    Subscribe = 2,
    GetTree = 4,
    GetVersion = 7,
}

impl Request {
    fn from_wire(message_type: u32) -> Option<Self> {
        [
            Self::RunCommand,
            Self::GetWorkspaces,
            Self::Subscribe,
            Self::GetTree,
            Self::GetVersion,
        ]
        .into_iter()
        .find(|request| *request as u32 == message_type)
    }
}

/// The events the daemon subscribes to (`docs/DESIGN.md` 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Workspace = 0,
    Output = 1,
    Window = 3,
    Shutdown = 6,
}

impl Event {
    fn name(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Output => "output",
            Self::Window => "window",
            Self::Shutdown => "shutdown",
        }
    }
}

/// Fake sway. Listens until dropped; the drop closes every connection and removes the socket.
#[derive(Debug)]
pub struct FakeSway {
    socket_path: PathBuf,
    shared: Arc<Shared>,
    accept_task: JoinHandle<()>,
}

#[derive(Debug)]
struct Shared {
    scripted_replies: Mutex<HashMap<Request, String>>,
    commands: Mutex<Vec<String>>,
    answering: AtomicBool,
    events: broadcast::Sender<(Event, String)>,
    close_generation: watch::Sender<u64>,
}

impl FakeSway {
    /// Listens on `dir/sway-ipc.sock` ([`SWAY_SOCKET_NAME`]). See [`start_at`](Self::start_at).
    pub fn start(dir: &Path, forbidden: &[PathBuf]) -> Self {
        Self::start_at(&dir.join(SWAY_SOCKET_NAME), dir, forbidden)
    }

    /// Listens on `socket` with the default fixtures as replies.
    ///
    /// Must be called inside a tokio runtime.
    ///
    /// # Panics
    ///
    /// When `socket` is outside `dir` after symlinks are resolved, equals a path in
    /// `forbidden`, already exists, or cannot be bound. The test harness passes the
    /// paths of any real sway socket as `forbidden`.
    pub fn start_at(socket: &Path, dir: &Path, forbidden: &[PathBuf]) -> Self {
        let socket_path = checked_socket_path(socket, dir, forbidden);
        let listener = UnixListener::bind(&socket_path).unwrap_or_else(|error| {
            panic!("fake sway cannot bind {}: {error}", socket_path.display())
        });
        let shared = Arc::new(Shared::new());
        let accept_task = tokio::spawn(accept_connections(listener, Arc::clone(&shared)));
        Self {
            socket_path,
            shared,
            accept_task,
        }
    }

    /// The bound path, free of symlinks.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Sets the payload sent for every later `request`. The text is sent as given, so a
    /// test can script a malformed reply. A `SUBSCRIBE` whose payload is not a JSON
    /// array of strings is refused as sway refuses it, whatever is scripted.
    pub fn set_reply(&self, request: Request, payload: impl Into<String>) {
        lock(&self.shared.scripted_replies).insert(request, payload.into());
    }

    /// Sends an event to every connection that subscribed to it.
    pub fn send_event(&self, event: Event, payload: impl Into<String>) {
        // An error only says that no connection is open; then nobody is subscribed.
        let _ = self.shared.events.send((event, payload.into()));
    }

    /// Requests are still read and recorded, but get no reply until
    /// [`resume_answering`](Self::resume_answering).
    pub fn stop_answering(&self) {
        self.shared.answering.store(false, Ordering::SeqCst);
    }

    /// Ends [`stop_answering`](Self::stop_answering). Requests read in between stay unanswered.
    pub fn resume_answering(&self) {
        self.shared.answering.store(true, Ordering::SeqCst);
    }

    /// Closes every open connection. The listener stays, so a client can reconnect.
    pub fn close_connections(&self) {
        self.shared
            .close_generation
            .send_modify(|generation| *generation += 1);
    }

    /// Every `RUN_COMMAND` payload received so far, oldest first, over all connections.
    /// Bytes that are not UTF-8 show as U+FFFD.
    pub fn commands(&self) -> Vec<String> {
        lock(&self.shared.commands).clone()
    }
}

impl Drop for FakeSway {
    fn drop(&mut self) {
        // Aborting the accept task drops its JoinSet, which aborts every connection.
        self.accept_task.abort();
        // Nothing can be reported from a drop. A file left behind makes the next
        // `start` on this path panic, so the failure is not silent.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Shared {
    fn new() -> Self {
        Self {
            scripted_replies: Mutex::new(HashMap::new()),
            commands: Mutex::new(Vec::new()),
            answering: AtomicBool::new(true),
            events: broadcast::channel(EVENT_BACKLOG).0,
            close_generation: watch::channel(0).0,
        }
    }

    fn reply(&self, request: Request) -> String {
        lock(&self.scripted_replies)
            .get(&request)
            .cloned()
            .unwrap_or_else(|| default_reply(request))
    }
}

fn default_reply(request: Request) -> String {
    match request {
        Request::RunCommand => r#"[{"success":true}]"#.to_owned(),
        Request::GetWorkspaces => fixtures::workspaces(),
        Request::Subscribe => r#"{"success":true}"#.to_owned(),
        Request::GetTree => fixtures::tree(),
        Request::GetVersion => fixtures::version(),
    }
}

/// A panic in a test that holds the lock must not hide the state from the next reader.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn accept_connections(listener: UnixListener, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                connections.spawn(serve_connection(stream, Arc::clone(&shared)));
            }
            Err(error) => {
                eprintln!("fake sway: accept failed, no further connections: {error}");
                return;
            }
        }
        while connections.try_join_next().is_some() {}
    }
}

struct Connection {
    shared: Arc<Shared>,
    writer: tokio::sync::Mutex<OwnedWriteHalf>,
    subscriptions: Mutex<HashSet<String>>,
}

async fn serve_connection(stream: UnixStream, shared: Arc<Shared>) {
    let events = shared.events.subscribe();
    let mut closed = shared.close_generation.subscribe();
    let (reader, writer) = stream.into_split();
    let connection = Connection {
        shared,
        writer: tokio::sync::Mutex::new(writer),
        subscriptions: Mutex::new(HashSet::new()),
    };
    // Each branch ends the connection, so no half-read frame is ever resumed. An I/O
    // error is the peer going away or sending garbage; like sway, the fake just hangs up.
    tokio::select! {
        _ = connection.answer_requests(reader) => {}
        _ = connection.forward_events(events) => {}
        _ = closed.changed() => {}
    }
}

impl Connection {
    async fn answer_requests(&self, mut reader: OwnedReadHalf) -> io::Result<()> {
        loop {
            let frame = read_frame(&mut reader).await?;
            if let Some(request) = Request::from_wire(frame.message_type) {
                self.answer(request, &frame.payload).await?;
            }
        }
    }

    async fn answer(&self, request: Request, payload: &[u8]) -> io::Result<()> {
        // Decided before the command is recorded: a test that sees the record and then
        // calls `resume_answering` can rely on this request staying unanswered.
        let answering = self.shared.answering.load(Ordering::SeqCst);
        if request == Request::RunCommand {
            lock(&self.shared.commands).push(String::from_utf8_lossy(payload).into_owned());
        }
        if !answering {
            return Ok(());
        }
        // The writer is held from before the subscription is recorded until the reply is
        // out, so no event can overtake the reply to its own `SUBSCRIBE`.
        let mut writer = self.writer.lock().await;
        let reply = self.reply_to(request, payload);
        write_frame(&mut *writer, request as u32, reply.as_bytes()).await
    }

    /// Picks the reply and, for a valid `SUBSCRIBE`, records the subscription.
    fn reply_to(&self, request: Request, payload: &[u8]) -> String {
        if request == Request::Subscribe {
            let Ok(event_names) = serde_json::from_slice::<Vec<String>>(payload) else {
                return SUBSCRIBE_REFUSED.to_owned();
            };
            lock(&self.subscriptions).extend(event_names);
        }
        self.shared.reply(request)
    }

    async fn forward_events(
        &self,
        mut events: broadcast::Receiver<(Event, String)>,
    ) -> io::Result<()> {
        loop {
            let (event, payload) = match events.recv().await {
                Ok(sent) => sent,
                // More than EVENT_BACKLOG events were injected while the peer did not read.
                // Sway drops events for a slow reader too.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return Ok(()),
            };
            if !lock(&self.subscriptions).contains(event.name()) {
                continue;
            }
            let message_type = EVENT_BIT | event as u32;
            write_frame(
                &mut *self.writer.lock().await,
                message_type,
                payload.as_bytes(),
            )
            .await?;
        }
    }
}
