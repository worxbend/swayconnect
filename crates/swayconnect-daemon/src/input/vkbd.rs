//! The daemon's virtual keyboard (`docs/DESIGN.md` 11.7 item 1): one Wayland connection on
//! a dedicated thread, one `zwp_virtual_keyboard_v1` on the configured seat for the life of
//! the connection. Commands arrive over a channel and run in order. Each delivery ends with
//! a roundtrip, so the compositor has processed the keys before the next check. A failed
//! connection is dropped and the next delivery connects again (a compositor restart).
//!
//! `just ci` never runs this: its tests use `FakeKeyboard`. `tests/real_input.rs` (`just real`)
//! types through it into a headless sway; the owner checks the live desktop in gate G-INPUT.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Write as _};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, SealFlags};
use tokio::sync::oneshot;
use wayland_client::globals::{BindError, GlobalError, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, QueueHandle, delegate_noop,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

use super::config::{SeatName, WaylandTarget};
use super::keymap::{Delivery, KeyEvent, Keymap};
use super::{KeyboardSink, SinkError};
use crate::proc::BoxFuture;

/// A delivery of one part takes milliseconds; longer means the compositor hangs.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
/// The oldest focus check a part may still be written on (`docs/DESIGN.md` 11.7 item 7).
/// A part that waited longer behind a stuck delivery is refused, not written late.
const MAX_CHECK_AGE: Duration = Duration::from_millis(250);
/// `wl_keyboard.keymap_format.xkb_v1`.
const KEYMAP_FORMAT_XKB_V1: u32 = 1;
/// `wl_keyboard.key_state`.
const KEY_RELEASED: u32 = 0;
const KEY_PRESSED: u32 = 1;
/// `wl_seat.name` needs version 2.
const SEAT_VERSION: u32 = 2;

enum Command {
    /// A part, the time its focus check passed, and where its result goes.
    Deliver(Delivery, Instant, oneshot::Sender<Result<(), SinkError>>),
    ReleaseAll,
}

/// What the keyboard thread does with one command. `fresh` says whether the part may still
/// be written; the job asks it again right before the first key.
enum Job<'a> {
    Deliver(&'a Delivery, &'a dyn Fn() -> Result<(), SinkError>),
    ReleaseAll,
}

/// The [`KeyboardSink`] on a real compositor.
pub struct VirtualKeyboard {
    commands: mpsc::Sender<Command>,
}

impl VirtualKeyboard {
    /// Starts the thread. It connects at the first delivery and ends when this is dropped.
    pub fn spawn(target: WaylandTarget, seat: SeatName) -> io::Result<VirtualKeyboard> {
        let mut keyboard: Option<Keyboard> = None;
        VirtualKeyboard::spawn_worker(move |job| {
            let result = match job {
                Job::Deliver(delivery, fresh) => {
                    deliver(&mut keyboard, &target, &seat, delivery, fresh)
                }
                Job::ReleaseAll => keyboard.as_mut().map_or(Ok(()), Keyboard::release_all),
            };
            if let Err(error) = &result
                && *error != SinkError::TimedOut
            {
                tracing::warn!(%error, "virtual keyboard failed; it connects again on the next input");
                keyboard = None;
            }
            result
        })
    }

    /// Runs `work` on its own thread, one command after the other.
    fn spawn_worker<W>(work: W) -> io::Result<VirtualKeyboard>
    where
        W: FnMut(Job<'_>) -> Result<(), SinkError> + Send + 'static,
    {
        let (commands, received) = mpsc::channel();
        std::thread::Builder::new()
            .name("swayconnect-vkbd".to_owned())
            .spawn(move || run(&received, work))?;
        Ok(VirtualKeyboard { commands })
    }
}

impl KeyboardSink for VirtualKeyboard {
    fn deliver(&self, delivery: Delivery) -> BoxFuture<'_, Result<(), SinkError>> {
        // Called right after the focus check of this part.
        let checked_at = Instant::now();
        Box::pin(async move {
            let (reply, answer) = oneshot::channel();
            self.commands
                .send(Command::Deliver(delivery, checked_at, reply))
                .map_err(|_| SinkError::Stopped)?;
            match tokio::time::timeout(DELIVERY_TIMEOUT, answer).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(SinkError::Stopped),
                Err(_) => Err(SinkError::TimedOut),
            }
        })
    }

    fn release_all(&self) {
        // A stopped thread has dropped its keyboard, and the compositor released its keys.
        let _ = self.commands.send(Command::ReleaseAll);
    }
}

fn run<W>(commands: &mpsc::Receiver<Command>, mut work: W)
where
    W: FnMut(Job<'_>) -> Result<(), SinkError>,
{
    for command in commands {
        match command {
            Command::Deliver(delivery, checked_at, reply) => {
                let fresh = || still_fresh(checked_at, &reply);
                let result = fresh().and_then(|()| work(Job::Deliver(&delivery, &fresh)));
                // The caller may have timed out and gone.
                let _ = reply.send(result);
            }
            Command::ReleaseAll => {
                // Logged and handled by `work`; nobody waits for the result.
                let _ = work(Job::ReleaseAll);
            }
        }
    }
}

/// `TimedOut` once the caller gave up or the focus check is older than [`MAX_CHECK_AGE`]:
/// focus may have moved since, so the part must not be written.
fn still_fresh(
    checked_at: Instant,
    reply: &oneshot::Sender<Result<(), SinkError>>,
) -> Result<(), SinkError> {
    if reply.is_closed() || checked_at.elapsed() > MAX_CHECK_AGE {
        Err(SinkError::TimedOut)
    } else {
        Ok(())
    }
}

fn deliver(
    slot: &mut Option<Keyboard>,
    target: &WaylandTarget,
    seat: &SeatName,
    delivery: &Delivery,
    fresh: &dyn Fn() -> Result<(), SinkError>,
) -> Result<(), SinkError> {
    let keyboard = match slot {
        Some(keyboard) => keyboard,
        None => slot.insert(Keyboard::connect(target, seat)?),
    };
    // A first connect can take a while; check again before the first key.
    fresh()?;
    keyboard.deliver(delivery)
}

/// The seats the registry offered, with their names once the compositor sent them.
#[derive(Default)]
struct State {
    seats: Vec<(wl_seat::WlSeat, Option<String>)>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut State,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut State,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wl_seat::Event::Name { name } = event
            && let Some(entry) = state.seats.iter_mut().find(|(known, _)| known == seat)
        {
            entry.1 = Some(name);
        }
    }
}

delegate_noop!(State: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ignore ZwpVirtualKeyboardV1);

struct Keyboard {
    _connection: Connection,
    queue: EventQueue<State>,
    state: State,
    keyboard: ZwpVirtualKeyboardV1,
    keymap: Option<Keymap>,
    pressed: BTreeSet<u32>,
    started: Instant,
}

impl Keyboard {
    fn connect(target: &WaylandTarget, seat_name: &SeatName) -> Result<Keyboard, SinkError> {
        let path = target.resolve().ok_or(SinkError::NoDisplay)?;
        let connection = Connection::from_socket(UnixStream::connect(path)?)?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
        let handle = queue.handle();
        let manager: ZwpVirtualKeyboardManagerV1 = globals.bind(&handle, 1..=1, ())?;
        let mut state = State::default();
        for global in globals.contents().clone_list() {
            if global.interface == "wl_seat" && global.version >= SEAT_VERSION {
                let seat = globals
                    .registry()
                    .bind(global.name, SEAT_VERSION, &handle, ());
                state.seats.push((seat, None));
            }
        }
        queue.roundtrip(&mut state)?;
        let seat = state
            .seats
            .iter()
            .find(|(_, name)| name.as_deref() == Some(seat_name.as_str()))
            .map(|(seat, _)| seat.clone())
            .ok_or(SinkError::NoSeat)?;
        let keyboard = manager.create_virtual_keyboard(&seat, &handle, ());
        // A compositor that refuses virtual keyboards raises a protocol error here.
        queue.roundtrip(&mut state)?;
        Ok(Keyboard {
            _connection: connection,
            queue,
            state,
            keyboard,
            keymap: None,
            pressed: BTreeSet::new(),
            started: Instant::now(),
        })
    }

    fn deliver(&mut self, delivery: &Delivery) -> Result<(), SinkError> {
        if self.keymap.as_ref() != Some(&delivery.keymap) {
            let (fd, size) = sealed_keymap(&delivery.keymap)?;
            self.keyboard.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
            self.keymap = Some(delivery.keymap.clone());
        }
        for event in &delivery.events {
            match *event {
                KeyEvent::Press(code) => self.key(code, KEY_PRESSED),
                KeyEvent::Release(code) => self.key(code, KEY_RELEASED),
                KeyEvent::Modifiers(mask) => self.keyboard.modifiers(mask, 0, 0, 0),
            }
        }
        self.queue.roundtrip(&mut self.state)?;
        Ok(())
    }

    fn release_all(&mut self) -> Result<(), SinkError> {
        for code in std::mem::take(&mut self.pressed) {
            self.key(code, KEY_RELEASED);
        }
        self.keyboard.modifiers(0, 0, 0, 0);
        self.queue.roundtrip(&mut self.state)?;
        Ok(())
    }

    fn key(&mut self, code: u32, state: u32) {
        if state == KEY_PRESSED {
            self.pressed.insert(code);
        } else {
            self.pressed.remove(&code);
        }
        self.keyboard.key(self.millis(), code, state);
    }

    /// Event time in milliseconds; Wayland times wrap around.
    fn millis(&self) -> u32 {
        (self.started.elapsed().as_millis() % (u128::from(u32::MAX) + 1)) as u32
    }
}

/// The keymap in a sealed memfd: NUL-terminated, as `wl_keyboard.keymap` expects, and
/// sealed against any change, so the compositor reads exactly these bytes.
pub(crate) fn sealed_keymap(keymap: &Keymap) -> Result<(OwnedFd, u32), SinkError> {
    let bytes = keymap.as_str().as_bytes();
    let size = u32::try_from(bytes.len() + 1).map_err(|_| SinkError::KeymapTooLarge)?;
    let fd = rustix::fs::memfd_create(
        "swayconnect-keymap",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .map_err(io::Error::from)?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.write_all(&[0])?;
    let fd = OwnedFd::from(file);
    let seals = SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL;
    rustix::fs::fcntl_add_seals(&fd, seals).map_err(io::Error::from)?;
    Ok((fd, size))
}

impl From<io::Error> for SinkError {
    fn from(error: io::Error) -> SinkError {
        SinkError::Wayland(error.to_string())
    }
}

impl From<ConnectError> for SinkError {
    fn from(error: ConnectError) -> SinkError {
        SinkError::Wayland(error.to_string())
    }
}

impl From<GlobalError> for SinkError {
    fn from(error: GlobalError) -> SinkError {
        SinkError::Wayland(error.to_string())
    }
}

impl From<BindError> for SinkError {
    fn from(error: BindError) -> SinkError {
        SinkError::Wayland(error.to_string())
    }
}

impl From<DispatchError> for SinkError {
    fn from(error: DispatchError) -> SinkError {
        SinkError::Wayland(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Seek as _};

    use super::*;
    use crate::input::keymap::text_part;

    #[test]
    fn the_keymap_fd_is_sealed_and_nul_terminated() {
        let keymap = text_part("ok").expect("typable").keymap;
        let (fd, size) = sealed_keymap(&keymap).expect("memfd");
        assert_eq!(size as usize, keymap.as_str().len() + 1);
        let seals = rustix::fs::fcntl_get_seals(&fd).expect("seals");
        assert!(seals.contains(SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW));
        assert!(seals.contains(SealFlags::SEAL));

        let mut file = File::from(fd);
        assert!(file.write_all(b"x").is_err(), "a sealed fd refuses writes");
        file.rewind().expect("rewind");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read");
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(&bytes[..bytes.len() - 1], keymap.as_str().as_bytes());
    }

    #[test]
    fn a_part_whose_check_is_old_or_whose_caller_left_is_not_written() {
        let (commands, received) = mpsc::channel();
        let delivery = text_part("a").expect("typable");
        let (old_reply, mut old_answer) = oneshot::channel();
        let old = Instant::now() - MAX_CHECK_AGE - Duration::from_millis(1);
        commands
            .send(Command::Deliver(delivery.clone(), old, old_reply))
            .expect("queued");
        let (gone_reply, gone_answer) = oneshot::channel();
        drop(gone_answer);
        commands
            .send(Command::Deliver(
                delivery.clone(),
                Instant::now(),
                gone_reply,
            ))
            .expect("queued");
        let (fresh_reply, mut fresh_answer) = oneshot::channel();
        commands
            .send(Command::Deliver(delivery, Instant::now(), fresh_reply))
            .expect("queued");
        drop(commands);
        let mut written = 0;
        run(&received, |_| {
            written += 1;
            Ok(())
        });
        assert_eq!(written, 1, "only the fresh part");
        assert_eq!(old_answer.try_recv(), Ok(Err(SinkError::TimedOut)));
        assert_eq!(fresh_answer.try_recv(), Ok(Ok(())));
    }

    /// A delivery stuck in the compositor: the caller times out and the next request checks
    /// and queues behind it. Its keys must not go out when the stuck one returns.
    #[tokio::test(start_paused = true)]
    async fn a_part_queued_behind_a_stuck_delivery_is_dropped() {
        let (unblock, blocked) = mpsc::channel::<()>();
        let (seen, jobs) = mpsc::channel::<&'static str>();
        let keyboard = VirtualKeyboard::spawn_worker(move |job| {
            if let Job::Deliver(..) = job {
                seen.send("written").expect("test listens");
                // The first delivery hangs until the test lets it go.
                let _ = blocked.recv();
            }
            Ok(())
        })
        .expect("thread");
        let delivery = text_part("a").expect("typable");
        let mut first = keyboard.deliver(delivery.clone());
        // One poll sends the command; then wait until the worker hangs in it.
        let polled = tokio::time::timeout(Duration::ZERO, &mut first).await;
        assert!(polled.is_err(), "still waiting");
        assert_eq!(jobs.recv(), Ok("written"));
        assert_eq!(first.await, Err(SinkError::TimedOut));
        let second = keyboard.deliver(delivery).await;
        assert_eq!(second, Err(SinkError::TimedOut));
        unblock.send(()).expect("worker waits");
        drop(unblock);
        drop(keyboard);
        let late: Vec<_> = jobs.iter().collect();
        assert!(
            late.is_empty(),
            "the second part reached the keyboard: {late:?}"
        );
    }

    #[tokio::test]
    async fn without_a_wayland_socket_delivery_fails_cleanly() {
        let target = WaylandTarget::Discover {
            runtime_dir: None,
            display: None,
        };
        let seat = SeatName::try_from("seat0".to_owned()).expect("seat");
        let keyboard = VirtualKeyboard::spawn(target, seat).expect("thread");
        let delivery = text_part("a").expect("typable");
        assert_eq!(keyboard.deliver(delivery).await, Err(SinkError::NoDisplay));
        keyboard.release_all();
    }
}
