//! Starts the M2a services (`docs/DESIGN.md` 11.6, 11.8): keep awake, topic `session`, the
//! lock command, and the task that ends keep-awake when its holder is revoked. Slice
//! D-WIRE.2a.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::watch;

use crate::audit::AuditLog;
use crate::config::Config;
use crate::keep_awake::{self, KeepAwake, KeepAwakeDeps, PowerOn};
use crate::lockstate::ProcScanner;
use crate::notify::Notifier;
use crate::proc::{BoxFuture, Runner};
use crate::security::Security;
use crate::session::{Controls, stopped};
use crate::session_topic::{Locker, SessionSources, SessionTopic};
use crate::sway_handle::{Action, ActionError, SwayHandle};

/// What the services read from the host.
pub struct HostParts {
    pub runner: Arc<dyn Runner>,
    pub proc_root: PathBuf,
}

/// Keep awake rule 2 (`docs/PROTOCOL.md` 17.8) through the sway handle.
struct SwayPower(Arc<SwayHandle>);

impl PowerOn for SwayPower {
    fn power_on(&self) -> BoxFuture<'_, Result<(), ActionError>> {
        Box::pin(self.0.act(Action::OutputsPowerOn))
    }
}

/// Starts the tasks; each ends when `shutdown` turns `true`. Needs a tokio runtime.
pub fn start(
    config: &Config,
    host: HostParts,
    sway: &Arc<SwayHandle>,
    audit: AuditLog,
    shutdown: &watch::Receiver<bool>,
) -> Controls {
    let HostParts { runner, proc_root } = host;
    let uid = rustix::process::getuid().as_raw();
    let scanner = Arc::new(ProcScanner::new(proc_root, uid, &config.session));
    let deps = KeepAwakeDeps {
        runner: Arc::clone(&runner),
        notifier: Notifier::new(Arc::clone(&runner), &config.notify),
        desktop: sway.state(),
        power: Arc::new(SwayPower(Arc::clone(sway))),
        start_check: keep_awake::START_CHECK,
    };
    let keep_awake = KeepAwake::spawn(&config.keep_awake, deps, shutdown.clone());
    let sources = SessionSources {
        keep_awake: keep_awake.view(),
        windows_inhibiting: sway.windows_inhibiting(),
        scanner: Arc::clone(&scanner),
    };
    let session_topic = Arc::new(SessionTopic::spawn(sources, shutdown.clone()));
    let locker = Locker::new(runner, &config.session, scanner).map(Arc::new);
    Controls {
        keep_awake,
        session_topic,
        locker,
        audit,
    }
}

/// Ends keep-awake when the device that holds it is revoked (17.8 rule 6). A revoke drops
/// the device's scope signal, so its watch closes; a pending device holds nothing.
pub async fn release_on_revoke(
    keep_awake: KeepAwake,
    security: Security,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut view = keep_awake.view();
    loop {
        let holder = view
            .borrow_and_update()
            .hold
            .as_ref()
            .map(|hold| hold.holder.clone());
        let revoked = async {
            let Some(holder) = &holder else {
                return std::future::pending().await;
            };
            // `None`: revoked before this task looked.
            if let Some(mut scopes) = security.scope_watch(holder) {
                while scopes.changed().await.is_ok() {}
            }
        };
        tokio::select! {
            biased;
            () = stopped(&mut shutdown) => return,
            changed = view.changed() => if changed.is_err() { return },
            () = revoked => {
                if let Some(holder) = &holder {
                    tracing::info!(device = holder.as_str(), "keep-awake holder revoked");
                    keep_awake.release_holder(holder).await;
                    // A release changes the view unless the task has stopped; wait for that,
                    // so a stale holder is never released twice in a row.
                    if view.changed().await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}
