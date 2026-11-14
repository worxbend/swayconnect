//! Input sessions (`docs/DESIGN.md` 11.7 item 6): the first delivered input of a connection
//! arms one; it ends after [`SESSION_IDLE`] without input, on disconnect, or by the panic
//! switch. The view names the device of the newest armed session.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use swayconnect_protocol::DeviceId;
use tokio::sync::watch;
use tokio::time::Instant;

pub const SESSION_IDLE: Duration = Duration::from_secs(10 * 60);

struct Live {
    connection: u64,
    device: DeviceId,
    last_input: Instant,
}

struct Inner {
    next_connection: AtomicU64,
    live: Mutex<Vec<Live>>,
    view: watch::Sender<Option<DeviceId>>,
}

impl Inner {
    fn live(&self) -> MutexGuard<'_, Vec<Live>> {
        // Every statement leaves the list valid, so a panic elsewhere cannot break it.
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Republishes the newest session's device. Call with the list locked, so views never
    /// go out of order.
    fn publish(&self, live: &[Live]) {
        let holder = live
            .iter()
            .max_by_key(|session| session.last_input)
            .map(|session| session.device.clone());
        self.view.send_if_modified(|current| {
            let changed = *current != holder;
            *current = holder;
            changed
        });
    }

    fn deadline(&self, connection: u64) -> Option<Instant> {
        self.live()
            .iter()
            .find(|session| session.connection == connection)
            .map(|session| session.last_input + SESSION_IDLE)
    }

    /// Ends the session when it saw no input for [`SESSION_IDLE`]. `true` once it is gone.
    fn expire_if_idle(&self, connection: u64) -> bool {
        let mut live = self.live();
        let now = Instant::now();
        let before = live.len();
        live.retain(|session| {
            session.connection != connection || now < session.last_input + SESSION_IDLE
        });
        let still_armed = live.iter().any(|session| session.connection == connection);
        if live.len() != before {
            self.publish(&live);
        }
        !still_armed
    }
}

pub(super) struct Sessions {
    inner: Arc<Inner>,
}

impl Default for Sessions {
    fn default() -> Sessions {
        Sessions {
            inner: Arc::new(Inner {
                next_connection: AtomicU64::new(0),
                live: Mutex::new(Vec::new()),
                view: watch::channel(None).0,
            }),
        }
    }
}

impl Sessions {
    pub(super) fn next_id(&self) -> u64 {
        self.inner.next_connection.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn view(&self) -> watch::Receiver<Option<DeviceId>> {
        self.inner.view.subscribe()
    }

    /// Records input now. `true` when this armed a new session; its caller then starts the
    /// expiry with [`Sessions::spawn_expiry`].
    pub(super) fn touch(&self, connection: u64, device: &DeviceId) -> bool {
        let mut live = self.inner.live();
        let now = Instant::now();
        let armed = match live
            .iter_mut()
            .find(|session| session.connection == connection)
        {
            Some(session) => {
                session.last_input = now;
                false
            }
            None => {
                live.push(Live {
                    connection,
                    device: device.clone(),
                    last_input: now,
                });
                true
            }
        };
        self.inner.publish(&live);
        armed
    }

    pub(super) fn end(&self, connection: u64) {
        let mut live = self.inner.live();
        live.retain(|session| session.connection != connection);
        self.inner.publish(&live);
    }

    pub(super) fn end_all(&self) {
        let mut live = self.inner.live();
        live.clear();
        self.inner.publish(&live);
    }

    /// Ends the session of `connection` once it is idle. Must be called inside a tokio
    /// runtime. The task stops when the session ends any other way.
    pub(super) fn spawn_expiry(&self, connection: u64) {
        let inner: Weak<Inner> = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                let deadline = inner.upgrade()?.deadline(connection)?;
                tokio::time::sleep_until(deadline).await;
                if inner.upgrade()?.expire_if_idle(connection) {
                    return None::<()>;
                }
            }
        });
    }
}
