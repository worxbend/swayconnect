//! `swayconnectd` as a child process: start, log lines, SIGTERM, SIGUSR1, and the admin
//! subcommands.
//!
//! Every child has a cleared environment, temp XDG directories, a config whose `sway.socket`
//! is the fake sway, and `listen = ["127.0.0.1:0"]` (`docs/DESIGN.md` 9.2).

mod support;

use std::ffi::OsString;
use std::fs::{self, Permissions};
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::json;
use swayconnect_cli::client::{ClientError, Connector, Profile};
use swayconnect_protocol::ByeCode;
use swayconnect_protocol::fingerprint::{encode_base64url, fingerprint};
use swayconnect_protocol::limits::DEFAULT_PORT;
use swayconnect_protocol::uri::PairingUri;
use swayconnect_testkit::{FakeSway, TestDirs};

const DAEMON: &str = env!("CARGO_BIN_EXE_swayconnectd");
const PATIENCE: Duration = Duration::from_secs(20);
const EXIT_FAILURE: i32 = 1;
const EXIT_USAGE: i32 = 2;

/// The lines a child writes to one of its pipes, read by a thread.
struct Lines {
    incoming: Receiver<String>,
    seen: Vec<String>,
}

impl Lines {
    fn of(pipe: impl Read + Send + 'static) -> Lines {
        let (sender, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Lines {
            incoming,
            seen: Vec::new(),
        }
    }

    /// `None` when the pipe closed, which means the child is gone.
    fn next(&mut self, deadline: Instant) -> Option<String> {
        let patience = deadline.saturating_duration_since(Instant::now());
        match self.incoming.recv_timeout(patience) {
            Ok(line) => {
                self.seen.push(line.clone());
                Some(line)
            }
            Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => panic!("no output in time; got {:#?}", self.seen),
        }
    }

    /// The next line that contains `text`.
    fn expect(&mut self, text: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            match self.next(deadline) {
                Some(line) if line.contains(text) => return line,
                Some(_) => {}
                None => panic!("the child ended before `{text}`; got {:#?}", self.seen),
            }
        }
    }

    fn until_closed(&mut self) {
        let deadline = Instant::now() + PATIENCE;
        while self.next(deadline).is_some() {}
    }
}

/// Kills the child when a test ends early, so no process outlives the test run.
struct Process {
    child: Child,
    lines: Lines,
}

impl Process {
    /// Waits for the output to end, then for the exit.
    fn exit_status(&mut self) -> ExitStatus {
        self.lines.until_closed();
        self.child.wait().expect("child exit status")
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Both fail when the child has exited already, which is the normal case.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Harness {
    dirs: TestDirs,
    sway: FakeSway,
}

/// The value after `name=` in a log line.
fn log_field<'line>(line: &'line str, name: &str) -> &'line str {
    let start = line.find(&format!("{name}=")).expect("log field") + name.len() + 1;
    line[start..]
        .split_whitespace()
        .next()
        .expect("field value")
}

impl Harness {
    /// Must run inside a tokio runtime: the fake sway spawns its accept task.
    fn new() -> Harness {
        let dirs = TestDirs::new();
        let sway = FakeSway::start(&dirs.runtime_dir(), &support::real_sway_sockets());
        let config = format!(
            "listen = [\"127.0.0.1:0\"]\nserver_name = \"desk\"\n[sway]\nsocket = \"{}\"\n",
            sway.socket_path().display()
        );
        fs::write(dirs.config_file(), config).expect("config file");
        Harness { dirs, sway }
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = self.dirs.command(DAEMON);
        command.args(arguments).stdin(Stdio::null());
        command
    }

    /// Starts `swayconnectd run` and waits until it listens and has reached the fake sway.
    fn start_daemon(&self) -> (Process, String) {
        let mut child = self
            .command(&["run"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("daemon starts");
        let lines = Lines::of(child.stderr.take().expect("stderr pipe"));
        let mut daemon = Process { child, lines };
        let started = daemon.lines.expect("swayconnectd started");
        daemon.lines.expect("listening");
        daemon.lines.expect("sway connected");
        (daemon, started)
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("subcommand runs")
    }

    /// Starts `swayconnectd pair` and reads the pairing URI from its output.
    fn start_pairing(&self, arguments: &[&str]) -> (Process, PairingUri) {
        let mut child = self
            .command(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("pair starts");
        let lines = Lines::of(child.stdout.take().expect("stdout pipe"));
        let mut pairing = Process { child, lines };
        let uri = pairing.lines.expect("swayconnect://pair");
        let uri = PairingUri::parse(&uri).expect("a valid pairing URI");
        (pairing, uri)
    }
}

fn answer(pairing: &mut Process, text: &str) {
    let stdin = pairing.child.stdin.as_mut().expect("stdin pipe");
    stdin.write_all(text.as_bytes()).expect("answer written");
}

fn send_signal(daemon: &Process, signal: &str) {
    let pid = daemon.child.id().to_string();
    let kill = Command::new("kill")
        .args([signal, &pid])
        .status()
        .expect("kill runs");
    assert!(kill.success());
}

/// Pairs a device with every scope through `swayconnectd pair` and the owner's yes.
async fn paired_device(harness: &Harness) -> Profile {
    let (mut pairing, uri) = harness.start_pairing(&["pair"]);
    let pairing_client = Connector::new(None);
    let pending = support::soon("pair", pairing_client.pair(&uri, "test phone", None)).await;
    let pending = pending.expect("pending pairing");
    pairing.lines.expect("code: ");
    answer(&mut pairing, "y\n");
    let profile = support::soon("pairing.confirmed", pending.confirmed()).await;
    assert!(pairing.exit_status().success());
    profile.expect("pairing confirmed")
}

fn focus_window_6(id: u64) -> Vec<u8> {
    let request = json!({"type": "req", "id": id, "op": "window.focus", "args": {"window_id": 6}});
    request.to_string().into_bytes()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_logs_address_and_fingerprint_and_stops_cleanly_on_sigterm() {
    let harness = Harness::new();
    let (mut daemon, started) = harness.start_daemon();

    let certificate = fs::read(harness.dirs.data_dir().join("identity.cert.der")).expect("cert");
    let expected_fingerprint = encode_base64url(&fingerprint(&certificate));
    assert_eq!(log_field(&started, "fingerprint"), expected_fingerprint);
    let listening = daemon
        .lines
        .seen
        .iter()
        .find(|line| line.contains("listening"));
    let address = log_field(listening.expect("listening line"), "address");
    let address: std::net::SocketAddr = address.parse().expect("a socket address");
    assert!(address.ip().is_loopback());
    assert!(![0, DEFAULT_PORT].contains(&address.port()));
    let admin_socket = harness.dirs.runtime_dir().join("swayconnect/admin.sock");
    assert!(admin_socket.exists());

    send_signal(&daemon, "-TERM");

    assert_eq!(daemon.exit_status().code(), Some(0));
    assert!(
        !admin_socket.exists(),
        "admin.sock is removed on a clean stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigusr1_switches_a_running_daemon_to_read_only() {
    let harness = Harness::new();
    let (mut daemon, _) = harness.start_daemon();
    let profile = paired_device(&harness).await;
    let connector = Connector::new(None);
    let connect = || support::soon("auth", connector.connect(&profile));
    let mut session = connect().await.expect("authenticated");
    assert!(!session.welcome().read_only);
    assert!(session.welcome().ops.iter().any(|op| op == "window.focus"));
    // The snapshot must hold window 6 before it can be focused.
    support::online_desktop(&mut session).await;

    let accepted = support::raw_exchange(&mut session, &focus_window_6(1)).await;
    assert_eq!(accepted["id"], 1);
    assert_eq!(accepted["ok"], true, "{accepted:?}");
    assert_eq!(harness.sway.commands(), ["[con_id=6] focus"]);

    send_signal(&daemon, "-USR1");
    daemon.lines.expect("read-only mode entered");

    let refused = support::raw_exchange(&mut session, &focus_window_6(2)).await;
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], "read_only");
    assert_eq!(harness.sway.commands().len(), 1, "nothing new reached sway");

    let later = connect().await.expect("authenticated");
    assert!(later.welcome().read_only);
    assert_eq!(later.welcome().ops, ["ping", "state.subscribe"]);

    send_signal(&daemon, "-TERM");
    assert_eq!(daemon.exit_status().code(), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_devices_and_revoke_work_against_a_running_daemon() {
    let harness = Harness::new();
    let (_daemon, _) = harness.start_daemon();
    assert_eq!(
        stdout(&harness.run(&["devices"])).trim(),
        "no paired devices"
    );

    let (mut pairing, uri) = harness.start_pairing(&["pair", "--ttl", "30"]);
    assert_eq!(uri.name.as_deref(), Some("desk"));
    assert!(![0, DEFAULT_PORT].contains(&uri.port));
    let connector = Connector::new(Some("process-test".to_owned()));
    let pending = tokio::time::timeout(PATIENCE, connector.pair(&uri, "test phone", Some("linux")))
        .await
        .expect("pairing in time")
        .expect("pending pairing");

    let shown = pairing.lines.expect("pending: ");
    assert!(
        shown.starts_with("pending: test phone (linux) from 127.0.0.1:"),
        "{shown}"
    );
    assert_eq!(
        pairing.lines.expect("code: "),
        format!("code: {}", pending.confirm_code())
    );
    answer(&mut pairing, "y\n");
    let profile = tokio::time::timeout(PATIENCE, pending.confirmed())
        .await
        .expect("confirmation in time")
        .expect("pairing confirmed");
    let id = profile.device_id.as_str();
    let paired = pairing.lines.expect("paired: ");
    let expected = format!("paired: test phone id {id} scopes state.read,state.titles,control");
    assert_eq!(paired, format!("Confirm this device? [y/N] {expected}"));
    assert!(pairing.exit_status().success());

    let listed = stdout(&harness.run(&["devices"]));
    assert!(
        listed.contains(id) && listed.contains("test phone"),
        "{listed}"
    );
    assert!(
        listed.contains("platform linux  client process-test"),
        "{listed}"
    );

    let revoked = harness.run(&["revoke", id]);
    assert!(revoked.status.success());
    assert_eq!(stdout(&revoked).trim(), format!("revoked: {id}"));
    assert_eq!(
        stdout(&harness.run(&["devices"])).trim(),
        "no paired devices"
    );

    let again = harness.run(&["revoke", id]);
    assert_eq!(again.status.code(), Some(EXIT_FAILURE));
    assert!(stderr(&again).contains("no such device"));
    let malformed = harness.run(&["revoke", "not-an-id"]);
    assert_eq!(malformed.status.code(), Some(EXIT_USAGE));
    assert!(stderr(&malformed).contains("not a device id"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_no_at_the_prompt_refuses_the_device() {
    let harness = Harness::new();
    let (_daemon, _) = harness.start_daemon();
    let (mut pairing, uri) = harness.start_pairing(&["pair", "--scopes", "state.read"]);
    let pending = tokio::time::timeout(PATIENCE, Connector::new(None).pair(&uri, "stranger", None))
        .await
        .expect("pairing in time")
        .expect("pending pairing");
    assert!(
        pairing
            .lines
            .expect("pending: ")
            .contains("stranger (unknown platform)")
    );
    pairing.lines.expect("code: ");

    answer(&mut pairing, "\n");

    assert!(pairing.lines.expect("refused").ends_with("[y/N] refused"));
    assert_eq!(pairing.exit_status().code(), Some(EXIT_FAILURE));
    let outcome = tokio::time::timeout(PATIENCE, pending.confirmed())
        .await
        .expect("answer in time");
    let Err(ClientError::Bye { code, .. }) = outcome else {
        panic!("the client must get `bye`");
    };
    assert_eq!(code, ByeCode::Revoked);
    assert_eq!(
        stdout(&harness.run(&["devices"])).trim(),
        "no paired devices"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_yes_typed_before_the_device_is_shown_does_not_confirm_it() {
    let harness = Harness::new();
    let (_daemon, _) = harness.start_daemon();
    let (mut pairing, uri) = harness.start_pairing(&["pair"]);
    // Typed ahead: it waits in the terminal when the first device presents the secret.
    answer(&mut pairing, "y\n");
    let pending = tokio::time::timeout(PATIENCE, Connector::new(None).pair(&uri, "stranger", None))
        .await
        .expect("pairing in time")
        .expect("pending pairing");
    pairing.lines.expect("code: ");

    answer(&mut pairing, "\n");

    assert!(pairing.lines.expect("refused").ends_with("[y/N] refused"));
    assert_eq!(pairing.exit_status().code(), Some(EXIT_FAILURE));
    drop(pending);
    assert_eq!(
        stdout(&harness.run(&["devices"])).trim(),
        "no paired devices"
    );
}

#[test]
fn the_subcommands_fail_clearly_without_a_daemon() {
    let dirs = TestDirs::new();
    let id = encode_base64url(&[9; 16]);
    for arguments in [vec!["devices"], vec!["pair"], vec!["revoke", &id]] {
        let output = dirs
            .command(DAEMON)
            .args(&arguments)
            .stdin(Stdio::null())
            .output()
            .expect("subcommand runs");
        assert_eq!(output.status.code(), Some(EXIT_FAILURE), "{arguments:?}");
        assert!(
            stderr(&output).contains("swayconnectd is not running"),
            "{arguments:?}"
        );
        assert_eq!(stdout(&output), "", "{arguments:?}");
    }
}

#[test]
fn the_fake_sway_is_kept_off_both_socket_variables_of_a_real_sway() {
    let named = support::sockets_named_by(|variable| Some(OsString::from(variable)));

    assert_eq!(named, [PathBuf::from("SWAYSOCK"), PathBuf::from("I3SOCK")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runtime_dir_too_long_for_a_socket_is_reported_once_with_the_path() {
    let harness = Harness::new();
    // A Unix socket address holds 108 bytes.
    let runtime_dir = harness.dirs.root().join("r".repeat(120));
    fs::create_dir(&runtime_dir).expect("runtime dir");
    fs::set_permissions(&runtime_dir, Permissions::from_mode(0o700)).expect("runtime dir mode");
    let socket = runtime_dir.join("swayconnect/admin.sock");
    let named_once = format!("swayconnectd: {}: ", socket.display());

    // `run` first: it creates the directory of the socket, which `devices` looks for.
    for subcommand in ["run", "devices"] {
        let mut command = harness.command(&[subcommand]);
        let output = command.env("XDG_RUNTIME_DIR", &runtime_dir).output();
        let output = output.expect("subcommand runs");

        assert_eq!(output.status.code(), Some(EXIT_USAGE), "{subcommand}");
        let errors = stderr(&output);
        let [line] = errors.lines().collect::<Vec<_>>()[..] else {
            panic!("{subcommand}: one line on stderr, got {errors:?}");
        };
        let cause = line.strip_prefix(&named_once);
        let cause = cause.unwrap_or_else(|| panic!("{subcommand}: no socket path in {line:?}"));
        assert!(
            !cause.is_empty() && !cause.contains(": "),
            "{subcommand}: the cause is printed once: {line:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_test_directories_are_removed_even_after_a_killed_daemon() {
    let harness = Harness::new();
    let root = harness.dirs.root().to_owned();
    let (daemon, _) = harness.start_daemon();
    let admin_socket = harness.dirs.runtime_dir().join("swayconnect/admin.sock");
    assert!(harness.dirs.data_dir().join("identity.cert.der").exists());

    // Dropping the process kills it, so it cannot remove its own socket.
    drop(daemon);
    assert!(admin_socket.exists());
    drop(harness);

    assert!(!root.exists(), "{} is left behind", root.display());
}
