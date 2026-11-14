use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use swayconnect_protocol::{Key, Modifier};
use swayconnect_sway::SwayTarget;
use swayconnect_testkit::{FakeSway, TestDirs};

use super::fake::{FakeKeyboard, Recorded};
use super::*;
use crate::apps::{Launcher, OpenLauncher};
use crate::lockstate::{ProcScanner, SessionConfig, add_fake_process};
use crate::notify::NotifyConfig;
use crate::proc::fake::FakeRunner;
use crate::proc::{Exit, ExitProbe, ProgramArgv};

pub(super) const UID: u32 = 1000;
pub(super) const WORKSPACE: u64 = 3;
pub(super) const LAUNCHER_PID: u32 = 4242;
const OTHER_GRABBER_PID: u32 = 4300;
const LOCKER_PID: u32 = 4400;
const NOTIFY_SEND: &str = "/fake/notify-send";

pub(super) fn this_device() -> DeviceId {
    DeviceId::from_bytes([1; 16])
}

pub(super) fn other_device() -> DeviceId {
    DeviceId::from_bytes([2; 16])
}

/// Windows 24, 29, 41 and 42 on workspace 3; window 42 is focused.
fn tree() -> String {
    let window = |id: u64| {
        json!({
            "id": id, "type": "con", "name": "invented", "app_id": "app", "pid": 1000 + id,
            "focused": id == 42, "nodes": [], "floating_nodes": []
        })
    };
    json!({
        "id": 1, "type": "root", "name": "root", "nodes": [{
            "id": 2, "type": "output", "name": "TEST-1", "nodes": [{
                "id": WORKSPACE, "type": "workspace", "name": "1", "num": 1, "focused": false,
                "nodes": [window(24), window(29), window(41), window(42)], "floating_nodes": []
            }], "floating_nodes": []
        }], "floating_nodes": []
    })
    .to_string()
}

/// What the grab check sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Grab {
    None,
    /// The launcher child (pid [`LAUNCHER_PID`]) is the only grabber.
    Launcher,
    Other,
}

/// The desktop the checks see: fake `/proc`, fake sway and the launcher view. Cheap to
/// clone into a [`FakeKeyboard`] hook.
#[derive(Clone)]
pub(super) struct Scene {
    pub(super) sway: Arc<FakeSway>,
    proc_root: PathBuf,
    launcher: Arc<watch::Sender<Option<OpenLauncher>>>,
    /// The exit of the launcher child that [`Scene::set_launcher`] shows.
    launcher_exit: Arc<watch::Sender<Option<Exit>>>,
}

impl Scene {
    pub(super) fn set_process(&self, pid: u32, name: &str, running: bool) {
        let dir = self.proc_root.join(pid.to_string());
        if running {
            add_fake_process(&self.proc_root, pid, name, UID);
        } else if dir.exists() {
            fs::remove_dir_all(dir).expect("remove fake process");
        }
    }

    /// `None`: the scan fails, so the lock state is unknown.
    pub(super) fn set_locked(&self, locked: Option<bool>) {
        match locked {
            None => {
                if self.proc_root.exists() {
                    fs::remove_dir_all(&self.proc_root).expect("remove fake proc");
                }
            }
            Some(locked) => {
                fs::create_dir_all(&self.proc_root).expect("fake proc");
                self.set_process(LOCKER_PID, "swaylock", locked);
            }
        }
    }

    pub(super) fn set_grab(&self, grab: Grab) {
        self.set_process(LAUNCHER_PID, "fuzzel", grab == Grab::Launcher);
        self.set_process(OTHER_GRABBER_PID, "wofi", grab == Grab::Other);
    }

    /// Moves focus without an event; `None` focuses the workspace.
    pub(super) fn focus(&self, window: Option<u64>) {
        self.sway.focus_without_event(window.unwrap_or(WORKSPACE));
    }

    pub(super) fn set_launcher(&self, open: Option<(DeviceId, Option<u64>)>) {
        self.launcher_exit.send_replace(None);
        let exit = ExitProbe::new(self.launcher_exit.subscribe());
        self.launcher
            .send_replace(open.map(|(holder, focus_window_id)| OpenLauncher {
                holder,
                focus_window_id,
                pid: LAUNCHER_PID,
                exit,
            }));
    }

    /// The launcher child ends; its view stays until the watch task would clear it.
    pub(super) fn end_launcher_child(&self) {
        self.launcher_exit.send_replace(Some(Exit::Code(0)));
    }
}

/// A service on fakes: [`FakeKeyboard`], the [`Scene`] and a fake notify-send.
pub(super) struct Rig {
    _dirs: TestDirs,
    pub(super) scene: Scene,
    pub(super) keyboard: FakeKeyboard,
    pub(super) runner: FakeRunner,
    pub(super) service: InputService,
}

impl Rig {
    pub(super) fn new() -> Rig {
        let (launcher, view) = watch::channel(None);
        Rig::with_launcher(launcher, view)
    }

    fn with_launcher(
        launcher: watch::Sender<Option<OpenLauncher>>,
        view: watch::Receiver<Option<OpenLauncher>>,
    ) -> Rig {
        let dirs = TestDirs::new();
        let sway = Arc::new(FakeSway::start(&dirs.runtime_dir(), &[]));
        sway.set_reply(swayconnect_testkit::Request::GetTree, tree());
        let proc_root = dirs.root().join("proc");
        fs::create_dir(&proc_root).expect("fake proc");
        let scanner = ProcScanner::new(proc_root.clone(), UID, &SessionConfig::default());
        let focus = SwayFocus::new(SwayTarget::Explicit(sway.socket_path().to_owned()));
        let keyboard = FakeKeyboard::default();
        let runner = FakeRunner::default();
        let config: NotifyConfig =
            toml::from_str(&format!("notify_send = \"{NOTIFY_SEND}\"")).expect("notify config");
        let service = InputService::new(
            Arc::new(keyboard.clone()),
            Guard::new(scanner, focus, view),
            Notifier::new(Arc::new(runner.clone()), &config),
        );
        Rig {
            _dirs: dirs,
            scene: Scene {
                sway,
                proc_root,
                launcher: Arc::new(launcher),
                launcher_exit: Arc::new(watch::channel(None).0),
            },
            keyboard,
            runner,
            service,
        }
    }

    pub(super) fn notices(&self) -> usize {
        self.runner
            .argvs()
            .iter()
            .filter(|argv| argv[0] == NOTIFY_SEND)
            .count()
    }
}

fn text(text: &str) -> InputText {
    InputText::parse(text).expect("valid text")
}

fn press(key: &str, modifiers: Vec<Modifier>, repeat: u8) -> KeyPress {
    KeyPress::new(Key::from_wire(key).expect("key"), modifiers, repeat).expect("press")
}

const WINDOW: InputTarget = InputTarget::Window(42);

#[tokio::test]
async fn text_goes_in_parts_of_sixteen_and_keys_are_released_after() {
    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    let result = input
        .type_text(WINDOW, &text("git status --short"))
        .await
        .expect("typed");
    assert_eq!(result.delivered_chars, 18);
    assert_eq!(rig.keyboard.typed_parts(), ["git status --sho", "rt"]);
    assert!(matches!(
        rig.keyboard.log().as_slice(),
        [
            Recorded::Delivery(_),
            Recorded::Delivery(_),
            Recorded::ReleaseAll
        ]
    ));
}

#[tokio::test]
async fn focus_moved_without_an_event_stops_the_second_part() {
    let rig = Rig::new();
    let sway = Arc::clone(&rig.scene.sway);
    rig.keyboard
        .after_delivery(move |_| sway.focus_without_event(24));
    let input = rig.service.connection(this_device());
    let error = input
        .type_text(WINDOW, &text("bbbbbbbbbbbbbbbbbbbb"))
        .await
        .expect_err("focus moved");
    assert_eq!(
        error,
        InputError::Refused(InputRefusal::FocusChanged {
            delivered_chars: 16,
            focused_window_id: Some(24),
        })
    );
    assert_eq!(rig.keyboard.typed_parts(), ["bbbbbbbbbbbbbbbb"]);
    assert_eq!(rig.keyboard.log().last(), Some(&Recorded::ReleaseAll));
}

#[tokio::test]
async fn checks_refuse_in_order_locked_launcher_grab_focus() {
    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    let refusal = |delivered_chars| InputRefusal::Locked { delivered_chars };
    rig.scene.set_locked(Some(true));
    rig.scene.set_grab(Grab::Other);
    rig.scene.focus(Some(41));
    let launcher = InputTarget::Launcher;
    let a = text("a");
    assert_eq!(
        input.type_text(launcher, &a).await,
        Err(InputError::Refused(refusal(0)))
    );
    rig.scene.set_locked(None);
    assert_eq!(
        input.type_text(launcher, &a).await,
        Err(InputError::Refused(refusal(0)))
    );
    rig.scene.set_locked(Some(false));
    rig.scene.set_grab(Grab::Other);
    assert_eq!(
        input.type_text(launcher, &a).await,
        Err(InputError::Refused(InputRefusal::LauncherClosed {
            delivered_chars: 0
        }))
    );
    rig.scene.set_launcher(Some((this_device(), Some(42))));
    assert_eq!(
        input.type_text(launcher, &a).await,
        Err(InputError::Refused(InputRefusal::KeyboardGrabbed {
            delivered_chars: 0
        }))
    );
    rig.scene.set_grab(Grab::Launcher);
    assert_eq!(
        input.type_text(launcher, &a).await,
        Err(InputError::Refused(InputRefusal::FocusChanged {
            delivered_chars: 0,
            focused_window_id: Some(41),
        }))
    );
    rig.scene.focus(Some(42));
    assert_eq!(
        input.type_text(launcher, &a).await,
        Ok(InputTextResult { delivered_chars: 1 })
    );
    assert_eq!(
        input.type_text(WINDOW, &a).await,
        Err(InputError::Refused(InputRefusal::KeyboardGrabbed {
            delivered_chars: 0
        })),
        "the launcher grabs the keyboard for a window target"
    );
    assert_eq!(rig.keyboard.typed_parts(), ["a"]);
}

#[tokio::test]
async fn an_ended_launcher_child_is_closed_before_its_view_clears() {
    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    let a = text("a");
    rig.scene.set_launcher(Some((this_device(), Some(42))));
    rig.scene.set_grab(Grab::Launcher);
    rig.scene.focus(Some(42));
    assert_eq!(
        input.type_text(InputTarget::Launcher, &a).await,
        Ok(InputTextResult { delivered_chars: 1 })
    );
    rig.scene.end_launcher_child();
    assert_eq!(
        input.type_text(InputTarget::Launcher, &a).await,
        Err(InputError::Refused(InputRefusal::LauncherClosed {
            delivered_chars: 0
        })),
        "focus is still on the window recorded at open, so only the exit tells"
    );
    assert_eq!(rig.keyboard.typed_parts(), ["a"]);
}

#[tokio::test]
async fn the_launcher_target_is_only_for_its_holder_while_the_child_lives() {
    let launcher_runner = FakeRunner::default();
    let argv = ProgramArgv::try_from(vec!["/fake/fuzzel".to_owned()]).expect("argv");
    let launcher = Launcher::new(argv, Arc::new(launcher_runner.clone()));
    let rig = Rig::with_launcher(watch::channel(None).0, launcher.view());
    launcher.open(&this_device(), Some(42)).expect("opened");
    let pid = launcher.view().borrow().as_ref().expect("open").pid;
    rig.scene.set_process(pid, "fuzzel", true);

    let other = rig.service.connection(other_device());
    let closed: Result<(), InputError> = Err(InputError::Refused(InputRefusal::LauncherClosed {
        delivered_chars: 0,
    }));
    assert_eq!(
        other
            .press_key(InputTarget::Launcher, &press("Return", Vec::new(), 1))
            .await,
        closed
    );
    let mine = rig.service.connection(this_device());
    let escape = press("Escape", Vec::new(), 1);
    mine.press_key(InputTarget::Launcher, &escape)
        .await
        .expect("holder types into its launcher");

    launcher_runner.end_child(0, Exit::Code(0));
    let mut view = launcher.view();
    view.wait_for(Option::is_none).await.expect("view");
    rig.scene.set_process(pid, "fuzzel", false);
    assert_eq!(mine.press_key(InputTarget::Launcher, &escape).await, closed);
}

#[tokio::test]
async fn a_key_press_sets_modifiers_around_its_repeats() {
    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    input
        .press_key(WINDOW, &press("t", vec![Modifier::Ctrl], 2))
        .await
        .expect("pressed");
    let deliveries = rig.keyboard.deliveries();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].events.first(),
        Some(&KeyEvent::Modifiers(0b100))
    );
    assert_eq!(deliveries[0].events.last(), Some(&KeyEvent::Modifiers(0)));
    assert_eq!(rig.keyboard.presses(), 2);
    assert_eq!(rig.keyboard.log().last(), Some(&Recorded::ReleaseAll));
}

#[tokio::test]
async fn keys_are_released_on_disconnect_and_on_a_dropped_request() {
    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    drop(input);
    assert_eq!(rig.keyboard.log(), [Recorded::ReleaseAll]);

    let rig = Rig::new();
    let sway = Arc::clone(&rig.scene.sway);
    // The second focus read never gets an answer, so the request hangs there.
    rig.keyboard.after_delivery(move |_| sway.stop_answering());
    let input = rig.service.connection(this_device());
    let long_text = text("cccccccccccccccccccc");
    let typing = input.type_text(WINDOW, &long_text);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), typing)
            .await
            .is_err()
    );
    assert!(matches!(
        rig.keyboard.log().as_slice(),
        [Recorded::Delivery(_), Recorded::ReleaseAll]
    ));
}

#[tokio::test]
async fn a_keyboard_failure_is_backend_unavailable_and_still_releases() {
    let rig = Rig::new();
    rig.keyboard.fail_next(SinkError::NoDisplay);
    let input = rig.service.connection(this_device());
    let error = input
        .type_text(WINDOW, &text("x"))
        .await
        .expect_err("fails");
    assert_eq!(error, InputError::BackendUnavailable);
    assert_eq!(error.error_body().code, ErrorCode::BackendUnavailable);
    assert_eq!(rig.keyboard.log(), [Recorded::ReleaseAll]);
}

#[tokio::test]
async fn an_unreachable_sway_refuses_input() {
    let rig = Rig::new();
    // The fake sway is gone: no answer means no input.
    let Rig {
        scene,
        keyboard,
        service,
        ..
    } = rig;
    drop(scene);
    let input = service.connection(this_device());
    let error = input
        .type_text(WINDOW, &text("x"))
        .await
        .expect_err("no sway");
    assert_eq!(error, InputError::SwayUnavailable);
    assert!(keyboard.deliveries().is_empty());
}

#[tokio::test]
async fn the_first_delivered_input_arms_a_session_with_one_notice() {
    let rig = Rig::new();
    let view = rig.service.view();
    let input = rig.service.connection(this_device());
    rig.scene.set_locked(Some(true));
    let _ = input.type_text(WINDOW, &text("a")).await;
    assert_eq!(*view.borrow(), None, "a refused request arms nothing");

    rig.scene.set_locked(Some(false));
    input.type_text(WINDOW, &text("a")).await.expect("typed");
    input
        .press_key(WINDOW, &press("Return", Vec::new(), 1))
        .await
        .expect("pressed");
    assert_eq!(*view.borrow(), Some(this_device()));
    tokio::task::yield_now().await;
    assert_eq!(rig.notices(), 1);
    assert_eq!(
        rig.runner.argvs()[0].last().map(String::as_str),
        Some("Remote keyboard active (device AQEB)")
    );

    drop(input);
    assert_eq!(*view.borrow(), None, "disconnect ends the session");
}

#[tokio::test]
async fn the_panic_switch_stops_input_and_ends_sessions() {
    let rig = Rig::new();
    let service = rig.service.clone();
    rig.keyboard.after_delivery(move |_| service.abort());
    let input = rig.service.connection(this_device());
    let error = input
        .type_text(WINDOW, &text("dddddddddddddddddddd"))
        .await
        .expect_err("aborted");
    assert_eq!(error, InputError::Aborted);
    assert_eq!(rig.keyboard.typed_parts(), ["dddddddddddddddd"]);
    assert_eq!(*rig.service.view().borrow(), None);
}

#[tokio::test(start_paused = true)]
async fn a_session_ends_after_ten_idle_minutes() {
    let sessions = Sessions::default();
    let view = sessions.view();
    let (first, second) = (sessions.next_id(), sessions.next_id());
    assert!(sessions.touch(first, &this_device()));
    sessions.spawn_expiry(first);
    tokio::time::advance(Duration::from_secs(60)).await;
    assert!(sessions.touch(second, &other_device()));
    sessions.spawn_expiry(second);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(
        !sessions.touch(first, &this_device()),
        "input refreshes the session"
    );
    assert_eq!(
        *view.borrow(),
        Some(this_device()),
        "the newest input holds it"
    );

    tokio::time::sleep(SESSION_IDLE - Duration::from_secs(1)).await;
    assert_eq!(*view.borrow(), Some(this_device()));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(*view.borrow(), None, "both sessions idled out");
}
