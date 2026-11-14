//! A [`Runner`] that starts nothing (`docs/DESIGN.md` 11.12). It records every call, answers
//! `run` with scripted replies, and lets a test end or inspect each spawned child. Works
//! under paused tokio time.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use super::{
    BoxFuture, Captured, ChildCommand, Exit, ExitProbe, Finished, LineChild, Lines, ProcError,
    Runner, Running, try_exit_of, wait_exit,
};

/// Pids of fake children start here, far from the test's own pid.
const FIRST_FAKE_PID: u32 = 4_000_000;
const SIGKILL: i32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Run(Duration),
    Spawn,
    SpawnLines,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub kind: CallKind,
    pub command: ChildCommand,
}

impl Call {
    /// The program, then the arguments, as text.
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.command.program().as_os_str())
            .chain(
                self.command
                    .args()
                    .iter()
                    .map(|argument| argument.as_os_str()),
            )
            .map(|word| word.to_string_lossy().into_owned())
            .collect()
    }
}

#[derive(Clone, Default)]
pub struct FakeRunner {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    calls: Vec<Call>,
    run_replies: VecDeque<Result<Finished, ProcError>>,
    failing_spawns: usize,
    children: Vec<ChildSlot>,
}

struct ChildSlot {
    exit: watch::Sender<Option<Exit>>,
    killed: bool,
    /// The stdout of a `spawn_lines` child until it ends; dropping it is the end of stream.
    lines: Option<mpsc::UnboundedSender<String>>,
}

impl FakeRunner {
    pub fn calls(&self) -> Vec<Call> {
        self.state().calls.clone()
    }

    /// The argv of every call, oldest first.
    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.state().calls.iter().map(Call::argv).collect()
    }

    /// The reply of the next `run` without a scripted reply is exit 0 with no output.
    pub fn reply_next_run(&self, reply: Result<Finished, ProcError>) {
        self.state().run_replies.push_back(reply);
    }

    /// The next `spawn` fails as if the program were missing.
    pub fn fail_next_spawn(&self) {
        self.state().failing_spawns += 1;
    }

    /// Ends the `index`th spawned child (0 = first) unless it already ended. Its stdout ends
    /// too.
    pub fn end_child(&self, index: usize, exit: Exit) {
        let mut state = self.state();
        let slot = &mut state.children[index];
        slot.lines = None;
        set_exit(&slot.exit, exit);
    }

    /// Writes `line` to the stdout of the `index`th spawned child, a `spawn_lines` child that
    /// still runs.
    pub fn send_line(&self, index: usize, line: &str) {
        let state = self.state();
        let sender = state.children[index].lines.as_ref();
        let sent = sender.is_some_and(|sender| sender.send(line.to_owned()).is_ok());
        assert!(sent, "child {index} has no open stdout");
    }

    pub fn was_killed(&self, index: usize) -> bool {
        self.state().children[index].killed
    }

    pub fn spawned_count(&self) -> usize {
        self.state().children.len()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

impl Runner for FakeRunner {
    fn run(
        &self,
        command: ChildCommand,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<Finished, ProcError>> {
        let mut state = self.state();
        state.calls.push(Call {
            kind: CallKind::Run(timeout),
            command,
        });
        let reply = state.run_replies.pop_front().unwrap_or_else(|| {
            Ok(Finished {
                exit: Exit::Code(0),
                stdout: Captured::default(),
                stderr: Captured::default(),
            })
        });
        Box::pin(async move { reply })
    }

    fn spawn(&self, command: ChildCommand) -> Result<Box<dyn Running>, ProcError> {
        self.start(CallKind::Spawn, command, None)
    }

    fn spawn_lines(&self, command: ChildCommand) -> Result<LineChild, ProcError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let child = self.start(CallKind::SpawnLines, command, Some(sender))?;
        Ok(LineChild {
            child,
            lines: Box::new(FakeLines(receiver)),
        })
    }
}

impl FakeRunner {
    fn start(
        &self,
        kind: CallKind,
        command: ChildCommand,
        lines: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<Box<dyn Running>, ProcError> {
        let mut state = self.state();
        let program = command.program().to_owned();
        state.calls.push(Call { kind, command });
        if state.failing_spawns > 0 {
            state.failing_spawns -= 1;
            return Err(ProcError::Spawn {
                program,
                source: io::ErrorKind::NotFound.into(),
            });
        }
        let (exit, exit_seen) = watch::channel(None);
        state.children.push(ChildSlot {
            exit,
            killed: false,
            lines,
        });
        Ok(Box::new(FakeChild {
            index: state.children.len() - 1,
            state: Arc::clone(&self.state),
            exit: exit_seen,
        }))
    }
}

struct FakeLines(mpsc::UnboundedReceiver<String>);

impl Lines for FakeLines {
    fn next_line(&mut self) -> BoxFuture<'_, Result<Option<String>, ProcError>> {
        Box::pin(async move { Ok(self.0.recv().await) })
    }
}

struct FakeChild {
    index: usize,
    state: Arc<Mutex<State>>,
    exit: watch::Receiver<Option<Exit>>,
}

impl Running for FakeChild {
    fn pid(&self) -> u32 {
        FIRST_FAKE_PID + self.index as u32
    }

    fn try_exit(&mut self) -> Result<Option<Exit>, ProcError> {
        try_exit_of(&self.exit)
    }

    fn exit_probe(&self) -> ExitProbe {
        ExitProbe::new(self.exit.clone())
    }

    fn exited(&mut self) -> BoxFuture<'_, Result<Exit, ProcError>> {
        Box::pin(wait_exit(&mut self.exit))
    }

    fn kill(&mut self) -> Result<(), ProcError> {
        let mut state = lock(&self.state);
        let slot = &mut state.children[self.index];
        if slot.exit.borrow().is_none() {
            slot.killed = true;
            slot.lines = None;
            set_exit(&slot.exit, Exit::Signal(SIGKILL));
        }
        Ok(())
    }
}

fn set_exit(sender: &watch::Sender<Option<Exit>>, exit: Exit) {
    sender.send_if_modified(|current| {
        let running = current.is_none();
        if running {
            *current = Some(exit);
        }
        running
    });
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
