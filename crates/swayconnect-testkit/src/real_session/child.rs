//! Child-process helpers of the sealed session: ready lines with a deadline, short
//! commands with a timeout, a watcher process that dies with its handle, and the
//! processes of a group.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};
use serde_json::Value;

/// The lines a child writes to stdout, read on a thread so a wait can time out.
/// The thread ends at end of file, when every writer of the pipe is gone.
pub(crate) struct Lines {
    receiver: Receiver<String>,
}

impl Lines {
    /// One item per text line.
    pub(crate) fn read(stdout: ChildStdout) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                // Keep reading after the receiver is gone, so the writer never gets EPIPE.
                let _ = sender.send(line);
            }
        });
        Self { receiver }
    }

    /// One item per JSON value, as compact JSON text. `swaymsg -m` writes each event
    /// pretty-printed over several lines.
    pub(crate) fn read_json_values(stdout: ChildStdout) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let values = serde_json::Deserializer::from_reader(stdout).into_iter::<Value>();
            for value in values {
                let Ok(value) = value else { return };
                let _ = sender.send(value.to_string());
            }
        });
        Self { receiver }
    }

    /// The first line that `wanted` accepts before `timeout` runs out. `None` on timeout
    /// or end of file.
    pub(crate) fn wait_for(
        &self,
        timeout: Duration,
        wanted: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.receiver.recv_timeout(left) {
                Ok(line) if wanted(&line) => return Some(line),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

/// A long-running helper (`pactl subscribe`, `swaymsg -t subscribe -m`) and its stdout.
/// Dropping it kills and reaps the process.
pub(crate) struct Watcher {
    child: Child,
    pub(crate) lines: Lines,
}

impl Watcher {
    /// Starts `command` and reads its stdout with `reader` ([`Lines::read`] or
    /// [`Lines::read_json_values`]).
    pub(crate) fn spawn(mut command: Command, reader: fn(ChildStdout) -> Lines) -> Self {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start {command:?}: {error}"));
        let stdout = child.stdout.take().expect("piped stdout");
        Self {
            child,
            lines: reader(stdout),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Already gone is fine: the result is only the reaping.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How often [`output_within`] looks whether its child has ended.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// Runs a short command to its end and returns its output. Panics when it fails to
/// start, runs longer than `timeout` (it is then killed), or exits with an error.
pub(crate) fn run_within(command: Command, timeout: Duration) -> Output {
    let program = format!("{command:?}");
    let output = output_within(command, timeout);
    assert!(
        output.status.success(),
        "{program} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    output
}

/// Runs a short command to its end and returns its output, whatever its exit status.
/// Panics when it fails to start or runs longer than `timeout` (it is then killed).
///
/// The child stays on this thread: `try_wait` and `kill` go through the same `Child`,
/// and std never signals a child it has already reaped, so the kill cannot reach a
/// process that reused the pid.
pub(crate) fn output_within(mut command: Command, timeout: Duration) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("cannot start {command:?}: {error}"));
    let stdout = read_all(child.stdout.take().expect("piped stdout"));
    let stderr = read_all(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(EXIT_POLL),
            Ok(None) => {
                // Already gone is fine: the wait below reaps it either way.
                let _ = child.kill();
                let _ = child.wait();
                panic!("{command:?} ran longer than {timeout:?}");
            }
            Err(error) => panic!("cannot wait for {command:?}: {error}"),
        }
    };
    Output {
        status,
        stdout: stdout.join().expect("stdout reader"),
        stderr: stderr.join().expect("stderr reader"),
    }
}

fn read_all(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).expect("read child output");
        bytes
    })
}

/// Pids of the processes in process group `group` that have not exited (zombies are
/// left out), from `/proc`.
pub fn live_processes_in_group(group: u32) -> Vec<u32> {
    fs::read_dir("/proc")
        .expect("read /proc")
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            stat_fields(*pid).is_some_and(|fields| {
                is_running(&fields) && fields.get(2) == Some(&group.to_string())
            })
        })
        .collect()
}

fn is_alive(pid: u32) -> bool {
    stat_fields(pid).is_some_and(|fields| is_running(&fields))
}

fn is_running(fields: &[String]) -> bool {
    fields
        .first()
        .is_some_and(|state| !matches!(state.as_str(), "Z" | "X"))
}

/// The fields of `/proc/<pid>/stat` after `comm`: state, ppid, pgrp, ... `None` when the
/// process has exited meanwhile.
fn stat_fields(pid: u32) -> Option<Vec<String>> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // "pid (comm) state ppid pgrp ...": comm may hold spaces and parentheses.
    let (_, rest) = stat.rsplit_once(')')?;
    Some(rest.split_whitespace().map(str::to_owned).collect())
}

/// How long [`leftovers_naming`] gives processes to leave with the compositor (teardown,
/// not readiness).
const LEFTOVER_TIMEOUT: Duration = Duration::from_secs(5);

/// Pids whose command line or environment names `runtime_dir`, zombies left out.
pub fn processes_naming(runtime_dir: &Path) -> Vec<u32> {
    let needle = runtime_dir.to_string_lossy().into_owned();
    fs::read_dir("/proc")
        .expect("read /proc")
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| is_alive(*pid))
        .filter(|pid| {
            ["cmdline", "environ"].iter().any(|file| {
                fs::read(format!("/proc/{pid}/{file}"))
                    .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&needle))
            })
        })
        .collect()
}

/// [`processes_naming`] once they had time to leave with the compositor: a client the
/// daemon started, or foot's shell after `setsid`, gets its SIGHUP or socket error a moment
/// after the session's group is gone. Any still there is killed (it names this test's own
/// runtime dir), so a failing check leaves nothing running.
pub fn leftovers_naming(runtime_dir: &Path) -> Vec<u32> {
    let deadline = Instant::now() + LEFTOVER_TIMEOUT;
    let mut left = processes_naming(runtime_dir);
    while !left.is_empty() && Instant::now() < deadline {
        thread::sleep(EXIT_POLL);
        left = processes_naming(runtime_dir);
    }
    for pid in &left {
        if let Some(pid) = i32::try_from(*pid).ok().and_then(Pid::from_raw) {
            // Gone meanwhile is fine.
            let _ = kill_process(pid, Signal::KILL);
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_process_is_in_a_live_process_group() {
        let own_group = rustix::process::getpgrp()
            .as_raw_nonzero()
            .get()
            .unsigned_abs();

        assert!(live_processes_in_group(own_group).contains(&std::process::id()));
    }
}
