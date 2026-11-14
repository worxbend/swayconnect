//! Typed text never reaches the log (`AGENTS.md` section 5 rule 6).

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use swayconnect_protocol::{InputTarget, InputText};

use super::SinkError;
use super::tests::{Rig, this_device};

const WINDOW: InputTarget = InputTarget::Window(42);

/// A `MakeWriter` that keeps every log line.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn typed_text_never_reaches_the_log() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _default = tracing::subscriber::set_default(subscriber);

    let rig = Rig::new();
    let input = rig.service.connection(this_device());
    let text = InputText::parse("hunter2 Пароль 😀 swordfish-xyz").expect("valid text");
    input.type_text(WINDOW, &text).await.expect("typed");
    rig.scene.focus(Some(41));
    let _ = input.type_text(WINDOW, &text).await;
    rig.keyboard
        .fail_next(SinkError::Wayland("broken pipe".to_owned()));
    rig.scene.focus(Some(42));
    let _ = input.type_text(WINDOW, &text).await;

    let logs = String::from_utf8(captured.0.lock().expect("logs").clone()).expect("utf-8");
    assert!(logs.contains("input.text"), "the code logs lengths: {logs}");
    for word in ["hunter2", "Пароль", "😀", "swordfish", "U0068", "U041F"] {
        assert!(!logs.contains(word), "log holds {word:?}: {logs}");
    }
}
