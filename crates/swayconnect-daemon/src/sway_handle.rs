//! Seam between the sway half and the session code. Signatures frozen by
//! `docs/IMPLEMENTATION_PLAN.md` section 5: private fields may be added, public names and
//! meanings stay.
//!
//! Slice S0 wrote the stubs. Slice S6b wrote the state half, slice S6d the body of `act`.
//! Slice D-POLL added `watch_desktop` and `windows_inhibiting`.

use std::sync::Arc;

use swayconnect_protocol::Desktop;
use swayconnect_sway::SwayTarget;
use tokio::sync::{mpsc, oneshot, watch};

use crate::icons::IconService;
use crate::model;
use crate::supervisor::{self, Desk};

/// No `Debug`: the desktop holds window titles, which are never logged.
pub struct Published {
    pub rev: u64,
    pub desktop: Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    FocusWorkspaceNumber(u16),
    FocusWorkspaceId(u64),
    FocusWindow(u64),
    Fullscreen { window_id: u64, enabled: bool },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    NotFound,
    UnsupportedTarget,
    SwayUnavailable,
    /// Sway's text, already safe to send: cleaned and cut by `actions.rs`.
    SwayError(String),
    Timeout,
}

/// Actions that wait for the supervisor (`docs/DESIGN.md` 4.1).
const ACTION_QUEUE_LEN: usize = 32;

/// One action on its way to the supervisor, which owns the command connection.
pub(crate) struct ActionRequest {
    pub(crate) action: Action,
    pub(crate) reply: oneshot::Sender<Result<(), ActionError>>,
}

pub struct SwayHandle {
    state: watch::Receiver<Arc<Published>>,
    windows_inhibiting: watch::Receiver<u32>,
    subscribers: Arc<watch::Sender<usize>>,
    actions: mpsc::Sender<ActionRequest>,
}

/// While one lives, the supervisor reads the tree again every 2 s, because sway sends no
/// event for a resize (`docs/DESIGN.md` 11.5). A session holds one while it subscribes to
/// `desktop` and drops it when it ends.
#[must_use = "the poll stops when this is dropped"]
pub struct DesktopInterest {
    subscribers: Arc<watch::Sender<usize>>,
}

impl Drop for DesktopInterest {
    fn drop(&mut self) {
        self.subscribers.send_modify(|count| *count -= 1);
    }
}

impl SwayHandle {
    /// Starts the supervisor task. It ends when `shutdown` turns `true` or closes. Every
    /// window icon is `null`.
    ///
    /// Must be called inside a tokio runtime.
    pub fn spawn(target: SwayTarget, shutdown: watch::Receiver<bool>) -> SwayHandle {
        SwayHandle::spawn_with_icons(target, shutdown, None)
    }

    /// [`SwayHandle::spawn`], and with `icons` the windows carry the hash of their app icon
    /// once the service has it.
    pub fn spawn_with_icons(
        target: SwayTarget,
        shutdown: watch::Receiver<bool>,
        icons: Option<Arc<IconService>>,
    ) -> SwayHandle {
        let (publisher, state) = watch::channel(Arc::new(Published {
            rev: 0,
            desktop: model::offline_desktop(),
        }));
        let (actions, queued) = mpsc::channel(ACTION_QUEUE_LEN);
        let (inhibit_publisher, windows_inhibiting) = watch::channel(0);
        let (subscribers, subscriber_count) = watch::channel(0);
        let desk = Desk {
            state: publisher,
            windows_inhibiting: inhibit_publisher,
            subscribers: subscriber_count,
            actions: queued,
            icon_jobs: icons.as_ref().map(|service| service.finished()),
            icons,
        };
        tokio::spawn(supervisor::run(target, shutdown, desk));
        SwayHandle {
            state,
            windows_inhibiting,
            subscribers: Arc::new(subscribers),
            actions,
        }
    }

    /// The newest snapshot. Offline with `rev` 0 until sway is first reached.
    pub fn state(&self) -> watch::Receiver<Arc<Published>> {
        self.state.clone()
    }

    /// The number of windows that keep the desktop awake by themselves (sway
    /// `inhibit_idle`), from the newest tree. 0 while sway is offline.
    pub fn windows_inhibiting(&self) -> watch::Receiver<u32> {
        self.windows_inhibiting.clone()
    }

    /// Counts one more desktop subscriber until the result is dropped.
    pub fn watch_desktop(&self) -> DesktopInterest {
        self.subscribers.send_modify(|count| *count += 1);
        DesktopInterest {
            subscribers: Arc::clone(&self.subscribers),
        }
    }

    /// Asks the supervisor to run `action`. It checks the target against the snapshot it
    /// published last and sends one typed command (`docs/DESIGN.md` 4.4).
    pub async fn act(&self, action: Action) -> Result<(), ActionError> {
        let (reply, outcome) = oneshot::channel();
        let request = ActionRequest { action, reply };
        // A closed queue: the supervisor has ended, so the daemon is stopping.
        self.actions
            .send(request)
            .await
            .map_err(|_| ActionError::SwayUnavailable)?;
        // The supervisor stopped while it held the request: the command may have been sent.
        outcome.await.unwrap_or(Err(ActionError::Timeout))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use swayconnect_testkit::{FakeSway, Request, TestDirs};

    use super::*;

    const TEST_DEADLINE: Duration = Duration::from_secs(10);
    const FOCUS_WINDOW: Action = Action::FocusWindow(6);
    const EVERY_ACTION: [Action; 5] = [
        Action::FocusWorkspaceNumber(3),
        Action::FocusWorkspaceId(4),
        FOCUS_WINDOW,
        Action::Fullscreen {
            window_id: 6,
            enabled: true,
        },
        Action::Fullscreen {
            window_id: 7,
            enabled: false,
        },
    ];

    struct Desk {
        _dirs: TestDirs,
        sway: FakeSway,
        handle: SwayHandle,
        // Dropping the sender stops the supervisor.
        shutdown: watch::Sender<bool>,
    }

    /// Waits for a snapshot that satisfies `wanted`.
    async fn snapshot_where(handle: &SwayHandle, wanted: impl Fn(&Published) -> bool) {
        let mut state = handle.state();
        tokio::time::timeout(TEST_DEADLINE, state.wait_for(|published| wanted(published)))
            .await
            .expect("the snapshot changes in time")
            .expect("supervisor runs");
    }

    impl Desk {
        /// A handle whose snapshot holds the default fixture: workspaces 4 and 5, windows 6 and 7.
        async fn online() -> Desk {
            let dirs = TestDirs::new();
            let sway = FakeSway::start(&dirs.runtime_dir(), &[]);
            let (shutdown, stop) = watch::channel(false);
            let handle = SwayHandle::spawn(SwayTarget::Explicit(dirs.sway_socket()), stop);
            snapshot_where(&handle, |published| published.desktop.sway.online).await;
            Desk {
                _dirs: dirs,
                sway,
                handle,
                shutdown,
            }
        }
    }

    #[tokio::test]
    async fn each_action_sends_exactly_one_command() {
        let desk = Desk::online().await;

        for (sent_before, action) in EVERY_ACTION.into_iter().enumerate() {
            assert_eq!(desk.handle.act(action).await, Ok(()), "{action:?}");
            assert_eq!(desk.sway.commands().len(), sent_before + 1, "{action:?}");
        }
    }

    #[tokio::test]
    async fn an_id_of_the_other_kind_or_of_no_entry_sends_no_command() {
        let desk = Desk::online().await;
        let workspace_as_window = [
            Action::FocusWindow(4),
            Action::Fullscreen {
                window_id: 5,
                enabled: true,
            },
        ];
        let window_as_workspace = [Action::FocusWorkspaceId(6)];
        let in_neither_list = [Action::FocusWindow(2), Action::FocusWorkspaceId(99)];

        for action in workspace_as_window
            .into_iter()
            .chain(window_as_workspace)
            .chain(in_neither_list)
        {
            let outcome = desk.handle.act(action).await;
            assert_eq!(outcome, Err(ActionError::NotFound), "{action:?}");
        }

        assert_eq!(desk.sway.commands(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn only_one_successful_result_is_a_success() {
        let desk = Desk::online().await;
        let refused_replies = [
            "[]",
            r#"[{"success":true},{"success":true}]"#,
            r#"[{"success":false,"error":"No matching node."}]"#,
            r#"[{"success":false}]"#,
            r#"{"success":true}"#,
        ];

        for reply in refused_replies {
            desk.sway.set_reply(Request::RunCommand, reply);
            let outcome = desk.handle.act(FOCUS_WINDOW).await;
            assert!(matches!(outcome, Err(ActionError::SwayError(_))), "{reply}");
        }
        desk.sway
            .set_reply(Request::RunCommand, r#"[{"success":true}]"#);
        assert_eq!(desk.handle.act(FOCUS_WINDOW).await, Ok(()));

        assert!(desk.handle.state().borrow().desktop.sway.online);
    }

    #[tokio::test]
    async fn the_error_text_of_sway_is_cleaned_and_cut() {
        let desk = Desk::online().await;
        let hostile = format!("bad\\u001b[2J{}", "x".repeat(300));
        let reply = format!(r#"[{{"success":false,"error":"{hostile}"}}]"#);
        desk.sway.set_reply(Request::RunCommand, reply);

        let Err(ActionError::SwayError(text)) = desk.handle.act(FOCUS_WINDOW).await else {
            panic!("sway refused the command");
        };

        assert_eq!(text.chars().count(), 200);
        assert!(text.starts_with("bad\u{fffd}[2Jxxx"), "{text:?}");
    }

    #[tokio::test]
    async fn without_sway_every_action_is_unavailable() {
        let dirs = TestDirs::new();
        let (_shutdown, stop) = watch::channel(false);
        let handle = SwayHandle::spawn(SwayTarget::Explicit(dirs.sway_socket()), stop);

        for action in EVERY_ACTION {
            let outcome = handle.act(action).await;
            assert_eq!(outcome, Err(ActionError::SwayUnavailable), "{action:?}");
        }
    }

    #[tokio::test]
    async fn after_sway_went_away_actions_are_unavailable() {
        let desk = Desk::online().await;
        drop(desk.sway);
        snapshot_where(&desk.handle, |published| !published.desktop.sway.online).await;

        let outcome = desk.handle.act(FOCUS_WINDOW).await;

        assert_eq!(outcome, Err(ActionError::SwayUnavailable));
    }

    #[tokio::test]
    async fn a_sway_that_does_not_answer_is_a_timeout_and_the_link_is_rebuilt() {
        let desk = Desk::online().await;
        let rev_before = desk.handle.state().borrow().rev;
        desk.sway.stop_answering();

        let outcome = desk.handle.act(FOCUS_WINDOW).await;

        assert_eq!(outcome, Err(ActionError::Timeout));
        desk.sway.resume_answering();
        // Offline is one publish, online again the next.
        snapshot_where(&desk.handle, |published| {
            published.desktop.sway.online && published.rev >= rev_before + 2
        })
        .await;
        assert_eq!(desk.handle.act(FOCUS_WINDOW).await, Ok(()));
    }

    #[tokio::test]
    async fn each_desktop_interest_counts_until_it_is_dropped() {
        let desk = Desk::online().await;
        let count = || *desk.handle.subscribers.borrow();

        let first = desk.handle.watch_desktop();
        let second = desk.handle.watch_desktop();
        assert_eq!(count(), 2);

        drop(first);
        assert_eq!(count(), 1);
        drop(second);
        assert_eq!(count(), 0);
    }

    #[tokio::test]
    async fn after_shutdown_actions_are_unavailable() {
        let desk = Desk::online().await;
        desk.shutdown.send(true).expect("supervisor listens");
        let mut state = desk.handle.state();
        // The supervisor owns the only sender of the state.
        let ended = async { while state.changed().await.is_ok() {} };
        tokio::time::timeout(TEST_DEADLINE, ended)
            .await
            .expect("the supervisor ends in time");

        let outcome = desk.handle.act(FOCUS_WINDOW).await;

        assert_eq!(outcome, Err(ActionError::SwayUnavailable));
        assert_eq!(desk.sway.commands(), Vec::<String>::new());
    }
}
