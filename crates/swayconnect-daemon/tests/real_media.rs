//! Media against a real session bus (`docs/M2C_PLAN.md` 4, slice R2): a daemon in this
//! process runs its real zbus MPRIS backend on the private bus of a `RealSession`, and the
//! players are the testkit's `mpris-test-player`, started in the session. Each player logs
//! every method call it gets, so the test sees which player a command reached. No other
//! program runs: every program path of the config does not exist. Runs only with the
//! `real-backends` feature: `just real`.

#![cfg(feature = "real-backends")]

mod support;

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use support::live::LiveAudio;
use support::{Pairing, refusal, soon};
use swayconnect_cli::client::{Event, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::services::MediaHost;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::media::session_bus_address;
use swayconnect_daemon::paths::Paths;
use swayconnect_protocol::ops::{Args, TOPIC_MEDIA};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{ErrorCode, Media, MediaCommand, Player, PlayerStatus, Scope};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{
    LiveEnv, RealSession, TestDirs, leftovers_naming, live_processes_in_group, mpris_test_player,
    nonexistent_programs,
};

const PAIRING_WINDOW: Duration = Duration::from_secs(300);
const MEDIA_SCOPES: [Scope; 3] = [Scope::StateRead, Scope::StateTitles, Scope::Media];
/// What `mpris-test-player` reports besides its name and track.
const DESKTOP_ENTRY: &str = "swayconnect-test-player";
const ARTIST: &str = "Test Artist";
const ALBUM: &str = "Test Album";

/// A sealed session and a daemon whose media backend is on the session's bus.
struct RealMedia {
    /// The live sound server before the test; the same after it.
    live_before: Option<LiveAudio>,
    daemon: DaemonHandle,
    paths: Paths,
    bus_address: String,
    dirs: TestDirs,
    // Last: the daemon leaves the bus before the bus goes.
    session: RealSession,
}

impl RealMedia {
    async fn start() -> RealMedia {
        let live_before = LiveAudio::read();
        let session = RealSession::start(&LiveEnv::from_vars(std::env::vars_os()));
        let dirs = TestDirs::new();
        let config = format!(
            "listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\n{}",
            nonexistent_programs()
        );
        let config = Config::from_toml(&config).expect("config");
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        // The address the way `main.rs` finds it, from the sealed values only.
        let bus_address = session_bus_address(
            Some(session_var(&session, "DBUS_SESSION_BUS_ADDRESS").as_ref()),
            Some(session.runtime_dir().as_path()),
        )
        .expect("the sealed bus address");
        let swaysock = PathBuf::from(session_var(&session, "SWAYSOCK"));
        let host = Host {
            media: MediaHost::Bus(Some(bus_address.clone())),
            ..Host::new(SwayTarget::Explicit(swaysock))
        };
        let daemon = Daemon::start_on(config, paths.clone(), host)
            .await
            .expect("daemon starts");
        RealMedia {
            live_before,
            daemon,
            paths,
            bus_address,
            dirs,
            session,
        }
    }

    /// Pairs a device with the owner's yes, connects it and subscribes it to `media`.
    async fn device(&self) -> (Session, Media) {
        let mut admin = soon("admin", AdminClient::connect(&self.paths))
            .await
            .expect("admin socket");
        let request = PairingRequest {
            scopes: MEDIA_SCOPES.into_iter().collect(),
            lifetime: PAIRING_WINDOW,
            host: None,
        };
        let uri = soon("pairing window", admin.open_pairing(&request))
            .await
            .expect("window");
        let uri = PairingUri::parse(&uri).expect("pairing URI");
        let mut pairing = Pairing { admin, uri };
        let pending = pairing.present_secret().await;
        pairing.answer(true).await;
        let profile = soon("confirmed", pending.confirmed())
            .await
            .expect("pairing confirmed");
        let mut session = soon("auth", support::connector().connect(&profile))
            .await
            .expect("authenticated");
        let states = soon("subscribe", session.subscribe(&[TOPIC_MEDIA])).await;
        let [first] = states.expect("subscribed").try_into().expect("one state");
        let media = Media::decode(&first.data).expect("media state");
        (session, media)
    }

    /// Starts `mpris-test-player` on the sealed bus as `org.mpris.MediaPlayer2.<name>`.
    fn start_player(&mut self, name: &str) -> TestPlayer {
        let log = self.dirs.root().join(format!("{name}.calls"));
        let mut command = self.session.command(mpris_test_player());
        command.arg(&self.bus_address).arg(name).arg(&log);
        let pid = self.session.spawn(command);
        TestPlayer {
            name: name.to_owned(),
            pid,
            log,
        }
    }

    /// Stops the daemon and the session, then checks that nothing they started is left,
    /// also outside the session's group, and that the live sound server is as before.
    async fn stop(self) {
        let RealMedia {
            live_before,
            daemon,
            session,
            ..
        } = self;
        daemon.stop().await;
        let group = session.process_group();
        let runtime_dir = session.runtime_dir();
        drop(session);
        assert_eq!(live_processes_in_group(group), Vec::<u32>::new());
        assert_eq!(leftovers_naming(&runtime_dir), Vec::<u32>::new());
        LiveAudio::assert_unchanged(&live_before);
    }
}

struct TestPlayer {
    name: String,
    pid: u32,
    log: PathBuf,
}

impl TestPlayer {
    /// The MPRIS methods the player got, oldest first.
    fn calls(&self) -> Vec<String> {
        match fs::read_to_string(&self.log) {
            Ok(text) => text.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read {}: {error}", self.log.display()),
        }
    }

    /// The player is gone as if it crashed: SIGTERM, no MPRIS `Quit`.
    fn kill(&self) {
        let pid = i32::try_from(self.pid).expect("pid fits i32");
        let pid = Pid::from_raw(pid).expect("pid is positive");
        kill_process(pid, Signal::TERM).expect("the player gets SIGTERM");
    }

    fn find<'a>(&self, media: &'a Media) -> Option<&'a Player> {
        media.players.iter().find(|player| player.name == self.name)
    }

    fn get<'a>(&self, media: &'a Media) -> &'a Player {
        self.find(media)
            .unwrap_or_else(|| panic!("{} is not in {media:?}", self.name))
    }
}

fn session_var(session: &RealSession, name: &str) -> std::ffi::OsString {
    session
        .env()
        .find(|(variable, _)| *variable == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_else(|| panic!("the sealed session sets {name}"))
}

/// The next `media` state that satisfies `wanted`.
async fn media_until(session: &mut Session, wanted: impl Fn(&Media) -> bool) -> Media {
    loop {
        match soon("media state", session.next_event()).await {
            Ok(Event::State(state)) if state.topic == TOPIC_MEDIA => {
                let media = Media::decode(&state.data).expect("media state");
                if wanted(&media) {
                    return media;
                }
            }
            other => panic!("not a media state: {other:?}"),
        }
    }
}

async fn command(session: &mut Session, player_id: u64, command: MediaCommand) {
    let args = Args::Media { player_id, command };
    soon("media command", session.request(&args))
        .await
        .unwrap_or_else(|error| panic!("{command:?} to player {player_id}: {error:?}"));
}

fn status_of(player: &TestPlayer, media: &Media) -> Option<PlayerStatus> {
    player.find(media).map(|found| found.status)
}

fn title_of<'a>(player: &TestPlayer, media: &'a Media) -> Option<&'a str> {
    player.find(media).and_then(|found| found.title.as_deref())
}

fn calls(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[tokio::test]
async fn a_real_player_shows_in_the_topic_takes_commands_and_leaves_when_it_quits() {
    let mut rig = RealMedia::start().await;
    let (mut session, first) = rig.device().await;
    assert_eq!(first.players, Vec::new(), "the sealed bus has no player");
    let alpha = rig.start_player("alpha");

    let media = media_until(&mut session, |media| alpha.find(media).is_some()).await;
    let player = alpha.get(&media).clone();
    let expected = Player {
        id: player.id,
        name: "alpha".to_owned(),
        app_id: Some(DESKTOP_ENTRY.to_owned()),
        icon: None,
        status: PlayerStatus::Paused,
        title: Some("Track 1".to_owned()),
        artist: Some(ARTIST.to_owned()),
        album: Some(ALBUM.to_owned()),
        can_play: true,
        can_pause: true,
        can_next: true,
        can_previous: true,
    };
    assert_eq!(player, expected);
    assert_eq!(media.active_player_id, None, "it never played");
    assert_eq!(alpha.calls(), Vec::<String>::new());

    command(&mut session, player.id, MediaCommand::PlayPause).await;
    assert_eq!(alpha.calls(), calls(&["PlayPause"]));
    let media = media_until(&mut session, |media| {
        status_of(&alpha, media) == Some(PlayerStatus::Playing)
    })
    .await;
    assert_eq!(media.active_player_id, Some(player.id));

    command(&mut session, player.id, MediaCommand::Next).await;
    media_until(&mut session, |media| {
        title_of(&alpha, media) == Some("Track 2")
    })
    .await;
    command(&mut session, player.id, MediaCommand::Previous).await;
    media_until(&mut session, |media| {
        title_of(&alpha, media) == Some("Track 1")
    })
    .await;
    assert_eq!(alpha.calls(), calls(&["PlayPause", "Next", "Previous"]));

    alpha.kill();
    let media = media_until(&mut session, |media| media.players.is_empty()).await;
    assert_eq!(media.active_player_id, None);
    let args = Args::Media {
        player_id: player.id,
        command: MediaCommand::PlayPause,
    };
    let error = refusal(soon("command to a gone player", session.request(&args)).await);
    assert_eq!(error.code, ErrorCode::NotFound);
    rig.stop().await;
}

#[tokio::test]
async fn of_two_real_players_the_active_one_is_the_one_that_started_playing_last() {
    let mut rig = RealMedia::start().await;
    let (mut session, _) = rig.device().await;
    let alpha = rig.start_player("alpha");
    let beta = rig.start_player("beta");
    let media = media_until(&mut session, |media| {
        alpha.find(media).is_some() && beta.find(media).is_some()
    })
    .await;
    let (alpha_id, beta_id) = (alpha.get(&media).id, beta.get(&media).id);
    assert_ne!(alpha_id, beta_id);
    assert_eq!(media.active_player_id, None);

    let playing = |player: &TestPlayer, media: &Media| {
        status_of(player, media) == Some(PlayerStatus::Playing)
    };
    command(&mut session, alpha_id, MediaCommand::PlayPause).await;
    let media = media_until(&mut session, |media| playing(&alpha, media)).await;
    assert_eq!(media.active_player_id, Some(alpha_id));

    command(&mut session, beta_id, MediaCommand::PlayPause).await;
    let media = media_until(&mut session, |media| playing(&beta, media)).await;
    assert_eq!(media.active_player_id, Some(beta_id), "beta started last");

    // Pausing alpha leaves beta active; alpha playing again takes the focus back.
    command(&mut session, alpha_id, MediaCommand::PlayPause).await;
    let media = media_until(&mut session, |media| !playing(&alpha, media)).await;
    assert_eq!(media.active_player_id, Some(beta_id));
    command(&mut session, alpha_id, MediaCommand::PlayPause).await;
    let media = media_until(&mut session, |media| playing(&alpha, media)).await;
    assert_eq!(media.active_player_id, Some(alpha_id));

    beta.kill();
    let media = media_until(&mut session, |media| beta.find(media).is_none()).await;
    assert_eq!(media.active_player_id, Some(alpha_id));
    assert_eq!(alpha.get(&media).id, alpha_id, "alpha keeps its id");
    assert_eq!(alpha.calls(), calls(&["PlayPause"; 3]));
    assert_eq!(beta.calls(), calls(&["PlayPause"]));
    rig.stop().await;
}
