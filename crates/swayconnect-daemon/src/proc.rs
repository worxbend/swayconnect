//! Child processes (`docs/DESIGN.md` 11.3 rule 8, 11.4): argv only, no shell. The program
//! is an absolute path from config, the environment comes from an allowlist, stdin is null,
//! output is read with a cap and never sent to a client, and `run` has a timeout and kill.
//!
//! Each child has one waiter thread that reaps it; signals go through a pidfd, so a kill
//! never reaches a reused pid. The async runtime needs no `process` feature, and its
//! `AsyncFd` constructors now need `unsafe`, which this crate forbids.

use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use rustix::io::Errno;
use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
use serde::Deserialize;
use tokio::io::AsyncReadExt as _;
use tokio::net::unix::pipe;
use tokio::sync::watch;

#[cfg(test)]
pub mod fake;
mod lines;

use self::lines::PipeLines;

/// The only variables a child gets, with the values of the desktop session. Every other
/// variable of the daemon's own environment is dropped.
pub const ENV_ALLOWLIST: [&str; 11] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "SWAYSOCK",
    "XDG_RUNTIME_DIR",
    "XDG_CURRENT_DESKTOP",
    "XDG_DATA_DIRS",
    "DBUS_SESSION_BUS_ADDRESS",
    // On the list of `docs/DESIGN.md` 11.6, so a test daemon's children never reach the real
    // system bus (11.12).
    "DBUS_SYSTEM_BUS_ADDRESS",
    "PATH",
    "HOME",
    "LANG",
];

/// The most bytes kept of one output stream of [`Runner::run`]. The rest is read and dropped,
/// so a chatty child never blocks on a full pipe.
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// The most bytes kept of one line of [`Runner::spawn_lines`]. The rest of that line is
/// read and dropped.
pub const MAX_LINE_BYTES: usize = 4096;

const READ_CHUNK_BYTES: usize = 8192;
const WAITER_THREAD_NAME: &str = "child-waiter";

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a program value of the config is refused (exit 2 at start).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProgramError {
    #[error("program path {0:?} is not absolute")]
    NotAbsolute(PathBuf),
    #[error("a program argv list must not be empty")]
    EmptyArgv,
}

/// An absolute program path from config. Deserialising refuses a relative path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PathBuf")]
pub struct ProgramPath(PathBuf);

impl ProgramPath {
    pub fn new(path: PathBuf) -> Result<ProgramPath, ProgramError> {
        if path.is_absolute() {
            Ok(ProgramPath(path))
        } else {
            Err(ProgramError::NotAbsolute(path))
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl TryFrom<PathBuf> for ProgramPath {
    type Error = ProgramError;

    fn try_from(path: PathBuf) -> Result<ProgramPath, ProgramError> {
        ProgramPath::new(path)
    }
}

/// A config argv list such as `["/path/lock.sh"]`: an absolute program, then fixed arguments.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<String>")]
pub struct ProgramArgv {
    program: ProgramPath,
    args: Vec<String>,
}

impl ProgramArgv {
    pub fn program(&self) -> &ProgramPath {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

impl TryFrom<Vec<String>> for ProgramArgv {
    type Error = ProgramError;

    fn try_from(argv: Vec<String>) -> Result<ProgramArgv, ProgramError> {
        let mut words = argv.into_iter();
        let program = words.next().ok_or(ProgramError::EmptyArgv)?;
        Ok(ProgramArgv {
            program: ProgramPath::new(PathBuf::from(program))?,
            args: words.collect(),
        })
    }
}

/// What to start: a config program and its arguments. The program is `argv[0]` as given;
/// it is never replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildCommand {
    program: ProgramPath,
    args: Vec<OsString>,
}

impl ChildCommand {
    pub fn new(program: &ProgramPath) -> ChildCommand {
        ChildCommand {
            program: program.clone(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, argument: impl Into<OsString>) -> ChildCommand {
        self.args.push(argument.into());
        self
    }

    pub fn program(&self) -> &Path {
        self.program.as_path()
    }

    pub fn args(&self) -> &[OsString] {
        &self.args
    }
}

impl From<&ProgramArgv> for ChildCommand {
    fn from(argv: &ProgramArgv) -> ChildCommand {
        argv.args()
            .iter()
            .fold(ChildCommand::new(argv.program()), |command, argument| {
                command.arg(argument)
            })
    }
}

/// The environment of every child: the [`ENV_ALLOWLIST`] part of the variables `main.rs`
/// passes in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildEnv {
    vars: Vec<(OsString, OsString)>,
}

impl ChildEnv {
    pub fn allowlisted(vars: impl IntoIterator<Item = (OsString, OsString)>) -> ChildEnv {
        let vars = vars
            .into_iter()
            .filter(|(name, _)| ENV_ALLOWLIST.iter().any(|allowed| name == allowed))
            .collect();
        ChildEnv { vars }
    }

    pub fn vars(&self) -> &[(OsString, OsString)] {
        &self.vars
    }
}

/// How a child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Exit {
    pub fn success(self) -> bool {
        self == Exit::Code(0)
    }
}

/// One output stream, cut at [`MAX_OUTPUT_BYTES`]. For parsing and logs only; never sent to
/// a client.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// A child that [`Runner::run`] waited for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub exit: Exit,
    pub stdout: Captured,
    pub stderr: Captured,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcError {
    #[error("cannot start {}: {source}", program.display())]
    Spawn {
        program: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("child process: {0}")]
    Io(#[from] io::Error),
    #[error("child process ran longer than {0:?} and was killed")]
    TimedOut(Duration),
    #[error("child process ended without an exit code or a signal")]
    NoExitStatus,
}

/// Starts child processes. `RealRunner` outside tests, `fake::FakeRunner` in unit tests.
pub trait Runner: Send + Sync + 'static {
    /// Runs the child to its end and returns its capped output. The child is killed when
    /// `timeout` passes, and also when the returned future is dropped early.
    fn run(
        &self,
        command: ChildCommand,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<Finished, ProcError>>;

    /// Starts a long-lived child with stdout and stderr on `/dev/null`.
    fn spawn(&self, command: ChildCommand) -> Result<Box<dyn Running>, ProcError>;

    /// Starts a long-lived child whose stdout is read line by line; stderr goes to
    /// `/dev/null`. Must be called inside a tokio runtime.
    fn spawn_lines(&self, command: ChildCommand) -> Result<LineChild, ProcError>;
}

/// A child started by [`Runner::spawn_lines`] and its stdout.
pub struct LineChild {
    pub child: Box<dyn Running>,
    pub lines: Box<dyn Lines>,
}

/// The stdout of a [`LineChild`]. For parsing only; never sent to a client.
pub trait Lines: Send {
    /// The next line without its newline, cut at [`MAX_LINE_BYTES`] and decoded lossily;
    /// `None` at the end of the stream. Cancel-safe.
    fn next_line(&mut self) -> BoxFuture<'_, Result<Option<String>, ProcError>>;
}

/// A child started by [`Runner::spawn`]. Dropping the handle leaves the child running (it
/// is reaped when it ends); call [`kill`](Running::kill) to end it.
pub trait Running: Send {
    fn pid(&self) -> u32;

    /// `None` while the child runs.
    fn try_exit(&mut self) -> Result<Option<Exit>, ProcError>;

    /// A cloneable [`try_exit`](Running::try_exit) for code that does not hold the child.
    fn exit_probe(&self) -> ExitProbe;

    /// Waits for the end. Cancel-safe.
    fn exited(&mut self) -> BoxFuture<'_, Result<Exit, ProcError>>;

    /// Sends `SIGKILL`; a child that already ended is left alone. Await
    /// [`exited`](Running::exited) afterwards for the end.
    fn kill(&mut self) -> Result<(), ProcError>;
}

/// Starts real programs with a cleared environment plus [`ChildEnv`], in `/`.
#[derive(Debug, Clone)]
pub struct RealRunner {
    env: ChildEnv,
}

impl RealRunner {
    pub fn new(env: ChildEnv) -> RealRunner {
        RealRunner { env }
    }

    fn std_command(&self, command: &ChildCommand) -> Command {
        let mut std_command = Command::new(command.program());
        std_command
            .args(command.args())
            .env_clear()
            .envs(self.env.vars().iter().map(|(name, value)| (name, value)))
            .current_dir("/")
            .stdin(Stdio::null());
        std_command
    }
}

impl Runner for RealRunner {
    fn run(
        &self,
        command: ChildCommand,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<Finished, ProcError>> {
        Box::pin(async move {
            let mut std_command = self.std_command(&command);
            std_command.stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = start(&mut std_command, &command)?;
            let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
            let mut child = KilledOnDrop(RealChild::adopt(child)?);
            let (stdout, stderr) = (receiver(stdout)?, receiver(stderr)?);
            let outcome = tokio::time::timeout(timeout, async {
                tokio::try_join!(read_capped(stdout), read_capped(stderr), child.0.exited())
            })
            .await;
            match outcome {
                Ok(Ok((stdout, stderr, exit))) => Ok(Finished {
                    exit,
                    stdout,
                    stderr,
                }),
                Ok(Err(error)) => Err(error),
                Err(_elapsed) => {
                    child.0.kill()?;
                    child.0.exited().await?;
                    Err(ProcError::TimedOut(timeout))
                }
            }
        })
    }

    fn spawn(&self, command: ChildCommand) -> Result<Box<dyn Running>, ProcError> {
        let mut std_command = self.std_command(&command);
        std_command.stdout(Stdio::null()).stderr(Stdio::null());
        let child = start(&mut std_command, &command)?;
        Ok(Box::new(RealChild::adopt(child)?))
    }

    fn spawn_lines(&self, command: ChildCommand) -> Result<LineChild, ProcError> {
        let mut std_command = self.std_command(&command);
        std_command.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = start(&mut std_command, &command)?;
        let stdout = child.stdout.take();
        let mut child = RealChild::adopt(child)?;
        match receiver(stdout) {
            Ok(stdout) => Ok(LineChild {
                child: Box::new(child),
                lines: Box::new(PipeLines::new(stdout)),
            }),
            Err(error) => {
                // Without its stdout the child is of no use; the waiter thread reaps it.
                let _ = child.kill();
                Err(error)
            }
        }
    }
}

fn start(std_command: &mut Command, command: &ChildCommand) -> Result<Child, ProcError> {
    std_command.spawn().map_err(|source| ProcError::Spawn {
        program: command.program().to_owned(),
        source,
    })
}

fn receiver(stream: Option<impl Into<OwnedFd>>) -> Result<pipe::Receiver, ProcError> {
    let stream = stream.ok_or_else(|| io::Error::other("child pipe missing"))?;
    Ok(pipe::Receiver::from_owned_fd(stream.into())?)
}

async fn read_capped(mut stream: pipe::Receiver) -> Result<Captured, ProcError> {
    let mut captured = Captured::default();
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(captured);
        }
        let room = MAX_OUTPUT_BYTES - captured.bytes.len();
        captured.bytes.extend_from_slice(&chunk[..read.min(room)]);
        captured.truncated |= read > room;
    }
}

/// A started child. A thread owns the `Child`, waits for it and reaps it, then publishes
/// the exit. Signals go through a pidfd taken before that thread starts, so a kill never
/// reaches a reused pid.
struct RealChild {
    pid: u32,
    pidfd: OwnedFd,
    exit: watch::Receiver<Option<Exit>>,
}

impl RealChild {
    fn adopt(mut child: Child) -> Result<RealChild, ProcError> {
        let pid = child.id();
        let pidfd = match pidfd_open(Pid::from_child(&child), PidfdFlags::empty()) {
            Ok(pidfd) => pidfd,
            Err(error) => {
                // The child cannot be signalled safely later, so it must not keep running.
                // These results add nothing to `error`: the kill fails only for an ended child.
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::from(error).into());
            }
        };
        let (exit_sender, exit) = watch::channel(None);
        let waiter = std::thread::Builder::new()
            .name(WAITER_THREAD_NAME.to_owned())
            .spawn(move || {
                // A failed wait drops the sender without an exit; `exited` reports that.
                if let Ok(Ok(exit)) = child.wait().map(exit_of) {
                    exit_sender.send_replace(Some(exit));
                }
            });
        if let Err(error) = waiter {
            // The child was moved into the thread that did not start; end it at least.
            let _ = pidfd_send_signal(&pidfd, Signal::KILL);
            return Err(error.into());
        }
        Ok(RealChild { pid, pidfd, exit })
    }
}

impl Running for RealChild {
    fn pid(&self) -> u32 {
        self.pid
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
        if self.try_exit()?.is_some() {
            return Ok(());
        }
        match pidfd_send_signal(&self.pidfd, Signal::KILL) {
            // Reaped between the check and the signal: it has ended, as wanted.
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(error) => Err(io::Error::from(error).into()),
        }
    }
}

/// Reads a child's exit as soon as its reaper published it, without the child handle.
#[derive(Debug, Clone)]
pub struct ExitProbe(watch::Receiver<Option<Exit>>);

impl ExitProbe {
    pub(crate) fn new(exit: watch::Receiver<Option<Exit>>) -> ExitProbe {
        ExitProbe(exit)
    }

    /// `None` while the child runs.
    pub fn try_exit(&self) -> Result<Option<Exit>, ProcError> {
        try_exit_of(&self.0)
    }
}

/// The published exit; an error when the waiter ended without one.
pub(crate) fn try_exit_of(exit: &watch::Receiver<Option<Exit>>) -> Result<Option<Exit>, ProcError> {
    // Read before the value: once closed, the value is final, so a `None` then is no race.
    let waiter_gone = exit.has_changed().is_err();
    let published = *exit.borrow();
    match published {
        Some(exit) => Ok(Some(exit)),
        None if waiter_gone => Err(ProcError::NoExitStatus),
        None => Ok(None),
    }
}

pub(crate) async fn wait_exit(exit: &mut watch::Receiver<Option<Exit>>) -> Result<Exit, ProcError> {
    let published = *exit
        .wait_for(Option::is_some)
        .await
        .map_err(|_| ProcError::NoExitStatus)?;
    published.ok_or(ProcError::NoExitStatus)
}

/// Kills the child of a `run` whose future is dropped, so a timeout cannot be escaped by
/// cancelling the caller.
struct KilledOnDrop(RealChild);

impl Drop for KilledOnDrop {
    fn drop(&mut self) {
        // An error means the child already ended or cannot be signalled; nothing is left to do.
        let _ = self.0.kill();
    }
}

fn exit_of(status: ExitStatus) -> Result<Exit, ProcError> {
    status
        .code()
        .map(Exit::Code)
        .or_else(|| status.signal().map(Exit::Signal))
        .ok_or(ProcError::NoExitStatus)
}

#[cfg(test)]
mod tests;
