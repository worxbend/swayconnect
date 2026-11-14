//! Starting an app under `systemd-run --user --scope`, and the one launcher child of
//! `launcher.open` (`docs/DESIGN.md` 11.6, `docs/PROTOCOL.md` 17.11).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use swayconnect_protocol::{DeviceId, Held, Holder, LauncherOpenResult};
use tokio::sync::{oneshot, watch};

use super::config::AppsConfig;
use super::index::IndexedApp;
use crate::proc::{ChildCommand, Exit, ExitProbe, ProcError, ProgramArgv, Runner, Running};

/// How long a new app is watched for an early failure, such as `systemd-run` finding no
/// user manager. An app still running then, or ended with 0, counts as started.
pub const START_CHECK: Duration = Duration::from_millis(500);
const UNIT_PREFIX: &str = "app-swayconnect-";
/// Keeps the unit name far below systemd's 255 bytes; entry ids are ASCII.
const MAX_UNIT_ID_BYTES: usize = 64;

/// `[apps] systemd_run --user --scope --collect --unit=app-swayconnect-<id>-<suffix> --`,
/// then the terminal argv for a terminal entry, then the entry's `Exec` tokens. Every
/// word comes from config or the daemon's own index; `suffix` is random hex.
pub fn launch_command(config: &AppsConfig, app: &IndexedApp, suffix: &str) -> ChildCommand {
    let unit_id = &app.entry_id[..app.entry_id.len().min(MAX_UNIT_ID_BYTES)];
    let command = ChildCommand::new(&config.systemd_run)
        .arg("--user")
        .arg("--scope")
        .arg("--collect")
        .arg(format!("--unit={UNIT_PREFIX}{unit_id}-{suffix}"))
        .arg("--");
    let command = if app.terminal {
        let terminal = &config.terminal;
        let program = terminal.program().as_path().as_os_str().to_owned();
        terminal
            .args()
            .iter()
            .fold(command.arg(program), |command, word| command.arg(word))
    } else {
        command
    };
    app.argv
        .iter()
        .fold(command, |command, word| command.arg(word))
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Proc(#[from] ProcError),
    #[error("the app ended at once with {0:?}")]
    Exited(Exit),
}

/// Spawns `command` and waits up to [`START_CHECK`] for an early failure. The child is
/// left running; its waiter reaps it when it ends.
pub async fn start(runner: &dyn Runner, command: ChildCommand) -> Result<(), StartError> {
    let mut child = runner.spawn(command)?;
    match tokio::time::timeout(START_CHECK, child.exited()).await {
        Err(_still_running) => Ok(()),
        Ok(Ok(exit)) if exit.success() => Ok(()),
        Ok(Ok(exit)) => Err(StartError::Exited(exit)),
        Ok(Err(error)) => Err(error.into()),
    }
}

/// The running launcher: who opened it, the window focused then, its pid (for the
/// keyboard-grabber exception of `PROTOCOL.md` 17.13) and its exit, which the input guard
/// reads before each part (`DESIGN.md` 11.7 item 3.2) instead of waiting for this view to
/// clear.
#[derive(Debug, Clone)]
pub struct OpenLauncher {
    pub holder: DeviceId,
    pub focus_window_id: Option<u64>,
    pub pid: u32,
    pub exit: ExitProbe,
}

/// Member `launcher` of topic `session` for `device`.
pub fn held_for(open: Option<&OpenLauncher>, device: &DeviceId) -> Held {
    Held {
        running: open.is_some(),
        holder: open.map(|open| {
            if open.holder == *device {
                Holder::ThisDevice
            } else {
                Holder::OtherDevice
            }
        }),
    }
}

/// Why `launcher.open` started nothing.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The panic switch closed the launcher for the rest of the daemon's life.
    #[error("the launcher is closed until the daemon restarts")]
    Closed,
    #[error(transparent)]
    Spawn(#[from] ProcError),
}

/// What the stop lock guards: the running child's stop signal, and whether
/// [`Launcher::close`] ran.
#[derive(Default)]
struct StopSlot {
    sender: Option<oneshot::Sender<()>>,
    closed: bool,
}

/// At most one `[launcher] argv` child. A task per child clears the view when it ends.
/// Dropping the `Launcher` kills a running child.
pub struct Launcher {
    argv: ProgramArgv,
    runner: Arc<dyn Runner>,
    view: Arc<watch::Sender<Option<OpenLauncher>>>,
    stop: Mutex<StopSlot>,
}

impl Launcher {
    pub fn new(argv: ProgramArgv, runner: Arc<dyn Runner>) -> Launcher {
        Launcher {
            argv,
            runner,
            view: Arc::new(watch::channel(None).0),
            stop: Mutex::new(StopSlot::default()),
        }
    }

    /// Starts the launcher unless one runs; the caller becomes its holder only then.
    /// Must be called inside a tokio runtime.
    pub fn open(
        &self,
        device: &DeviceId,
        focus_window_id: Option<u64>,
    ) -> Result<LauncherOpenResult, OpenError> {
        let mut stop = self.lock_stop();
        // Under the lock: a `close` that already ran, or runs next, sees this child.
        if stop.closed {
            return Err(OpenError::Closed);
        }
        if self.view.borrow().is_some() {
            return Ok(LauncherOpenResult { already_open: true });
        }
        let child = self.runner.spawn(ChildCommand::from(&self.argv))?;
        self.view.send_replace(Some(OpenLauncher {
            holder: device.clone(),
            focus_window_id,
            pid: child.pid(),
            exit: child.exit_probe(),
        }));
        let (stop_sender, stop_receiver) = oneshot::channel();
        stop.sender = Some(stop_sender);
        tokio::spawn(watch_child(child, stop_receiver, Arc::clone(&self.view)));
        Ok(LauncherOpenResult {
            already_open: false,
        })
    }

    /// The running launcher, `None` once its child ended.
    pub fn view(&self) -> watch::Receiver<Option<OpenLauncher>> {
        self.view.subscribe()
    }

    /// Kills a running launcher, waits until it ended, and refuses every later `open`
    /// (panic switch, `DESIGN.md` 11.8: until the daemon restarts).
    pub async fn close(&self) {
        let mut view = self.view.subscribe();
        let sender = {
            let mut stop = self.lock_stop();
            stop.closed = true;
            stop.sender.take()
        };
        if let Some(sender) = sender {
            // A closed receiver means the watch task already saw the child end.
            let _ = sender.send(());
        }
        // Errs only when the sender is gone, which `self` prevents.
        let _ = view.wait_for(Option::is_none).await;
    }

    fn lock_stop(&self) -> MutexGuard<'_, StopSlot> {
        self.stop.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Waits for the child's end, or kills it on `stop` (also when the `Launcher` is dropped).
async fn watch_child(
    mut child: Box<dyn Running>,
    stop: oneshot::Receiver<()>,
    view: Arc<watch::Sender<Option<OpenLauncher>>>,
) {
    let ended = tokio::select! {
        exit = child.exited() => exit,
        _ = stop => match child.kill() {
            Ok(()) => child.exited().await,
            Err(error) => Err(error),
        },
    };
    if let Err(error) = ended {
        tracing::warn!(%error, "launcher child state lost; treating it as closed");
    }
    view.send_replace(None);
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
