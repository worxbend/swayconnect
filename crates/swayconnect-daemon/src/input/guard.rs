//! The checks before every part (`docs/PROTOCOL.md` 17.13 rule 2, `docs/DESIGN.md` 11.7
//! item 3), in order: lock, launcher, keyboard grab, focus. Each check reads fresh state:
//! a `/proc` scan and one `GET_TREE`. Any check that cannot tell refuses (11.3 rule 10).

use std::path::PathBuf;
use std::sync::Arc;

use swayconnect_protocol::{DeviceId, InputTarget};
use swayconnect_sway::{Connection, Node, SwayTarget, resolve};
use tokio::sync::{Mutex, watch};

use crate::apps::OpenLauncher;
use crate::lockstate::{DesktopFacts, ProcScanner};
use crate::model::is_window;

/// Why a part may not be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Locked, or the lock state is unknown.
    Locked,
    LauncherClosed,
    KeyboardGrabbed,
    /// Sway reported this window as focused; `None` for none.
    FocusChanged(Option<u64>),
    /// Sway did not answer the focus read.
    SwayUnavailable,
}

/// Reads the focused window from sway on every call, over its own IPC connection. Sway
/// answers in its main loop, so the reply shows focus at that moment; no earlier answer
/// and no event is reused (17.13 rule 3).
pub struct SwayFocus {
    target: SwayTarget,
    connection: Mutex<Option<Connection>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FocusError {
    #[error("no sway socket found")]
    NoSway,
    #[error(transparent)]
    Sway(#[from] swayconnect_sway::Error),
}

impl SwayFocus {
    pub fn new(target: SwayTarget) -> SwayFocus {
        SwayFocus {
            target,
            connection: Mutex::new(None),
        }
    }

    /// The focused window. A broken connection is replaced once, so a sway restart costs
    /// no refusal.
    pub async fn focused_window(&self) -> Result<Option<u64>, FocusError> {
        let mut slot = self.connection.lock().await;
        if let Some(connection) = slot.as_ref() {
            match connection.tree().await {
                Ok(tree) => return Ok(focused_window(&tree)),
                Err(error) => {
                    tracing::debug!(%error, "focus connection lost; connecting again");
                    *slot = None;
                }
            }
        }
        let path = self.socket()?;
        let connection = Connection::connect(&path).await?;
        let tree = connection.tree().await?;
        *slot = Some(connection);
        Ok(focused_window(&tree))
    }

    fn socket(&self) -> Result<PathBuf, FocusError> {
        resolve(&self.target).ok_or(FocusError::NoSway)
    }
}

/// The focused window of `tree`: the focused node when it is a window, else `None` (a
/// focused workspace or container).
pub fn focused_window(tree: &Node) -> Option<u64> {
    let focused = find_focused(tree)?;
    is_window(focused).then_some(focused.id)
}

fn find_focused(node: &Node) -> Option<&Node> {
    if node.focused {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(find_focused)
}

/// The four checks of 17.13 rule 2.
pub struct Guard {
    scanner: Arc<ProcScanner>,
    focus: SwayFocus,
    launcher: watch::Receiver<Option<OpenLauncher>>,
}

impl Guard {
    /// `launcher` is `Launcher::view()`.
    pub fn new(
        scanner: ProcScanner,
        focus: SwayFocus,
        launcher: watch::Receiver<Option<OpenLauncher>>,
    ) -> Guard {
        Guard {
            scanner: Arc::new(scanner),
            focus,
            launcher,
        }
    }

    /// `Ok` when a part for `target` may go to the keyboard now.
    pub async fn check(&self, device: &DeviceId, target: InputTarget) -> Result<(), Stop> {
        let facts = self.scan().await.ok_or(Stop::Locked)?;
        if facts.locked {
            return Err(Stop::Locked);
        }
        let (launcher_pid, expected) = match target {
            InputTarget::Window(id) => (None, Some(id)),
            InputTarget::Launcher => {
                let open = self.own_launcher(device).ok_or(Stop::LauncherClosed)?;
                (Some(open.pid), open.focus_window_id)
            }
        };
        if facts
            .grabbers
            .iter()
            .any(|grabber| Some(grabber.pid) != launcher_pid)
        {
            return Err(Stop::KeyboardGrabbed);
        }
        let focused = self.focus.focused_window().await.map_err(|error| {
            tracing::warn!(%error, "focus unknown; input refused");
            Stop::SwayUnavailable
        })?;
        if focused == expected {
            Ok(())
        } else {
            Err(Stop::FocusChanged(focused))
        }
    }

    /// The running launcher when `device` opened it. The child's own exit is read, so a
    /// launcher that just ended counts as closed before its view clears.
    fn own_launcher(&self, device: &DeviceId) -> Option<OpenLauncher> {
        self.launcher
            .borrow()
            .as_ref()
            .filter(|open| open.holder == *device)
            .filter(|open| matches!(open.exit.try_exit(), Ok(None)))
            .cloned()
    }

    /// `None`: lock state unknown.
    async fn scan(&self) -> Option<DesktopFacts> {
        let scanner = Arc::clone(&self.scanner);
        match tokio::task::spawn_blocking(move || scanner.scan()).await {
            Ok(Ok(facts)) => Some(facts),
            Ok(Err(error)) => {
                tracing::warn!(%error, "lock state unknown; input refused");
                None
            }
            Err(error) => {
                tracing::warn!(%error, "lock scan did not finish; input refused");
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
