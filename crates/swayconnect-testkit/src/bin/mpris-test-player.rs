//! A test MPRIS player for the sealed session bus (`docs/M2C_PLAN.md` 4, slice R2).
//!
//! Usage: `mpris-test-player <bus address> <name> <log file>`. It owns
//! `org.mpris.MediaPlayer2.<name>` on that bus (it reads no environment), starts paused on track 1, and serves the MPRIS root
//! and player interfaces at `/org/mpris/MediaPlayer2`. Each method call appends its name
//! and a newline to the log file before it answers, so a caller that got the answer finds
//! the line. `PlayPause`, `Play`, `Pause` and `Stop` change `PlaybackStatus`; `Next` and
//! `Previous` change the track. Each change emits `PropertiesChanged`. It runs until it is
//! killed. `Seek`, `SetPosition`, `OpenUri` and the optional properties are left out.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::{OwnedVariant, Variant, interface};

const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
/// `DesktopEntry` of every test player.
const DESKTOP_ENTRY: &str = "swayconnect-test-player";
const ARTIST: &str = "Test Artist";
const ALBUM: &str = "Test Album";

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let (address, name, log) = arguments();
    let calls = CallLog { path: log };
    let _connection = zbus::connection::Builder::address(address.as_str())
        .serve_at(
            PLAYER_PATH,
            Root {
                identity: name.clone(),
                calls: calls.clone(),
            },
        )
        .serve_at(
            PLAYER_PATH,
            Player {
                playing: false,
                track: 1,
                calls,
            },
        )
        .name(format!("{BUS_PREFIX}{name}"))
        .build()
        .await?;
    std::future::pending::<()>().await;
    Ok(())
}

fn arguments() -> (String, String, PathBuf) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match <[String; 3]>::try_from(args) {
        Ok([address, name, log]) => (address, name, PathBuf::from(log)),
        Err(_) => panic!("usage: mpris-test-player <bus address> <name> <log file>"),
    }
}

/// Where each method call is written, one name per line.
#[derive(Clone)]
struct CallLog {
    path: PathBuf,
}

impl CallLog {
    fn record(&self, method: &str) -> fdo::Result<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| writeln!(file, "{method}"))
            .map_err(|error| fdo::Error::Failed(format!("cannot log {method}: {error}")))
    }
}

struct Root {
    identity: String,
    calls: CallLog,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) -> fdo::Result<()> {
        self.calls.record("Raise")
    }

    fn quit(&self) -> fdo::Result<()> {
        self.calls.record("Quit")
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        DESKTOP_ENTRY.to_owned()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    playing: bool,
    /// `Track <n>` is the title.
    track: i64,
    calls: CallLog,
}

impl Player {
    async fn set_playing(
        &mut self,
        method: &str,
        playing: bool,
        emitter: &SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.calls.record(method)?;
        self.playing = playing;
        self.playback_status_changed(emitter).await?;
        Ok(())
    }

    async fn skip(
        &mut self,
        method: &str,
        by: i64,
        emitter: &SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.calls.record(method)?;
        self.track += by;
        self.metadata_changed(emitter).await?;
        Ok(())
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn play_pause(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let playing = !self.playing;
        self.set_playing("PlayPause", playing, &emitter).await
    }

    async fn play(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.set_playing("Play", true, &emitter).await
    }

    async fn pause(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.set_playing("Pause", false, &emitter).await
    }

    async fn stop(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.set_playing("Stop", false, &emitter).await
    }

    async fn next(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.skip("Next", 1, &emitter).await
    }

    async fn previous(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.skip("Previous", -1, &emitter).await
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing { "Playing" } else { "Paused" }.to_owned()
    }

    #[zbus(property)]
    fn metadata(&self) -> fdo::Result<HashMap<String, OwnedVariant>> {
        let title = format!("Track {}", self.track);
        Ok(HashMap::from([
            ("xesam:title".to_owned(), owned(title.into())?),
            ("xesam:artist".to_owned(), owned(vec![ARTIST].into())?),
            ("xesam:album".to_owned(), owned(ALBUM.into())?),
        ]))
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
}

fn owned(value: Variant<'_>) -> fdo::Result<OwnedVariant> {
    OwnedVariant::try_from(value).map_err(|error| fdo::Error::Failed(error.to_string()))
}
