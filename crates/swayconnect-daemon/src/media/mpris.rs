//! [`MprisBackend`]: MPRIS players on the session bus through zbus (`docs/DESIGN.md` D26,
//! 11.4). The bus address is passed in (`super::session_bus_address`); zbus never reads the
//! environment.
//!
//! One task owns the connection: it connects with backoff, listens for `NameOwnerChanged`
//! of MPRIS names and `PropertiesChanged` on the MPRIS object path, and bumps the change
//! counter for each. The players are read fresh on each [`MediaBackend::players`] call.

use std::collections::HashMap;
use std::future::poll_fn;
use std::io;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use swayconnect_protocol::MediaCommand;
use swayconnect_protocol::text::escape_for_terminal;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::sleep;
// zbus streams implement only the `futures-core` trait; zbus re-exports that crate.
use zbus::export::futures_core::Stream;
use zbus::{Connection, Message, MessageStream, OwnedVariant, Variant};

use super::{BackendError, MPRIS_PREFIX, MediaBackend, PlayerReport, Players};
use crate::proc::BoxFuture;
use crate::session::stopped;

/// At most this many MPRIS names are read per refresh; the topic shows at most 16.
pub const MAX_QUERIED_PLAYERS: usize = 64;
/// Artists of one track joined into `artist`.
const MAX_ARTISTS: usize = 16;
/// Every method call on the bus: a player that takes longer counts as not answering.
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const RECONNECT_FIRST: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(60);
/// Signals queued per match rule; older ones are dropped, which is fine for a change hint.
const QUEUED_SIGNALS: NonZeroUsize = NonZeroUsize::new(64).expect("not zero");
/// Variant layers unwrapped around one value.
const MAX_VARIANT_LAYERS: usize = 4;

const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const NAME_RULE: &str = "type='signal',sender='org.freedesktop.DBus',\
    interface='org.freedesktop.DBus',member='NameOwnerChanged',\
    arg0namespace='org.mpris.MediaPlayer2'";
const PROPERTIES_RULE: &str = "type='signal',interface='org.freedesktop.DBus.Properties',\
    member='PropertiesChanged',path='/org/mpris/MediaPlayer2'";

#[derive(Debug)]
pub struct MprisBackend {
    connection: Mutex<Option<Connection>>,
    changes: watch::Sender<u64>,
}

impl MprisBackend {
    /// Starts the connection task. Without an address it never connects, and every call is
    /// [`BackendError::Unavailable`]. The task ends when `shutdown` turns `true` or closes.
    /// Must be called inside a tokio runtime.
    pub fn spawn(address: Option<String>, shutdown: watch::Receiver<bool>) -> Arc<MprisBackend> {
        let backend = Arc::new(MprisBackend {
            connection: Mutex::new(None),
            changes: watch::channel(0).0,
        });
        match address {
            Some(address) => {
                tokio::spawn(keep_connected(Arc::clone(&backend), address, shutdown));
            }
            None => tracing::info!("no session bus address; media is unavailable"),
        }
        backend
    }

    fn connection(&self) -> Result<Connection, BackendError> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| BackendError::Unavailable("not connected to the session bus".into()))
    }

    fn set_connection(&self, connection: Option<Connection>) {
        *self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = connection;
        self.bump();
    }

    fn bump(&self) {
        self.changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

impl MediaBackend for MprisBackend {
    fn players(&self) -> BoxFuture<'_, Result<Players, BackendError>> {
        let connection = self.connection();
        Box::pin(async move { list_players(connection?).await })
    }

    fn send<'a>(
        &'a self,
        bus_name: &'a str,
        command: MediaCommand,
    ) -> BoxFuture<'a, Result<(), BackendError>> {
        let connection = self.connection();
        Box::pin(async move {
            connection?
                .call_method(
                    Some(bus_name),
                    PLAYER_PATH,
                    Some(PLAYER_INTERFACE),
                    method_name(command),
                    &(),
                )
                .await
                .map_err(backend_error)?;
            Ok(())
        })
    }

    fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}

/// The MPRIS method of a command.
pub fn method_name(command: MediaCommand) -> &'static str {
    match command {
        MediaCommand::PlayPause => "PlayPause",
        MediaCommand::Next => "Next",
        MediaCommand::Previous => "Previous",
    }
}

async fn keep_connected(
    backend: Arc<MprisBackend>,
    address: String,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut delay = RECONNECT_FIRST;
    loop {
        match connect(&address).await {
            Ok((connection, names, properties)) => {
                tracing::info!("connected to the session bus for media");
                delay = RECONNECT_FIRST;
                backend.set_connection(Some(connection));
                tokio::select! {
                    () = stopped(&mut shutdown) => {
                        backend.set_connection(None);
                        return;
                    }
                    () = forward_signals(names, properties, &backend) => {}
                }
                backend.set_connection(None);
                tracing::warn!("session bus connection lost; media is unavailable");
            }
            Err(error) => {
                let error = escape_for_terminal(&error.to_string());
                tracing::debug!(%error, "session bus not reachable");
            }
        }
        tokio::select! {
            () = stopped(&mut shutdown) => return,
            () = sleep(delay) => {}
        }
        delay = (delay * 2).min(RECONNECT_MAX);
    }
}

async fn connect(address: &str) -> zbus::Result<(Connection, MessageStream, MessageStream)> {
    let connection = zbus::connection::Builder::address(address)
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await?;
    let names = MessageStream::for_match_rule(NAME_RULE, &connection, Some(QUEUED_SIGNALS)).await?;
    let properties =
        MessageStream::for_match_rule(PROPERTIES_RULE, &connection, Some(QUEUED_SIGNALS)).await?;
    Ok((connection, names, properties))
}

/// Bumps the counter per signal. Returns when a stream ends: the connection is gone.
async fn forward_signals(
    mut names: MessageStream,
    mut properties: MessageStream,
    backend: &MprisBackend,
) {
    loop {
        let signal = tokio::select! {
            signal = next_message(&mut names) => signal,
            signal = next_message(&mut properties) => signal,
        };
        match signal {
            Some(Ok(_)) => backend.bump(),
            Some(Err(error)) => {
                let error = escape_for_terminal(&error.to_string());
                tracing::debug!(%error, "bad message on the session bus");
            }
            None => return,
        }
    }
}

async fn next_message(stream: &mut MessageStream) -> Option<zbus::Result<Message>> {
    poll_fn(|context| Pin::new(&mut *stream).poll_next(context)).await
}

async fn list_players(connection: Connection) -> Result<Players, BackendError> {
    let reply = connection
        .call_method(Some(BUS_NAME), BUS_PATH, Some(BUS_NAME), "ListNames", &())
        .await
        .map_err(backend_error)?;
    let names: Vec<String> = reply.body().deserialize().map_err(backend_error)?;
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .collect();
    names.sort_unstable();
    if names.len() > MAX_QUERIED_PLAYERS {
        tracing::warn!(
            count = names.len(),
            "more MPRIS players than {MAX_QUERIED_PLAYERS}; the rest are not shown"
        );
        names.truncate(MAX_QUERIED_PLAYERS);
    }
    // Read in parallel, so one player that does not answer costs one timeout, not one each.
    let mut reads = JoinSet::new();
    for (index, name) in names.iter().cloned().enumerate() {
        let connection = connection.clone();
        reads.spawn(async move { (index, read_player(&connection, name).await) });
    }
    let mut players = Vec::new();
    let mut read = vec![false; names.len()];
    while let Some(joined) = reads.join_next().await {
        match joined {
            Ok((index, Ok(player))) => {
                read[index] = true;
                players.push((index, player));
            }
            Ok((_, Err(error))) => tracing::debug!(%error, "MPRIS player skipped"),
            Err(error) => tracing::warn!(%error, "MPRIS player read failed"),
        }
    }
    players.sort_unstable_by_key(|(index, _)| *index);
    let unread = names
        .into_iter()
        .zip(read)
        .filter_map(|(name, read)| (!read).then_some(name))
        .collect();
    Ok(Players {
        reports: players.into_iter().map(|(_, player)| player).collect(),
        unread,
    })
}

async fn read_player(
    connection: &Connection,
    bus_name: String,
) -> Result<PlayerReport, BackendError> {
    let root = properties(connection, &bus_name, ROOT_INTERFACE).await?;
    let player = properties(connection, &bus_name, PLAYER_INTERFACE).await?;
    Ok(report_of(bus_name, &root, &player))
}

async fn properties(
    connection: &Connection,
    bus_name: &str,
    interface: &str,
) -> Result<HashMap<String, OwnedVariant>, BackendError> {
    let reply = connection
        .call_method(
            Some(bus_name),
            PLAYER_PATH,
            Some(PROPERTIES_INTERFACE),
            "GetAll",
            &(interface,),
        )
        .await
        .map_err(backend_error)?;
    reply.body().deserialize().map_err(backend_error)
}

/// Builds the report from the two property maps. A missing or mistyped property counts as
/// absent; with `CanControl` false every `Can*` is false (MPRIS).
pub fn report_of(
    bus_name: String,
    root: &HashMap<String, OwnedVariant>,
    player: &HashMap<String, OwnedVariant>,
) -> PlayerReport {
    let track = player
        .get("Metadata")
        .map(|metadata| Track::of(metadata))
        .unwrap_or_default();
    let can_control = flag(player, "CanControl").unwrap_or(true);
    let can = |key| can_control && flag(player, key).unwrap_or(false);
    PlayerReport {
        identity: text_property(root, "Identity"),
        desktop_entry: text_property(root, "DesktopEntry"),
        status: text_property(player, "PlaybackStatus").unwrap_or_default(),
        title: track.title,
        artist: track.artist,
        album: track.album,
        can_play: can("CanPlay"),
        can_pause: can("CanPause"),
        can_go_next: can("CanGoNext"),
        can_go_previous: can("CanGoPrevious"),
        bus_name,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Track {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

impl Track {
    /// `xesam:title`, `xesam:artist` (a list, joined with ", "; a plain string is taken too)
    /// and `xesam:album` of the MPRIS `Metadata` map.
    fn of(metadata: &Variant<'_>) -> Track {
        let mut track = Track::default();
        let Variant::Dict(entries) = unwrapped(metadata) else {
            return track;
        };
        for (key, value) in entries.iter() {
            match <&str>::try_from(unwrapped(key)) {
                Ok("xesam:title") => track.title = text(value),
                Ok("xesam:artist") => track.artist = artists(value),
                Ok("xesam:album") => track.album = text(value),
                _ => {}
            }
        }
        track
    }
}

fn artists(value: &Variant<'_>) -> Option<String> {
    let Variant::Array(list) = unwrapped(value) else {
        return text(value);
    };
    let names: Vec<&str> = list
        .inner()
        .iter()
        .filter_map(|name| <&str>::try_from(unwrapped(name)).ok())
        .filter(|name| !name.is_empty())
        .take(MAX_ARTISTS)
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

fn text_property(properties: &HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    properties.get(key).and_then(|value| text(value))
}

/// A string value; an empty string counts as absent.
fn text(value: &Variant<'_>) -> Option<String> {
    <&str>::try_from(unwrapped(value))
        .ok()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn flag(properties: &HashMap<String, OwnedVariant>, key: &str) -> Option<bool> {
    properties
        .get(key)
        .and_then(|value| bool::try_from(unwrapped(value)).ok())
}

fn unwrapped<'v, 'a>(mut value: &'v Variant<'a>) -> &'v Variant<'a> {
    for _ in 0..MAX_VARIANT_LAYERS {
        match value {
            Variant::Variant(inner) => value = inner,
            _ => break,
        }
    }
    value
}

/// A timeout is [`BackendError::Timeout`]; every other failure is unavailable. The text is
/// for the log only and escaped, because a player chooses its error messages.
fn backend_error(error: zbus::Error) -> BackendError {
    match &error {
        zbus::Error::InputOutput(io_error) if io_error.kind() == io::ErrorKind::TimedOut => {
            BackendError::Timeout
        }
        _ => BackendError::Unavailable(escape_for_terminal(&error.to_string())),
    }
}

#[cfg(test)]
mod tests;
