//! `FakeKeyboard`: a [`KeyboardSink`] that records what it gets (`docs/DESIGN.md` 11.12).
//! Tests never open a Wayland socket.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::keymap::{Delivery, KeyEvent};
use super::{KeyboardSink, SinkError};
use crate::proc::BoxFuture;

/// One call the keyboard got, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Recorded {
    Delivery(Delivery),
    ReleaseAll,
}

type Hook = Box<dyn Fn(usize) + Send + Sync>;

#[derive(Default)]
struct Inner {
    log: Mutex<Vec<Recorded>>,
    hook: Mutex<Option<Hook>>,
    failures: Mutex<Vec<SinkError>>,
}

#[derive(Clone, Default)]
pub(crate) struct FakeKeyboard {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeKeyboard {
    pub(crate) fn log(&self) -> Vec<Recorded> {
        lock(&self.inner.log).clone()
    }

    pub(crate) fn deliveries(&self) -> Vec<Delivery> {
        self.log()
            .into_iter()
            .filter_map(|recorded| match recorded {
                Recorded::Delivery(delivery) => Some(delivery),
                Recorded::ReleaseAll => None,
            })
            .collect()
    }

    /// The text of each delivered part, read back through its keymap.
    pub(crate) fn typed_parts(&self) -> Vec<String> {
        self.deliveries().iter().map(typed).collect()
    }

    /// Key presses over all deliveries.
    pub(crate) fn presses(&self) -> usize {
        self.deliveries()
            .iter()
            .flat_map(|delivery| &delivery.events)
            .filter(|event| matches!(event, KeyEvent::Press(_)))
            .count()
    }

    /// Runs `hook` after every delivery with the number of deliveries so far, for example
    /// to move focus in the fake sway before the next check.
    pub(crate) fn after_delivery(&self, hook: impl Fn(usize) + Send + Sync + 'static) {
        *lock(&self.inner.hook) = Some(Box::new(hook));
    }

    /// The next delivery fails with `error` and records nothing.
    pub(crate) fn fail_next(&self, error: SinkError) {
        lock(&self.inner.failures).push(error);
    }
}

impl KeyboardSink for FakeKeyboard {
    fn deliver(&self, delivery: Delivery) -> BoxFuture<'_, Result<(), SinkError>> {
        Box::pin(async move {
            if let Some(error) = lock(&self.inner.failures).pop() {
                return Err(error);
            }
            let count = {
                let mut log = lock(&self.inner.log);
                log.push(Recorded::Delivery(delivery));
                log.iter()
                    .filter(|recorded| matches!(recorded, Recorded::Delivery(_)))
                    .count()
            };
            if let Some(hook) = lock(&self.inner.hook).as_ref() {
                hook(count);
            }
            Ok(())
        })
    }

    fn release_all(&self) {
        lock(&self.inner.log).push(Recorded::ReleaseAll);
    }
}

/// The characters `delivery` types: each press looked up in its keymap. Only `U<hex>`
/// keys type a character; named keys are skipped.
pub(crate) fn typed(delivery: &Delivery) -> String {
    let names: HashMap<u32, &str> = delivery
        .keymap
        .as_str()
        .lines()
        .filter_map(|line| {
            let (code, rest) = line.strip_prefix("key <C")?.split_once("> { [ ")?;
            Some((code.parse().ok()?, rest.strip_suffix(" ] };")?))
        })
        .collect();
    delivery
        .events
        .iter()
        .filter_map(|event| match event {
            KeyEvent::Press(code) => names.get(&(code + 8)),
            _ => None,
        })
        .filter_map(|name| {
            let hex = name.strip_prefix('U')?;
            char::from_u32(u32::from_str_radix(hex, 16).ok()?)
        })
        .collect()
}
