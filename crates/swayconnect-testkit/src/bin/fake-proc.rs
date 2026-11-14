//! `fake-proc`: stands in for every program a test daemon starts (`docs/DESIGN.md` 11.12).
//!
//! Its own path is `argv[0]`, which must be absolute: the testkit installs it under each
//! program's name and a test config names that path. It appends its arguments to
//! `<own path>.log`, writes `<own path>.stdout` if present, then does what
//! `<own path>.mode` says (default: exit 0). It reads no environment.
//! A fault of the fake itself exits 125, so it never passes for a scripted result.
//!
//! The build script compiles this file alone with `rustc`, so it uses `std` only.

#![forbid(unsafe_code)]

#[path = "../fake_proc_spec.rs"]
mod fake_proc_spec;

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use fake_proc_spec::{LOG_SUFFIX, MODE_SUFFIX, Mode, STDOUT_SUFFIX, json_string, sibling};

const FAULT: u8 = 125;
const HANG_STEP: Duration = Duration::from_secs(3600);

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("fake-proc: {message}");
            ExitCode::from(FAULT)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut argv = std::env::args_os();
    let own_path = argv
        .next()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("argv[0] is not an absolute path")?;
    let arguments: Vec<OsString> = argv.collect();

    append_call(&own_path, &arguments)?;
    write_scripted_stdout(&own_path)?;
    Ok(act(read_mode(&own_path)?))
}

fn append_call(own_path: &Path, arguments: &[OsString]) -> Result<(), String> {
    let log_path = sibling(own_path, LOG_SUFFIX);
    let mut line = json_array(arguments);
    line.push('\n');
    // One write of the whole line with O_APPEND, so parallel calls do not interleave.
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut log| log.write_all(line.as_bytes()))
        .map_err(|error| format!("cannot append to {}: {error}", log_path.display()))
}

fn write_scripted_stdout(own_path: &Path) -> Result<(), String> {
    let stdout_path = sibling(own_path, STDOUT_SUFFIX);
    let Some(bytes) = read_if_present(&stdout_path)? else {
        return Ok(());
    };
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write stdout: {error}"))
}

fn read_mode(own_path: &Path) -> Result<Mode, String> {
    let mode_path = sibling(own_path, MODE_SUFFIX);
    let Some(bytes) = read_if_present(&mode_path)? else {
        return Ok(Mode::Exit(0));
    };
    let text =
        String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", mode_path.display()))?;
    text.parse()
}

fn read_if_present(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn act(mode: Mode) -> ExitCode {
    match mode {
        Mode::Exit(code) => ExitCode::from(code),
        Mode::Sleep(duration) => {
            std::thread::sleep(duration);
            ExitCode::SUCCESS
        }
        Mode::Hang => loop {
            std::thread::sleep(HANG_STEP);
        },
    }
}

/// Arguments that are not UTF-8 are logged with U+FFFD.
fn json_array(arguments: &[OsString]) -> String {
    let items: Vec<String> = arguments
        .iter()
        .map(|argument| json_string(&argument.to_string_lossy()))
        .collect();
    format!("[{}]", items.join(","))
}
