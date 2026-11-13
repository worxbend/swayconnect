//! A runnable fake sway, for checks by hand with a daemon that must not reach the real desktop.
//!
//! Usage: `fake_sway <dir> [forbidden socket path ...]`
//!
//! Serves the default fixtures on `<dir>/sway-ipc.sock` and prints every `RUN_COMMAND`
//! payload on its own line. Pass the real sway socket as a forbidden path. Ctrl-C ends it
//! and removes the socket.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use swayconnect_testkit::FakeSway;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let Some(dir) = arguments.next() else {
        eprintln!("usage: fake_sway <dir> [forbidden socket path ...]");
        return ExitCode::from(2);
    };
    let forbidden: Vec<PathBuf> = arguments.collect();

    let sway = FakeSway::start(&dir, &forbidden);
    println!("listening: {}", sway.socket_path().display());

    let mut printed = 0;
    let mut poll = tokio::time::interval(POLL_INTERVAL);
    loop {
        tokio::select! {
            _ = poll.tick() => {}
            _ = tokio::signal::ctrl_c() => return ExitCode::SUCCESS,
        }
        let commands = sway.commands();
        for command in &commands[printed..] {
            println!("RUN_COMMAND: {command}");
        }
        printed = commands.len();
    }
}
