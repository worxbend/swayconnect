//! The log lines of a daemon that runs on the thread of the test.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tracing::subscriber::DefaultGuard;

#[derive(Clone, Default)]
struct SharedText(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedText {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut text = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        text.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Collects what is logged at info and above on this thread until it is dropped. A
/// `#[tokio::test]` runs the daemon's tasks on this thread too.
pub struct CapturedLog {
    text: SharedText,
    _subscription: DefaultGuard,
}

impl CapturedLog {
    pub fn start() -> CapturedLog {
        let text = SharedText::default();
        let writer = text.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        CapturedLog {
            text,
            _subscription: tracing::subscriber::set_default(subscriber),
        }
    }

    /// How many lines hold `text`.
    pub fn lines_with(&self, text: &str) -> usize {
        let logged = self.text.0.lock().unwrap_or_else(PoisonError::into_inner);
        let logged = String::from_utf8_lossy(&logged);
        logged.lines().filter(|line| line.contains(text)).count()
    }
}
