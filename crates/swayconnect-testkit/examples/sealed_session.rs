//! A sealed session for a manual walk (`docs/M2C_PLAN.md` 3): the same `RealSession`, guard
//! and teardown as `just real`. Prints the session's environment as `export` lines, then
//! runs until stdin ends (Ctrl+D, or the writer of a pipe going away) and tears the
//! session down.
//!
//! `just sealed-session [--allow-user-systemd]`; the flag adds the one D1 link. An example,
//! not a bin of `src/`, because it reads the test process's environment, as a test does.
//!
//! Killed by a signal instead, Drop does not run: the session's supervisor still kills its
//! process group, but the temp root stays in `/tmp`.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use swayconnect_testkit::{LiveEnv, RealSession};

const USAGE: &str = "usage: just sealed-session [--allow-user-systemd]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let user_systemd = match args.as_slice() {
        [] => false,
        [flag] if flag == "--allow-user-systemd" => true,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut session = RealSession::start(&LiveEnv::from_vars(std::env::vars_os()));
    if user_systemd {
        session.allow_user_systemd();
    }
    let mut stdout = std::io::stdout().lock();
    for (name, value) in session.env() {
        let value = value.to_string_lossy().replace('\'', r"'\''");
        writeln!(stdout, "export {name}='{value}'").expect("write the environment");
    }
    writeln!(
        stdout,
        "# process group {}; end stdin to stop the session",
        session.process_group()
    )
    .expect("write the environment");
    stdout.flush().expect("flush the environment");
    // Any end of stdin, also a read error, stops the session.
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    drop(session);
    ExitCode::SUCCESS
}
