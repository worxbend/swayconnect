//! The sealed test session of `docs/M2C_PLAN.md` 3: a headless sway, a private D-Bus
//! session bus and a private PipeWire with WirePlumber, all on a private runtime directory.
//!
//! Every program starts with a cleared environment that holds only sealed values, and the
//! guard ([`LiveEnv`]) refuses any value that names the live desktop session.

mod audio;
mod child;
mod guard;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;
use tempfile::TempDir;

use crate::NO_SYSTEM_BUS;
use child::{Lines, Watcher, output_within, run_within};
pub use child::{leftovers_naming, live_processes_in_group, processes_naming};
pub use guard::LiveEnv;

/// The two null sinks every session has. WirePlumber makes one of them the default.
pub const SEALED_SINKS: [&str; 2] = ["sealed_sink_1", "sealed_sink_2"];
/// The one null source every session has, and the default source.
pub const SEALED_SOURCE: &str = "sealed_source";
/// The one output of the headless sway, 1280x720.
pub const HEADLESS_OUTPUT: &str = "HEADLESS-1";

const SWAY: &str = "/usr/bin/sway";
const SWAYMSG: &str = "/usr/bin/swaymsg";
const DBUS_DAEMON: &str = "/usr/bin/dbus-daemon";
const PIPEWIRE: &str = "/usr/bin/pipewire";
const WIREPLUMBER: &str = "/usr/bin/wireplumber";
const PACTL: &str = "/usr/bin/pactl";
const GRIM: &str = "/usr/bin/grim";
const SH: &str = "/bin/sh";

const SWAY_CONF: &str = include_str!("real_session/sway.conf");
const DBUS_CONF: &str = include_str!("real_session/dbus.conf");
const PIPEWIRE_CONF: &str = include_str!("real_session/sealed-pipewire.conf");

/// How long a service may take to signal that it is ready. Generous: the machine may be
/// busy with a build.
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long one `swaymsg`, `pactl` or `grim` call may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long one probe of `pactl subscribe` waits for the event of a new client.
const SUBSCRIPTION_PROBE: Duration = Duration::from_millis(500);
/// How long teardown waits for killed processes to leave the process table.
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// One sealed session. Dropping it, also while a test panics, kills its process group
/// and removes its directories; after a panic the logs stay in `<root>.log`.
///
/// Layout under one temp root in `/tmp` (short, so socket paths stay far below the
/// 108 bytes of a Unix socket address): `rt` (`XDG_RUNTIME_DIR`, mode 0700), `home`,
/// `config`, `cache`, `data`, `state`, `share` (the XDG directories, empty), `etc` (the
/// service configs) and `log` (stderr of every program the session starts).
#[derive(Debug)]
pub struct RealSession {
    /// Every process the session started, in start order. The first, the supervisor,
    /// leads the group.
    children: Vec<std::process::Child>,
    group: Option<Pid>,
    /// The only writer of the supervisor's stdin; it closes when the test process ends.
    supervisor: Option<ChildStdin>,
    env: BTreeMap<&'static str, OsString>,
    live: LiveEnv,
    /// Decision D1 is on: the guard allows the one link of `allow_user_systemd`.
    user_systemd: bool,
    root: TempDir,
}

impl RealSession {
    /// Starts the bus, PipeWire, WirePlumber with two null sinks and a null source, and
    /// sway. Panics when the guard trips or a service is not ready in time; what started
    /// is torn down.
    pub fn start(live: &LiveEnv) -> Self {
        let root = tempfile::Builder::new()
            .prefix("swayconnect-real.")
            .tempdir_in("/tmp")
            .expect("create the sealed session root under /tmp");
        let mut session = Self {
            children: Vec::new(),
            group: None,
            supervisor: None,
            env: BTreeMap::new(),
            live: live.clone(),
            user_systemd: false,
            root,
        };
        session.create_dirs();
        session.start_supervisor();
        session.start_bus();
        session.start_audio();
        session.start_sway();
        session
    }

    /// The private `XDG_RUNTIME_DIR`.
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.path().join("rt")
    }

    /// The whole environment a child of the session gets: `XDG_RUNTIME_DIR`, `HOME`, the
    /// XDG directories, `PATH`, `LANG`, `WAYLAND_DISPLAY` (a name in the runtime dir),
    /// `SWAYSOCK`, `DBUS_SESSION_BUS_ADDRESS`, `DBUS_SYSTEM_BUS_ADDRESS` (no system bus)
    /// and `PULSE_SERVER`.
    pub fn env(&self) -> impl Iterator<Item = (&'static str, &OsStr)> {
        self.env
            .iter()
            .map(|(name, value)| (*name, value.as_os_str()))
    }

    /// The process group that holds every process of the session.
    pub fn process_group(&self) -> u32 {
        let group = self.group.expect("a started session has a process group");
        group.as_raw_nonzero().get().unsigned_abs()
    }

    /// A command with a cleared environment plus [`env`](Self::env), in the session's
    /// process group. Start it with [`spawn`](Self::spawn) or [`output`](Self::output),
    /// which check the guard again.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .envs(&self.env)
            .process_group(self.group.map_or(0, |group| group.as_raw_nonzero().get()));
        command
    }

    /// Starts a program of the session (foot, fuzzel, ...) and returns its pid. stdin and
    /// stdout are null, stderr goes to the session log. The child gets only the variables
    /// set on `command`, never the test process's own; panics when one names the live
    /// session. The session kills and reaps it on drop.
    pub fn spawn(&mut self, mut command: Command) -> u32 {
        let log = self.log_file(&format!("child-{}", self.children.len()));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        self.seal(&mut command);
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start {command:?}: {error}"));
        let pid = child.id();
        self.children.push(child);
        pid
    }

    /// Runs a short command to its end, sealed as [`spawn`](Self::spawn) seals it, and
    /// returns its output whatever its exit status. Panics when the guard trips, or when it
    /// runs longer than 10 s (it is then killed).
    pub fn output(&self, mut command: Command) -> Output {
        self.seal(&mut command);
        output_within(command, COMMAND_TIMEOUT)
    }

    /// The environment of a program that the test starts outside the session's own
    /// helpers (the daemon's children: pactl, fuzzel, systemd-run), as given. Panics when a
    /// value names the live session.
    pub fn checked_env(
        &self,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        let vars: Vec<(OsString, OsString)> = vars.into_iter().collect();
        self.check(&vars);
        vars
    }

    /// Runs `swaymsg` on the sealed sway and returns its stdout.
    pub fn swaymsg(&self, args: &[&str]) -> String {
        self.run(SWAYMSG, args, "sway")
    }

    /// Runs `pactl` on the sealed sound server and returns its stdout.
    pub fn pactl(&self, args: &[&str]) -> String {
        self.run(PACTL, args, "pipewire")
    }

    /// Writes a PNG of the headless output to `png`. Panics when grim fails or writes
    /// nothing.
    pub fn screenshot(&self, png: &Path) {
        let mut command = self.command(GRIM);
        command.args(["-o", HEADLESS_OUTPUT]).arg(png);
        self.seal(&mut command);
        run_within(command, COMMAND_TIMEOUT);
        let size = fs::metadata(png).map_or(0, |metadata| metadata.len());
        assert!(size > 0, "grim wrote no image to {}", png.display());
    }

    /// Waits until sway shows a window with this `app_id`. Panics after a timeout.
    pub fn wait_for_window(&self, app_id: &str) {
        let mut command = self.command(SWAYMSG);
        command.args(["-t", "subscribe", "-m", r#"["window","tick"]"#]);
        self.seal(&mut command);
        let watcher = Watcher::spawn(command, Lines::read_json_values);
        // A tick subscription gets a first tick at once: from then on no window event is
        // missed, so a window absent from the tree below shows up as an event.
        watcher
            .lines
            .wait_for(READY_TIMEOUT, |event| {
                json_field(event, "first") == Some(true.into())
            })
            .expect("sway answers the window subscription");
        let tree: Value =
            serde_json::from_str(&self.swaymsg(&["-t", "get_tree"])).expect("sway tree is JSON");
        if has_app_id(&tree, app_id) {
            return;
        }
        let shown = watcher.lines.wait_for(READY_TIMEOUT, |event| {
            serde_json::from_str::<Value>(event)
                .is_ok_and(|event| event["container"]["app_id"] == app_id)
        });
        assert!(shown.is_some(), "no window with app_id {app_id} appeared");
    }

    /// Decision D1 (`docs/M2C_PLAN.md` 6), opt-in per test: lets `systemd-run --user` in
    /// this session reach the owner's real user manager and nothing else of the live
    /// session. `<rt>/systemd/private` becomes a symlink to the live manager's private
    /// socket, which `systemd-run --user --scope` uses, and the guard allows exactly that
    /// link. The session bus, the compositor and the sound server stay sealed. The test
    /// stops every unit it causes.
    pub fn allow_user_systemd(&mut self) {
        let link = self.runtime_dir().join(guard::D1_LINK);
        fs::create_dir(link.parent().expect("<rt>/systemd")).expect("create <rt>/systemd");
        std::os::unix::fs::symlink(self.live.runtime_dir().join(guard::D1_LINK), link)
            .expect("link the user manager's private socket");
        self.user_systemd = true;
    }

    fn create_dirs(&mut self) {
        let root = self.root.path().to_owned();
        let runtime_dir = self.runtime_dir();
        if let Err(trip) = guard::check_runtime_dir(&runtime_dir, &self.live) {
            panic!("sealed session guard: {trip}");
        }
        for name in [
            "rt", "home", "config", "cache", "data", "state", "share", "etc", "log",
        ] {
            fs::create_dir(root.join(name)).expect("create a sealed session directory");
        }
        fs::set_permissions(&runtime_dir, Permissions::from_mode(0o700))
            .expect("set the runtime dir mode");
        let sealed = |name: &str| root.join(name).into_os_string();
        self.env.extend([
            ("XDG_RUNTIME_DIR", runtime_dir.clone().into_os_string()),
            ("HOME", sealed("home")),
            ("XDG_CONFIG_HOME", sealed("config")),
            ("XDG_CACHE_HOME", sealed("cache")),
            ("XDG_DATA_HOME", sealed("data")),
            ("XDG_STATE_HOME", sealed("state")),
            ("XDG_DATA_DIRS", sealed("share")),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "C.UTF-8".into()),
            ("DBUS_SYSTEM_BUS_ADDRESS", NO_SYSTEM_BUS.into()),
            (
                "PULSE_SERVER",
                format!("unix:{}", runtime_dir.join("pulse/native").display()).into(),
            ),
        ]);
    }

    /// Starts the leader of the session's process group. It kills the group when the test
    /// process dies, also by SIGKILL, when Drop never runs: its stdin is a pipe whose only
    /// writer is this process (std opens it close-on-exec, so no other child holds it), so
    /// the kernel closes it with the test process, `cat` returns at end of file and
    /// `kill -KILL 0` kills the group. The daemon's children outside the group then lose
    /// sway, the sound server or the bus and end.
    fn start_supervisor(&mut self) {
        let mut command = self.command(SH);
        command
            .args(["-c", "/usr/bin/cat >/dev/null; kill -KILL 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(self.log_file("supervisor"));
        self.seal(&mut command);
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start the supervisor: {error}"));
        self.group = Some(Pid::from_child(&child));
        self.supervisor = child.stdin.take();
        self.children.push(child);
    }

    fn start_bus(&mut self) {
        let listen = format!("unix:path={}", self.runtime_dir().join("bus").display());
        let config = self.write_etc("dbus.conf", &DBUS_CONF.replace("@LISTEN@", &listen));
        let mut command = self.command(DBUS_DAEMON);
        command
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .arg(format!("--config-file={}", config.display()));
        let lines = self.spawn_service(command, "dbus-daemon");
        let address = lines.wait_for(READY_TIMEOUT, |line| line.starts_with("unix:"));
        let address = address.unwrap_or_else(|| self.not_ready("dbus-daemon"));
        self.env.insert("DBUS_SESSION_BUS_ADDRESS", address.into());
    }

    fn start_sway(&mut self) {
        let config = self.write_etc("sway.conf", SWAY_CONF);
        let mut command = self.command(SWAY);
        command
            .env("WLR_BACKENDS", "headless")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .arg("-c")
            .arg(config);
        let lines = self.spawn_service(command, "sway");
        let ready = lines.wait_for(READY_TIMEOUT, |line| line.starts_with("sway-ready "));
        let ready = ready.unwrap_or_else(|| self.not_ready("sway"));
        let mut words = ready.split_whitespace().skip(1);
        let (Some(display), Some(swaysock)) = (words.next(), words.next()) else {
            panic!("sway ready line without WAYLAND_DISPLAY and SWAYSOCK: {ready}");
        };
        assert!(
            !display.contains('/') && Path::new(swaysock).starts_with(self.runtime_dir()),
            "sway sockets outside the sealed runtime dir: {ready}"
        );
        self.env.insert("WAYLAND_DISPLAY", display.into());
        self.env.insert("SWAYSOCK", swaysock.into());
    }

    /// Starts a service with stdout read as [`Lines`] and stderr in the log.
    fn spawn_service(&mut self, mut command: Command, name: &str) -> Lines {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(self.log_file(name));
        self.seal(&mut command);
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("cannot start {name}: {error}"));
        let stdout = child.stdout.take().expect("piped stdout");
        self.children.push(child);
        Lines::read(stdout)
    }

    /// Runs `program` to its end and returns its stdout. On failure the panic shows the
    /// tail of the log of `service`, the program it talks to.
    fn run(&self, program: &str, args: &[&str], service: &str) -> String {
        let mut command = self.command(program);
        command.args(args);
        let output = self.output(command);
        assert!(
            output.status.success(),
            "{program} {args:?} failed with {}: {}\nthe {service} log ends:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
            self.log_tail(service)
        );
        String::from_utf8(output.stdout).expect("UTF-8 output")
    }

    /// Makes the variables set on `command` its whole environment, so nothing of the test
    /// process is inherited, then checks them and the links of the runtime dir.
    fn seal(&self, command: &mut Command) {
        let vars: Vec<(OsString, OsString)> = command
            .get_envs()
            .filter_map(|(name, value)| Some((name.to_owned(), value?.to_owned())))
            .collect();
        command.env_clear().envs(vars.iter().cloned());
        self.check(&vars);
    }

    fn check(&self, vars: &[(OsString, OsString)]) {
        let env = vars
            .iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()));
        let checked = guard::check_child_env(env, &self.live).and_then(|()| {
            guard::check_runtime_links(&self.runtime_dir(), &self.live, self.user_systemd)
        });
        if let Err(trip) = checked {
            panic!("sealed session guard: {trip}");
        }
    }

    fn write_etc(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.path().join("etc").join(name);
        fs::write(&path, text).expect("write a sealed session config");
        path
    }

    fn log_file(&self, name: &str) -> File {
        File::create(self.log_path(name)).expect("create a sealed session log")
    }

    fn log_path(&self, name: &str) -> PathBuf {
        self.root.path().join("log").join(format!("{name}.err"))
    }

    /// The last 20 lines of the log of `name`.
    fn log_tail(&self, name: &str) -> String {
        let log = fs::read_to_string(self.log_path(name)).unwrap_or_default();
        let tail: Vec<&str> = log.lines().rev().take(20).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        tail.join("\n")
    }

    fn not_ready(&self, name: &str) -> ! {
        panic!(
            "{name} of the sealed session was not ready within {READY_TIMEOUT:?}; its log:\n{}",
            self.log_tail(name)
        );
    }

    /// After a panic: moves the logs out of the root before it is removed, to
    /// `<root>.log`, so a flake of a real program can be read afterwards.
    fn keep_logs(&self) {
        let kept = kept_log_dir(&self.runtime_dir());
        match fs::rename(self.root.path().join("log"), &kept) {
            Ok(()) => eprintln!("sealed session logs kept in {}", kept.display()),
            Err(error) => eprintln!("cannot keep the sealed session logs: {error}"),
        }
    }
}

impl Drop for RealSession {
    fn drop(&mut self) {
        let Some(group) = self.group else { return };
        // The leader is never reaped before this point, so the group id is still ours.
        // An error means the group is already empty.
        let _ = kill_process_group(group, Signal::KILL);
        for child in &mut self.children {
            let _ = child.wait();
        }
        // Grandchildren (sway's `exec` shell) were killed too; wait until they are gone,
        // so no process of the session outlives the value.
        let group = group.as_raw_nonzero().get().unsigned_abs();
        let deadline = Instant::now() + TEARDOWN_TIMEOUT;
        while !live_processes_in_group(group).is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if thread::panicking() {
            self.keep_logs();
        }
    }
}

/// Where a session whose runtime dir is `runtime_dir` keeps its logs after a panic:
/// `<root>.log`, beside the removed root.
pub fn kept_log_dir(runtime_dir: &Path) -> PathBuf {
    let root = runtime_dir.parent().unwrap_or(runtime_dir);
    PathBuf::from(format!("{}.log", root.display()))
}

fn json_field(json: &str, field: &str) -> Option<Value> {
    serde_json::from_str::<Value>(json)
        .ok()?
        .get(field)
        .cloned()
}

/// Whether the sway tree holds a window with this `app_id`, tiled or floating.
fn has_app_id(node: &Value, app_id: &str) -> bool {
    node["app_id"] == app_id
        || ["nodes", "floating_nodes"].iter().any(|key| {
            node[*key]
                .as_array()
                .is_some_and(|children| children.iter().any(|child| has_app_id(child, app_id)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_found_in_tiled_and_floating_nodes() {
        let tree: Value = serde_json::json!({
            "nodes": [{"nodes": [{"app_id": "foot"}]}],
            "floating_nodes": [],
        });
        let floating: Value = serde_json::json!({
            "nodes": [{"floating_nodes": [{"app_id": "fuzzel"}]}],
        });

        assert!(has_app_id(&tree, "foot"));
        assert!(!has_app_id(&tree, "fuzzel"));
        assert!(has_app_id(&floating, "fuzzel"));
    }
}
