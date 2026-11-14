//! Typing against the real compositor (`docs/M2C_PLAN.md` 4, slice R3): a read-write daemon
//! in this process types through its real `zwp_virtual_keyboard_v1` into the headless sway
//! of a `RealSession`. The targets are foot windows that run `cat` into a FIFO the test
//! reads, and the real fuzzel as the launcher. The guard reads the real `/proc`. Every
//! other program is `fake-proc`. Runs only with the `real-backends` feature: `just real`.

#![cfg(feature = "real-backends")]

#[path = "real_input/observe.rs"]
mod observe;
#[path = "v2_m2b/requests.rs"]
#[allow(dead_code, reason = "only `ask`, `refused` and `detail` are used here")]
mod requests;
mod support;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use observe::{Typed, shot_path, wait_for_launcher_grab};
use requests::{ask, detail, refused};
use rustix::process::getuid;
use serde_json::{Value, json};
use support::live::LiveAudio;
use support::{Pairing, soon};
use swayconnect_cli::client::{Event, Session};
use swayconnect_daemon::admin::{AdminClient, PairingRequest};
use swayconnect_daemon::config::Config;
use swayconnect_daemon::daemon::services::KeyboardHost;
use swayconnect_daemon::daemon::{Daemon, DaemonHandle, Host};
use swayconnect_daemon::input::WaylandTarget;
use swayconnect_daemon::lockstate::ProcScanner;
use swayconnect_daemon::paths::Paths;
use swayconnect_daemon::proc::{ChildEnv, RealRunner};
use swayconnect_protocol::ops::{Args, TOPIC_DESKTOP, TOPIC_SESSION};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{
    ErrorCode, InputTarget, InputText, JsonObject, Key, KeyPress, Modifier, Scope,
};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{
    FakeProcs, LiveEnv, ProgramSection, RealSession, TestConfig, TestDirs, leftovers_naming,
    live_processes_in_group,
};

const FOOT: &str = "/usr/bin/foot";
/// The real launcher, the one system path of the config. Every other section is fake.
const REAL_LAUNCHER: &str = "[launcher]\nargv = [\"/usr/bin/fuzzel\"]\n";
const FAKE_SECTIONS: [ProgramSection; 5] = [
    ProgramSection::Session,
    ProgramSection::KeepAwake,
    ProgramSection::Notify,
    ProgramSection::Audio,
    ProgramSection::Apps,
];
const INPUT_SCOPES: [Scope; 5] = [
    Scope::StateRead,
    Scope::StateTitles,
    Scope::Launch,
    Scope::InputText,
    Scope::InputKeys,
];
const PAIRING_WINDOW: Duration = Duration::from_secs(300);
/// fuzzel lists the decoy first; only typed text selects the probe.
const PROBE_APP_ID: &str = "swayconnect-r3-fuzzel-probe";
/// Desktop entry file, name and app id of foot's window.
const DESKTOP_ENTRIES: [(&str, &str, &str); 2] = [
    ("aaa-decoy", "Aaa Decoy", "swayconnect-r3-fuzzel-decoy"),
    ("zqx-probe", "Zqx Probe", PROBE_APP_ID),
];
const WARM_UP_APP_ID: &str = "swayconnect-r3-warm-up";
/// The XKB name of `§`, which the focus test binds in sway.
const BOUND_KEYSYM: &str = "section";

/// A sealed session and a daemon whose children get only the session's variables, with
/// `XDG_DATA_DIRS` naming the test's desktop entries, checked by the session's guard.
struct RealInput {
    /// The live sound server before the test; the same after it.
    live_before: Option<LiveAudio>,
    daemon: DaemonHandle,
    paths: Paths,
    _programs: FakeProcs,
    dirs: TestDirs,
    session: RealSession,
}

impl RealInput {
    async fn start() -> RealInput {
        let live_before = LiveAudio::read();
        let session = RealSession::start(&LiveEnv::from_vars(std::env::vars_os()));
        let dirs = TestDirs::new();
        let programs = FakeProcs::install(&dirs.root().join("bin"));
        let fake = TestConfig::with_sections(&programs, &FAKE_SECTIONS)
            .preamble("listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\nread_only = false")
            .build();
        let config = Config::from_toml(&format!("{fake}\n{REAL_LAUNCHER}")).expect("config");
        refuse_a_locked_or_grabbed_desktop(&config);
        let paths = Paths {
            config_file: dirs.config_file(),
            data_dir: dirs.data_dir(),
            runtime_dir: dirs.runtime_dir(),
        };
        let share = write_desktop_entries(dirs.root());
        let sealed = session.checked_env(session.env().map(|(name, value)| match name {
            "XDG_DATA_DIRS" => (name.into(), share.clone().into_os_string()),
            _ => (name.into(), value.to_owned()),
        }));
        let keyboard = KeyboardHost::Wayland(WaylandTarget::Discover {
            runtime_dir: Some(session.runtime_dir()),
            display: Some(session_var(&session, "WAYLAND_DISPLAY")),
        });
        let swaysock = PathBuf::from(session_var(&session, "SWAYSOCK"));
        let host = Host {
            runner: Arc::new(RealRunner::new(ChildEnv::allowlisted(sealed))),
            proc_root: PathBuf::from("/proc"),
            keyboard,
            ..Host::new(SwayTarget::Explicit(swaysock))
        };
        let daemon = Daemon::start_on(config, paths.clone(), host)
            .await
            .expect("daemon starts");
        RealInput {
            live_before,
            daemon,
            paths,
            _programs: programs,
            dirs,
            session,
        }
    }

    /// Pairs a device with the owner's yes and connects it twice: one connection sends the
    /// requests, the other watches the topics.
    async fn device(&self) -> (Session, Watcher) {
        let mut admin = soon("admin", AdminClient::connect(&self.paths))
            .await
            .expect("admin socket");
        let request = PairingRequest {
            scopes: INPUT_SCOPES.iter().copied().collect(),
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
        let connector = support::connector();
        let mut sessions = Vec::new();
        for _ in 0..2 {
            let session = soon("auth", connector.connect(&profile)).await;
            sessions.push(session.expect("authenticated"));
        }
        let watcher = Watcher::start(sessions.pop().expect("two")).await;
        (sessions.pop().expect("two"), watcher)
    }

    /// A foot window `app_id` whose `cat` writes every typed line into a new FIFO.
    fn foot(&mut self, app_id: &str) -> Typed {
        let fifo = self.dirs.root().join(format!("{app_id}.fifo"));
        let typed = Typed::open(&fifo);
        let mut foot = self.session.command(FOOT);
        foot.args(["--app-id", app_id, "--title", app_id, "sh", "-c"])
            .arg(format!("exec cat > '{}'", fifo.display()));
        self.session.spawn(foot);
        self.session.wait_for_window(app_id);
        typed
    }

    /// The sealed sway starts with no keyboard, and clients bind `wl_keyboard` only after the
    /// seat offers one, so keys sent just after the daemon's keyboard appears reach no
    /// client (they are still counted as delivered). The owner's desktop always has a
    /// keyboard; this gives the sealed seat one first: one key into a throwaway window
    /// creates the daemon's virtual keyboard, then the window closes.
    async fn arm_keyboard(&mut self, phone: &mut Session, watcher: &mut Watcher) {
        let mut foot = self.session.command(FOOT);
        foot.args(["--app-id", WARM_UP_APP_ID, "/bin/cat"]);
        self.session.spawn(foot);
        self.session.wait_for_window(WARM_UP_APP_ID);
        let window_id = watcher.window_id(WARM_UP_APP_ID).await;
        pressed(phone, &key(InputTarget::Window(window_id), "space", &[])).await;
        self.swaymsg(&format!("[con_id={window_id}] kill"));
        let gone = |data: &JsonObject| window_of(data, WARM_UP_APP_ID).is_none();
        watcher.wait(TOPIC_DESKTOP, gone).await;
    }

    fn swaymsg(&self, command: &str) {
        let reply = self.session.swaymsg(&[command]);
        let results: Vec<Value> = serde_json::from_str(&reply).expect("swaymsg JSON");
        let done = results.iter().all(|result| result["success"] == true);
        assert!(done && !results.is_empty(), "{command}: {reply}");
    }

    /// Stops the daemon and the session, then checks that nothing they started is left:
    /// fuzzel's children are in no group of the session and leave with the compositor.
    async fn stop(self) {
        let group = self.session.process_group();
        let runtime_dir = self.session.runtime_dir();
        self.daemon.stop().await;
        drop(self.session);
        assert_eq!(live_processes_in_group(group), Vec::<u32>::new());
        let left = leftovers_naming(&runtime_dir);
        assert_eq!(left, Vec::<u32>::new(), "processes outlived the session");
        drop(self.dirs);
        LiveAudio::assert_unchanged(&self.live_before);
    }
}

/// The guard reads the real `/proc` of the own uid: a locked live screen or a live keyboard
/// grabber refuses every part. Says so instead of failing later with `locked` or
/// `keyboard_grabbed` (`docs/M2C_PLAN.md` 5 risk 1).
fn refuse_a_locked_or_grabbed_desktop(config: &Config) {
    let scanner = ProcScanner::new(PathBuf::from("/proc"), getuid().as_raw(), &config.session);
    let facts = scanner.scan().expect("read /proc");
    assert!(
        !facts.locked && facts.grabbers.is_empty(),
        "real_input needs an unlocked desktop with no keyboard grabber running; \
         the live desktop shows locked={} grabbers={:?}: unlock or close them and rerun",
        facts.locked,
        facts.grabbers
    );
}

/// Two desktop entries for fuzzel. Each runs foot with an app id that a sway rule of the
/// launcher test sends to workspace 2, so the visible output changes only with fuzzel.
fn write_desktop_entries(root: &Path) -> PathBuf {
    // Not `TestDirs`' own `share`: fuzzel lists exactly these two entries.
    let share = root.join("r3-share");
    let applications = share.join("applications");
    fs::create_dir_all(&applications).expect("applications dir");
    for (id, name, app_id) in DESKTOP_ENTRIES {
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec={FOOT} --app-id={app_id} /bin/cat\n"
        );
        fs::write(applications.join(format!("{id}.desktop")), entry).expect("desktop entry");
    }
    share
}

fn session_var(session: &RealSession, name: &str) -> OsString {
    session
        .env()
        .find(|(var, _)| *var == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_else(|| panic!("the sealed session sets {name}"))
}

fn text(target: InputTarget, text: &str) -> Args {
    let text = InputText::parse(text).expect("input text");
    Args::InputText { target, text }
}

fn key(target: InputTarget, name: &str, modifiers: &[Modifier]) -> Args {
    let key = Key::from_wire(name).expect("key name");
    let press = KeyPress::new(key, modifiers.to_vec(), 1).expect("key press");
    Args::InputKey { target, press }
}

async fn typed_chars(phone: &mut Session, args: &Args) -> u64 {
    let result = ask(phone, args).await.expect("typed");
    result["delivered_chars"].as_u64().expect("delivered_chars")
}

async fn pressed(phone: &mut Session, args: &Args) {
    ask(phone, args).await.expect("pressed");
}

/// A connection that only watches topics `desktop` and `session`: it reads every state in
/// order and keeps the newest, so no wait sees a stale state.
struct Watcher {
    session: Session,
    latest: BTreeMap<String, JsonObject>,
}

impl Watcher {
    async fn start(mut session: Session) -> Watcher {
        let states = soon(
            "subscribe",
            session.subscribe(&[TOPIC_DESKTOP, TOPIC_SESSION]),
        )
        .await
        .expect("subscribed");
        let latest = states
            .into_iter()
            .map(|state| (state.topic, state.data))
            .collect();
        Watcher { session, latest }
    }

    /// The newest state of `topic` once it satisfies `wanted`.
    async fn wait(&mut self, topic: &str, wanted: impl Fn(&JsonObject) -> bool) -> JsonObject {
        loop {
            if let Some(data) = self.latest.get(topic).filter(|data| wanted(data)) {
                return data.clone();
            }
            match soon(topic, self.session.next_event()).await {
                Ok(Event::State(state)) => {
                    self.latest.insert(state.topic, state.data);
                }
                other => panic!("not a state event: {other:?}"),
            }
        }
    }

    /// The id of window `app_id` once the daemon publishes it.
    async fn window_id(&mut self, app_id: &str) -> u64 {
        let desktop = self
            .wait(TOPIC_DESKTOP, |data| window_of(data, app_id).is_some())
            .await;
        window_of(&desktop, app_id).expect("published")
    }

    /// Waits until topic `session` showed the launcher running and then closed.
    async fn launcher_ran_and_closed(&mut self) {
        for open in [true, false] {
            self.wait(TOPIC_SESSION, |data| data["launcher"]["open"] == open)
                .await;
        }
    }
}

fn window_of(desktop: &JsonObject, app_id: &str) -> Option<u64> {
    let windows = desktop.get("windows")?.as_array()?;
    let window = windows.iter().find(|window| window["app_id"] == app_id)?;
    window["id"].as_u64()
}

async fn open_launcher(phone: &mut Session) {
    let opened = ask(phone, &Args::LauncherOpen)
        .await
        .expect("launcher.open");
    assert_eq!(opened.get("already_open"), Some(&Value::Bool(false)));
}

/// 200 characters over 13 parts: ASCII, Latin-1, Greek, Cyrillic, CJK, an emoji beyond the
/// BMP and the joiners of 17.13, so parts need many different keymaps.
fn long_text() -> String {
    let alphabet: Vec<char> =
        "The quick brown fox, 0-9 & ~!@#$%^*()_+{}|:\"<>? ÄÖÜß éñ ΩЖ 語🙂\u{200D}"
            .chars()
            .collect();
    (0..200)
        .map(|i| alphabet[(i * 7) % alphabet.len()])
        .collect()
}

#[tokio::test]
async fn text_and_keys_reach_foot_as_typed() {
    let mut rig = RealInput::start().await;
    let (mut phone, mut watcher) = rig.device().await;
    rig.arm_keyboard(&mut phone, &mut watcher).await;
    let mut typed = rig.foot("swayconnect-r3-text");
    let target = InputTarget::Window(watcher.window_id("swayconnect-r3-text").await);

    let words = "Grüße aus Zürich: naïve café, ½ € — Ωμέγα, привет, 日本語 🙂";
    let count = |text: &str| text.chars().count() as u64;
    assert_eq!(
        typed_chars(&mut phone, &text(target, words)).await,
        count(words)
    );
    let long = long_text();
    assert_eq!(count(&long), 200);
    assert_eq!(typed_chars(&mut phone, &text(target, &long)).await, 200);
    pressed(&mut phone, &key(target, "Return", &[])).await;
    let mut expected = format!("{words}{long}\n");
    typed.wait_for(&expected);

    pressed(&mut phone, &key(target, "Escape", &[])).await;
    pressed(&mut phone, &key(target, "Return", &[])).await;
    expected.push_str("\x1b\n");
    typed.wait_for(&expected);

    // A second layout in sway must not change what the generated keymap types: `y` and `z`
    // swap places in `de`. The key presses reuse the keymap the keyboard already holds.
    rig.swaymsg("input type:keyboard xkb_layout de");
    pressed(&mut phone, &key(target, "y", &[])).await;
    pressed(&mut phone, &key(target, "z", &[])).await;
    assert_eq!(typed_chars(&mut phone, &text(target, " yz äöü")).await, 7);
    pressed(&mut phone, &key(target, "Return", &[])).await;
    expected.push_str("yz yz äöü\n");
    typed.wait_for(&expected);
    rig.session.screenshot(&shot_path("text"));

    pressed(&mut phone, &key(target, "c", &[Modifier::Ctrl])).await;
    assert_eq!(typed.end(), expected, "Ctrl+C ends cat with nothing more");
    rig.stop().await;
}

#[tokio::test]
async fn a_focus_change_mid_text_stops_the_rest() {
    let mut rig = RealInput::start().await;
    let (mut phone, mut watcher) = rig.device().await;
    rig.arm_keyboard(&mut phone, &mut watcher).await;
    let mut typed = rig.foot("swayconnect-r3-target");
    let other = rig.foot("swayconnect-r3-other");
    let target_id = watcher.window_id("swayconnect-r3-target").await;
    let other_id = watcher.window_id("swayconnect-r3-other").await;
    let target = InputTarget::Window(target_id);
    rig.swaymsg(&format!("[con_id={target_id}] focus"));
    // Sway itself moves focus while the first part is delivered: its last character runs
    // this binding, so the check before the second part sees the other window.
    rig.swaymsg(&format!("bindsym {BOUND_KEYSYM} [con_id={other_id}] focus"));

    let first = "abcdefghijklmno";
    let input = format!("{first}§ the rest must not be typed");
    let error = refused(&mut phone, &text(target, &input), ErrorCode::FocusChanged).await;
    assert_eq!(detail(&error, "delivered_chars"), json!(16));
    assert_eq!(detail(&error, "focused_window_id"), json!(other_id));

    rig.swaymsg(&format!("[con_id={target_id}] focus"));
    pressed(&mut phone, &key(target, "Return", &[])).await;
    typed.wait_for(&format!("{first}\n"));
    pressed(&mut phone, &key(target, "c", &[Modifier::Ctrl])).await;
    assert_eq!(typed.end(), format!("{first}\n"), "the rest was typed");

    rig.swaymsg(&format!("[con_id={other_id}] focus"));
    let other_target = InputTarget::Window(other_id);
    pressed(&mut phone, &key(other_target, "c", &[Modifier::Ctrl])).await;
    assert_eq!(other.end(), "", "the other window got keys");
    rig.stop().await;
}

/// The first text goes out right after `launcher.open` answered, before fuzzel has mapped
/// its layer surface: the guard waits until fuzzel holds the keyboard, so nothing reaches
/// the foot window under it.
#[tokio::test]
async fn typing_reaches_the_real_fuzzel_and_escape_closes_it() {
    let mut rig = RealInput::start().await;
    let (mut phone, mut watcher) = rig.device().await;
    rig.arm_keyboard(&mut phone, &mut watcher).await;
    let foot = rig.foot("swayconnect-r3-under");
    let foot_id = watcher.window_id("swayconnect-r3-under").await;
    rig.swaymsg(
        "for_window [app_id=\"^swayconnect-r3-fuzzel-\"] move container to workspace number 2",
    );
    let launcher = InputTarget::Launcher;

    open_launcher(&mut phone).await;
    assert_eq!(typed_chars(&mut phone, &text(launcher, "zqx")).await, 3);
    rig.session.screenshot(&shot_path("fuzzel-typed"));
    pressed(&mut phone, &key(launcher, "Return", &[])).await;
    rig.session.wait_for_window(PROBE_APP_ID);
    watcher.launcher_ran_and_closed().await;
    let error = refused(
        &mut phone,
        &key(launcher, "Return", &[]),
        ErrorCode::LauncherClosed,
    )
    .await;
    assert_eq!(detail(&error, "delivered_chars"), json!(0));

    rig.swaymsg(&format!("[con_id={foot_id}] focus"));
    open_launcher(&mut phone).await;
    wait_for_launcher_grab(&rig.session);
    rig.session.screenshot(&shot_path("fuzzel-again"));
    // The real fuzzel is a keyboard grabber for every target but its own launcher.
    let under = InputTarget::Window(foot_id);
    let error = refused(&mut phone, &text(under, "x"), ErrorCode::KeyboardGrabbed).await;
    assert_eq!(detail(&error, "delivered_chars"), json!(0));
    pressed(&mut phone, &key(launcher, "Escape", &[])).await;
    watcher.launcher_ran_and_closed().await;
    let error = refused(&mut phone, &text(launcher, "x"), ErrorCode::LauncherClosed).await;
    assert_eq!(detail(&error, "delivered_chars"), json!(0));

    pressed(&mut phone, &key(under, "c", &[Modifier::Ctrl])).await;
    assert_eq!(
        foot.end(),
        "",
        "keys meant for fuzzel reached the window under it"
    );
    rig.stop().await;
}
