//! [`FakeMedia`]: a [`MediaBackend`] for tests. It reports the players a test sets, records
//! each command it is sent, and answers as the test says. Never touches a bus.

use std::sync::{Mutex, MutexGuard, PoisonError};

use swayconnect_protocol::MediaCommand;
use tokio::sync::watch;

use super::{BackendError, MediaBackend, PlayerReport, Players};
use crate::proc::BoxFuture;

/// How the fake answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeAnswer {
    Ok,
    /// The player is gone or refused: [`BackendError::Unavailable`].
    Fails,
    /// Never answers.
    Hangs,
}

#[derive(Debug)]
pub struct FakeMedia {
    inner: Mutex<Inner>,
    changes: watch::Sender<u64>,
}

#[derive(Debug)]
struct Inner {
    players: Result<Vec<PlayerReport>, BackendError>,
    unread: Vec<String>,
    answer: FakeAnswer,
    sent: Vec<(String, MediaCommand)>,
}

impl Default for FakeMedia {
    fn default() -> FakeMedia {
        FakeMedia {
            inner: Mutex::new(Inner {
                players: Ok(Vec::new()),
                unread: Vec::new(),
                answer: FakeAnswer::Ok,
                sent: Vec::new(),
            }),
            changes: watch::channel(0).0,
        }
    }
}

impl FakeMedia {
    /// Sets what [`MediaBackend::players`] reports and signals a change.
    pub fn set_players(&self, players: Result<Vec<PlayerReport>, BackendError>) {
        self.lock().players = players;
        self.changes.send_modify(|count| *count += 1);
    }

    /// Sets the bus names that are present but whose read fails, and signals a change.
    pub fn set_unread(&self, unread: Vec<String>) {
        self.lock().unread = unread;
        self.changes.send_modify(|count| *count += 1);
    }

    pub fn set_answer(&self, answer: FakeAnswer) {
        self.lock().answer = answer;
    }

    /// Every command sent so far, answered or not, in order, as `(bus name, command)`.
    pub fn sent(&self) -> Vec<(String, MediaCommand)> {
        self.lock().sent.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl MediaBackend for FakeMedia {
    fn players(&self) -> BoxFuture<'_, Result<Players, BackendError>> {
        let inner = self.lock();
        let unread = inner.unread.clone();
        let players = inner
            .players
            .clone()
            .map(|reports| Players { reports, unread });
        Box::pin(async move { players })
    }

    fn send<'a>(
        &'a self,
        bus_name: &'a str,
        command: MediaCommand,
    ) -> BoxFuture<'a, Result<(), BackendError>> {
        let answer = {
            let mut inner = self.lock();
            inner.sent.push((bus_name.to_owned(), command));
            inner.answer
        };
        Box::pin(async move {
            match answer {
                FakeAnswer::Ok => Ok(()),
                FakeAnswer::Fails => Err(BackendError::Unavailable("fake failure".to_owned())),
                FakeAnswer::Hangs => std::future::pending().await,
            }
        })
    }

    fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}
