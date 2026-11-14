//! Audio against the real sound server (`docs/M2C_PLAN.md` 4, slice R1): a read-write
//! daemon in this process runs the real `pactl` on the sealed PipeWire of a `RealSession`;
//! every other program is `fake-proc`. The test reads each result back with `pactl` on the
//! sealed server. Runs only with the `real-backends` feature: `just real`.

#![cfg(feature = "real-backends")]

#[path = "v2_m2a/rig.rs"]
#[allow(dead_code, reason = "only `Gate` of the M2a rig is used here")]
mod m2a_rig;
#[path = "v2_m2b/requests.rs"]
#[allow(dead_code, reason = "only the audio requests are used here")]
mod requests;
mod support;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use m2a_rig::Gate;
use requests::{ask, audio, detail, refused, topic_state};
use serde_json::Value;
use support::live::LiveAudio;
use support::{Pairing, soon};
use swayconnect_cli::client::{Event, Profile, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::proc::{ChildEnv, RealRunner};
use swayconnect_protocol::ops::TOPIC_AUDIO;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{Audio, AudioChange, Device, ErrorCode, JsonObject, Scope};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{
    FakeProcs, LiveEnv, ProgramSection, RealSession, SEALED_SINKS, SEALED_SOURCE, TestConfig,
    TestDirs, leftovers_naming, live_processes_in_group,
};

const NOTIFY: &str = "notify-send";
/// The real program, the one system path of the config. Every other section is fake.
const REAL_PACTL: &str = "[audio]\npactl = \"/usr/bin/pactl\"\n";
const FAKE_SECTIONS: [ProgramSection; 5] = [
    ProgramSection::Session,
    ProgramSection::KeepAwake,
    ProgramSection::Notify,
    ProgramSection::Apps,
    ProgramSection::Launcher,
];
const PAIRING_WINDOW: Duration = Duration::from_secs(300);
const AUDIO_SCOPES: [Scope; 2] = [Scope::StateRead, Scope::Audio];
const CAPTURE_SCOPES: [Scope; 3] = [Scope::StateRead, Scope::Audio, Scope::AudioCapture];

/// A sealed session and a daemon whose children get only the session's variables, checked
/// by the session's guard.
struct RealAudio {
    /// The live sound server before the test; the same after it.
    live_before: Option<LiveAudio>,
    daemon: DaemonHandle,
    paths: Paths,
    programs: FakeProcs,
    dirs: TestDirs,
    // Last: the daemon's `pactl subscribe` ends with the sound server.
    session: RealSession,
}

impl RealAudio {
    async fn start() -> RealAudio {
        let live_before = LiveAudio::read();
        let session = RealSession::start(&LiveEnv::from_vars(std::env::vars_os()));
        let dirs = TestDirs::new();
        let programs = FakeProcs::install(&dirs.root().join("bin"));
        let proc_root = dirs.root().join("proc");
        fs::create_dir(&proc_root).expect("fake /proc");
        let fake = TestConfig::with_sections(&programs, &FAKE_SECTIONS)
            .preamble("listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\nread_only = false")
            .build();
        let config = Config::from_toml(&format!("{fake}\n{REAL_PACTL}")).expect("config");
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let sealed = session.checked_env(
            session
                .env()
                .map(|(name, value)| (OsString::from(name), value.to_owned())),
        );
        let swaysock = session_var(&session, "SWAYSOCK");
        let host = Host {
            runner: Arc::new(RealRunner::new(ChildEnv::allowlisted(sealed))),
            proc_root,
            ..Host::new(SwayTarget::Explicit(PathBuf::from(swaysock)))
        };
        let daemon = Daemon::start_on(config, paths.clone(), host)
            .await
            .expect("daemon starts");
        RealAudio {
            live_before,
            daemon,
            paths,
            programs,
            dirs,
            session,
        }
    }

    /// Pairs a device with the owner's yes and connects it.
    async fn device(&self, scopes: &[Scope]) -> (Profile, Session) {
        let mut admin = soon("admin", AdminClient::connect(&self.paths))
            .await
            .expect("admin socket");
        let request = PairingRequest {
            scopes: scopes.iter().copied().collect(),
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
        let session = soon("auth", support::connector().connect(&profile))
            .await
            .expect("authenticated");
        (profile, session)
    }

    /// One device of `pactl -f json list sinks` or `sources` on the sealed server.
    fn pactl_node(&self, list: &str, name: &str) -> Value {
        let json = self.session.pactl(&["-f", "json", "list", list]);
        let nodes: Vec<Value> = serde_json::from_str(&json).expect("pactl JSON array");
        nodes
            .into_iter()
            .find(|node| node["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not in {json}"))
    }

    fn pactl_default(&self, direction: &str) -> String {
        let stdout = self.session.pactl(&[&format!("get-default-{direction}")]);
        stdout.trim_end().to_owned()
    }

    /// Stops the daemon and the session, then checks that nothing they started is left
    /// and that the live sound server is as before.
    async fn stop(self) {
        let group = self.session.process_group();
        let runtime_dir = self.session.runtime_dir();
        self.daemon.stop().await;
        drop(self.session);
        assert_eq!(live_processes_in_group(group), Vec::<u32>::new());
        assert_eq!(leftovers_naming(&runtime_dir), Vec::<u32>::new());
        drop(self.dirs);
        LiveAudio::assert_unchanged(&self.live_before);
    }
}

fn session_var(session: &RealSession, name: &str) -> OsString {
    session
        .env()
        .find(|(var, _)| *var == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_else(|| panic!("the sealed session sets {name}"))
}

/// The loudest channel of a pactl node in percent, rounded as the daemon rounds.
fn volume_pct(node: &Value) -> u64 {
    let channels = node["volume"].as_object().expect("volume channels");
    let loudest = channels
        .values()
        .filter_map(|channel| channel["value"].as_u64())
        .max()
        .expect("a channel");
    (loudest * 100 + 32_768) / 65_536
}

fn decode(data: &JsonObject) -> Audio {
    Audio::decode(data).expect("audio state")
}

/// The first audio state, once pactl was read.
async fn first_audio(session: &mut Session) -> Audio {
    decode(&topic_state(session, TOPIC_AUDIO, |data| decode(data).available).await)
}

/// The next `state.changed` of topic `audio` that satisfies `wanted`.
async fn next_audio(session: &mut Session, wanted: impl Fn(&Audio) -> bool) -> Audio {
    loop {
        match soon("audio state.changed", session.next_event()).await {
            Ok(Event::State(state)) if state.topic == TOPIC_AUDIO => {
                let audio = decode(&state.data);
                if wanted(&audio) {
                    return audio;
                }
            }
            Ok(Event::State(_)) => {}
            other => panic!("not a state event: {other:?}"),
        }
    }
}

/// The device of `devices` whose description pactl gives to `name`.
fn by_name<'a>(rig: &RealAudio, list: &str, devices: &'a [Device], name: &str) -> &'a Device {
    let description = rig.pactl_node(list, name)["description"].clone();
    let mut matching = devices
        .iter()
        .filter(|device| description == device.description.as_str());
    let device = matching
        .next()
        .unwrap_or_else(|| panic!("no device for {name}: {devices:?}"));
    assert!(matching.next().is_none(), "two devices like {name}");
    device
}

/// Keeps the pactl output of the sealed server as evidence and as the source of the
/// null-sink fixtures (`tests/fixtures/pactl/README.md`).
fn keep_capture(rig: &RealAudio) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-session");
    fs::create_dir_all(&dir).expect("create the capture dir");
    for list in ["sinks", "sources"] {
        let json = rig.session.pactl(&["-f", "json", "list", list]);
        fs::write(dir.join(format!("null-{list}.json")), json).expect("keep the capture");
    }
}

#[tokio::test]
async fn the_daemon_lists_and_writes_the_sealed_devices() {
    let rig = RealAudio::start().await;
    keep_capture(&rig);
    let (_, mut phone) = rig.device(&AUDIO_SCOPES).await;

    let state = first_audio(&mut phone).await;
    assert_eq!(state.sinks.len(), 2, "two null sinks: {state:?}");
    assert_eq!(
        state.sources.len(),
        1,
        "one source, monitors hidden: {state:?}"
    );
    let [sink_1, sink_2] = SEALED_SINKS.map(|name| by_name(&rig, "sinks", &state.sinks, name).id);
    let source = by_name(&rig, "sources", &state.sources, SEALED_SOURCE).id;
    let default_sink = rig.pactl_default("sink");
    let (default_id, other_id, other_name) = if default_sink == SEALED_SINKS[0] {
        (sink_1, sink_2, SEALED_SINKS[1])
    } else {
        (sink_2, sink_1, SEALED_SINKS[0])
    };
    assert_eq!(state.default_sink, Some(default_id), "{default_sink}");
    assert_eq!(state.default_source, Some(source));

    let volume = ask(&mut phone, &audio(other_id, AudioChange::Volume(37))).await;
    assert!(volume.is_ok(), "{volume:?}");
    assert_eq!(volume_pct(&rig.pactl_node("sinks", other_name)), 37);
    let muted = ask(&mut phone, &audio(other_id, AudioChange::Mute(true))).await;
    assert!(muted.is_ok(), "{muted:?}");
    assert_eq!(rig.pactl_node("sinks", other_name)["mute"], true);
    let default = ask(&mut phone, &audio(other_id, AudioChange::Default)).await;
    assert!(default.is_ok(), "{default:?}");
    assert_eq!(rig.pactl_default("sink"), other_name);

    let shown = next_audio(&mut phone, |state| state.default_sink == Some(other_id)).await;
    let other = shown.sinks.iter().find(|sink| sink.id == other_id);
    let other = other.expect("the sink keeps its id");
    assert_eq!((other.volume_pct, other.muted), (37, true));
    rig.stop().await;
}

#[tokio::test]
async fn a_change_made_outside_the_daemon_reaches_the_audio_topic() {
    let rig = RealAudio::start().await;
    let (_, mut phone) = rig.device(&AUDIO_SCOPES).await;
    let state = first_audio(&mut phone).await;
    let sink = by_name(&rig, "sinks", &state.sinks, SEALED_SINKS[1]).id;
    let source = by_name(&rig, "sources", &state.sources, SEALED_SOURCE).id;

    rig.session
        .pactl(&["set-sink-volume", SEALED_SINKS[1], "42%"]);
    let shown = next_audio(&mut phone, |state| {
        state
            .sinks
            .iter()
            .any(|device| device.id == sink && device.volume_pct == 42)
    })
    .await;
    assert_eq!(shown.sinks.len(), 2);

    rig.session.pactl(&["set-source-mute", SEALED_SOURCE, "1"]);
    next_audio(&mut phone, |state| {
        state
            .sources
            .iter()
            .any(|device| device.id == source && device.muted)
    })
    .await;

    rig.session.pactl(&["set-default-sink", SEALED_SINKS[1]]);
    next_audio(&mut phone, |state| state.default_sink == Some(sink)).await;
    rig.stop().await;
}

#[tokio::test]
async fn a_microphone_unmute_needs_audio_capture_and_shows_the_notice() {
    let rig = RealAudio::start().await;
    let notice = Gate::install(&rig.programs, NOTIFY);
    rig.session.pactl(&["set-source-mute", SEALED_SOURCE, "1"]);
    let (_, mut plain) = rig.device(&AUDIO_SCOPES).await;
    let state = first_audio(&mut plain).await;
    let source = by_name(&rig, "sources", &state.sources, SEALED_SOURCE).id;

    let unmute = audio(source, AudioChange::Mute(false));
    let error = refused(&mut plain, &unmute, ErrorCode::Forbidden).await;
    assert_eq!(detail(&error, "scope"), "audio.capture");
    assert_eq!(rig.pactl_node("sources", SEALED_SOURCE)["mute"], true);

    let (profile, mut capture) = rig.device(&CAPTURE_SCOPES).await;
    let short_id = &profile.device_id.as_str()[..4];
    let connected = format!("Device with remote control rights connected (device {short_id})");
    let shown = ["--app-name=swayconnect", "--", "swayconnect", &connected];
    assert_eq!(notice.release().await, shown, "tier 2 connect notice");
    let unmuted = ask(&mut capture, &unmute).await;
    assert!(unmuted.is_ok(), "{unmuted:?}");
    assert_eq!(rig.pactl_node("sources", SEALED_SOURCE)["mute"], false);
    let text = format!("Microphone unmuted remotely (device {short_id})");
    let shown = ["--app-name=swayconnect", "--", "swayconnect", &text];
    assert_eq!(notice.release().await, shown, "the remote unmute notice");
    drop(notice);
    rig.stop().await;
}
