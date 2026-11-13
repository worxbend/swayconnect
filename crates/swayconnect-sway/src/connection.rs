//! One sway IPC connection: a reader task, replies matched to requests in order, and a
//! timeout on every request (`docs/DESIGN.md` 4.3).

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::command::Command;
use crate::events::Event;
use crate::frame::{Frame, FrameError, MessageType, read_frame, write_frame};
use crate::types::{Node, Outcome, Version, WorkspaceInfo};

/// How long a request may take, from the first written byte to the reply.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

const EVENT_QUEUE_LEN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sway IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("sway IPC frame does not start with \"i3-ipc\"")]
    BadMagic,
    #[error("sway IPC payload of {0} bytes is over the limit")]
    FrameTooLarge(u64),
    #[error("sway did not reply within {} s", REQUEST_TIMEOUT.as_secs())]
    Timeout,
    #[error("the sway connection is closed")]
    Closed,
    #[error("sway replied with message type {got}, expected {expected}")]
    UnexpectedReply { expected: u32, got: u32 },
    #[error("sway sent a reply of type {0} that nobody asked for")]
    UnsolicitedReply(u32),
    #[error("sway reply is not the expected JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The text comes from sway and is not safe to show without escaping.
    #[error("sway refused the command: {0}")]
    CommandFailed(String),
    #[error("sway refused the event subscription")]
    SubscribeRefused,
}

impl From<FrameError> for Error {
    fn from(error: FrameError) -> Error {
        match error {
            FrameError::Io(error) => Error::Io(error),
            FrameError::BadMagic => Error::BadMagic,
            FrameError::TooLarge(len) => Error::FrameTooLarge(len),
        }
    }
}

pub(crate) fn parse<T: DeserializeOwned>(payload: &[u8]) -> Result<T, Error> {
    Ok(serde_json::from_slice(payload)?)
}

type ReplySender = oneshot::Sender<Result<Frame, Error>>;

/// Requests that wait for a reply, oldest first. Sway answers in request order.
/// `None` once the connection is closed.
#[derive(Debug)]
struct ReplyQueue(Mutex<Option<VecDeque<ReplySender>>>);

impl ReplyQueue {
    fn with<T>(&self, action: impl FnOnce(&mut Option<VecDeque<ReplySender>>) -> T) -> T {
        // The guarded value is valid after every statement, so a panic elsewhere cannot leave it broken.
        action(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn push(&self) -> Result<oneshot::Receiver<Result<Frame, Error>>, Error> {
        let (sender, receiver) = oneshot::channel();
        self.with(|waiting| waiting.as_mut().map(|queue| queue.push_back(sender)))
            .ok_or(Error::Closed)?;
        Ok(receiver)
    }

    fn pop(&self) -> Option<ReplySender> {
        self.with(|waiting| waiting.as_mut()?.pop_front())
    }

    /// Closes the queue. Returns the oldest waiter; the others see a dropped sender.
    fn close(&self) -> Option<ReplySender> {
        self.with(|waiting| waiting.take()?.pop_front())
    }
}

/// A connection to sway. Requests may be sent concurrently; replies come back in order.
///
/// Any transport error or timeout closes the connection, because the stream may be out of
/// step. Every later call returns [`Error::Closed`]; the owner drops it and connects again.
/// Dropping a request future half way has the same effect only if the write was cut, so
/// callers should not cancel requests.
#[derive(Debug)]
pub struct Connection {
    writer: tokio::sync::Mutex<OwnedWriteHalf>,
    waiting: Arc<ReplyQueue>,
    events: mpsc::Receiver<Result<Event, Error>>,
    reader: JoinHandle<()>,
}

impl Connection {
    pub async fn connect(socket: &Path) -> Result<Connection, Error> {
        let (read_half, write_half) = UnixStream::connect(socket).await?.into_split();
        let waiting = Arc::new(ReplyQueue(Mutex::new(Some(VecDeque::new()))));
        let (event_sender, events) = mpsc::channel(EVENT_QUEUE_LEN);
        let reader = tokio::spawn(read_frames(read_half, Arc::clone(&waiting), event_sender));
        Ok(Connection {
            writer: tokio::sync::Mutex::new(write_half),
            waiting,
            events,
            reader,
        })
    }

    pub async fn version(&self) -> Result<Version, Error> {
        parse(&self.request(MessageType::GetVersion, b"").await?)
    }

    pub async fn workspaces(&self) -> Result<Vec<WorkspaceInfo>, Error> {
        parse(&self.request(MessageType::GetWorkspaces, b"").await?)
    }

    pub async fn tree(&self) -> Result<Node, Error> {
        parse(&self.request(MessageType::GetTree, b"").await?)
    }

    /// Sends one command. `Ok` only when sway reports exactly one result and it is a success
    /// (`docs/DESIGN.md` 4.4 rule 5).
    pub async fn run(&self, command: Command) -> Result<(), Error> {
        let reply = self
            .request(MessageType::RunCommand, command.render().as_bytes())
            .await?;
        let outcomes: Vec<Outcome> = parse(&reply)?;
        match outcomes.as_slice() {
            [Outcome { success: true, .. }] => Ok(()),
            [Outcome { error, .. }] => Err(Error::CommandFailed(error.clone().unwrap_or_default())),
            _ => Err(Error::CommandFailed(format!(
                "{} results for one command",
                outcomes.len()
            ))),
        }
    }

    pub(crate) async fn request(
        &self,
        kind: MessageType,
        payload: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let reply = tokio::time::timeout(REQUEST_TIMEOUT, self.exchange(kind, payload))
            .await
            .unwrap_or(Err(Error::Timeout));
        if reply.is_err() {
            self.close();
        }
        reply
    }

    async fn exchange(&self, kind: MessageType, payload: &[u8]) -> Result<Vec<u8>, Error> {
        let reply = {
            // Held over both steps, so the queue order is the order on the wire.
            let mut writer = self.writer.lock().await;
            let reply = self.waiting.push()?;
            write_frame(&mut *writer, kind, payload).await?;
            reply
        };
        let frame = reply.await.map_err(|_| Error::Closed)??;
        if frame.code != kind.code() {
            return Err(Error::UnexpectedReply {
                expected: kind.code(),
                got: frame.code,
            });
        }
        Ok(frame.payload)
    }

    pub(crate) async fn next_event(&mut self) -> Result<Event, Error> {
        self.events.recv().await.unwrap_or(Err(Error::Closed))
    }

    fn close(&self) {
        self.waiting.close();
        self.reader.abort();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// The reader task. It is the only reader, so no frame is ever read half way.
async fn read_frames(
    mut socket: OwnedReadHalf,
    waiting: Arc<ReplyQueue>,
    events: mpsc::Sender<Result<Event, Error>>,
) {
    let error = loop {
        match read_frame(&mut socket).await {
            Ok(frame) if frame.is_event() => {
                // Never waits for the consumer. A full queue already holds a trigger, and a
                // closed one means nobody listens, so the event can go.
                let _ = events.try_send(Ok(Event::from_code(frame.code)));
            }
            Ok(frame) => match waiting.pop() {
                // A failed send means the requester stopped waiting. Order is still intact.
                Some(requester) => drop(requester.send(Ok(frame))),
                None => break Error::UnsolicitedReply(frame.code),
            },
            Err(error) => break Error::from(error),
        }
    };
    // The error goes to whoever waits: the oldest requester, else the event consumer.
    // Everyone after that sees `Closed`.
    match waiting.close() {
        Some(requester) => drop(requester.send(Err(error))),
        None => drop(events.try_send(Err(error))),
    }
}

#[cfg(test)]
mod tests;
