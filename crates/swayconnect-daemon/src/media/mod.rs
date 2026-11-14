//! Topic `media` and the `media.*` operations (`docs/PROTOCOL.md` 17.10; `docs/DESIGN.md`
//! 11.4, 11.6).
//!
//! A [`MediaBackend`] reports the MPRIS players and sends their commands. The real one is
//! [`mpris::MprisBackend`] (zbus on the session bus); tests use [`fake::FakeMedia`].
//! [`MediaService`] issues the player ids, builds the topic state (`facts`) and answers the
//! ops. Every string a player reports is untrusted: it is cleaned and cut before it is kept,
//! and titles are never logged.

use std::ffi::OsStr;
use std::path::Path;

use swayconnect_protocol::{ErrorCode, MediaCommand};
use tokio::sync::watch;

use crate::proc::BoxFuture;

pub mod facts;
pub mod fake;
pub mod mpris;
pub mod service;

pub use facts::{MediaFacts, TrackedPlayer};
pub use service::{MediaService, PublishedMedia};

/// Bus name prefix of an MPRIS player.
pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// The proxy `playerctld` mirrors other players; listing it would show each one twice.
pub const PLAYERCTLD: &str = "org.mpris.MediaPlayer2.playerctld";

/// One read of the bus: the players that answered, and the names of those that are on the
/// bus but whose read failed. An unread player keeps its id and marks (`Tracker`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Players {
    pub reports: Vec<PlayerReport>,
    pub unread: Vec<String>,
}

/// One player as its backend reports it. Every string is raw and untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerReport {
    pub bus_name: String,
    /// MPRIS `Identity`.
    pub identity: Option<String>,
    /// MPRIS `DesktopEntry`: the icon hint.
    pub desktop_entry: Option<String>,
    /// MPRIS `PlaybackStatus` as sent: `Playing`, `Paused`, `Stopped` or anything else.
    pub status: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

impl PlayerReport {
    pub fn is_playing(&self) -> bool {
        self.status == "Playing"
    }
}

/// Why a backend call failed.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BackendError {
    /// No bus, the bus is down, or the player is gone or refused the call.
    #[error("media backend unavailable: {0}")]
    Unavailable(String),
    /// No answer in time.
    #[error("the media player did not answer in time")]
    Timeout,
}

/// The refusals of a `media.*` op after the generic checks (17.10).
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum MediaError {
    #[error("no player with this id")]
    NotFound,
    #[error("the player does not allow this command")]
    UnsupportedTarget,
    #[error(transparent)]
    Backend(#[from] BackendError),
}

impl MediaError {
    pub fn code(&self) -> ErrorCode {
        match self {
            MediaError::NotFound => ErrorCode::NotFound,
            MediaError::UnsupportedTarget => ErrorCode::UnsupportedTarget,
            MediaError::Backend(BackendError::Unavailable(_)) => ErrorCode::BackendUnavailable,
            MediaError::Backend(BackendError::Timeout) => ErrorCode::Timeout,
        }
    }
}

/// Where the players live. Implementations must be cheap to share behind an `Arc`.
pub trait MediaBackend: Send + Sync + 'static {
    /// Every player on the bus whose name starts with [`MPRIS_PREFIX`], in a stable order.
    /// A player that fails to answer is in [`Players::unread`]; an error means the bus
    /// itself failed.
    fn players(&self) -> BoxFuture<'_, Result<Players, BackendError>>;

    /// Sends one command to the player that owns `bus_name`.
    fn send<'a>(
        &'a self,
        bus_name: &'a str,
        command: MediaCommand,
    ) -> BoxFuture<'a, Result<(), BackendError>>;

    /// A counter that grows each time the players may have changed (a player came or went,
    /// a property changed, the bus connection came or went).
    fn changes(&self) -> watch::Receiver<u64>;
}

/// The session bus address of `docs/DESIGN.md` 11.4: `DBUS_SESSION_BUS_ADDRESS` when set
/// and not empty, else `unix:path=$XDG_RUNTIME_DIR/bus`. `None` when neither is known or
/// the runtime dir is not valid UTF-8. Only `main.rs` reads the environment and passes the
/// values in; zbus never reads it.
pub fn session_bus_address(
    dbus_session_bus_address: Option<&OsStr>,
    xdg_runtime_dir: Option<&Path>,
) -> Option<String> {
    if let Some(address) = dbus_session_bus_address.filter(|value| !value.is_empty()) {
        return address.to_str().map(str::to_owned);
    }
    let runtime_dir = xdg_runtime_dir.filter(|dir| dir.is_absolute())?;
    let path = runtime_dir.join("bus");
    Some(format!(
        "unix:path={}",
        escape_address_value(path.to_str()?)
    ))
}

/// D-Bus address value escaping: bytes outside `-0-9A-Za-z_/.\*` become `%XX`.
fn escape_address_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_/.\\*".contains(&byte) {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

#[cfg(test)]
mod tests;
