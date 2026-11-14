//! The guard of `docs/M2C_PLAN.md` 3 item 5: a sealed session never shares the runtime
//! directory, compositor, sway IPC, session bus or sound server of the live desktop.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rustix::process::getuid;

/// Variables of the test process that name the live session.
const SESSION_VARIABLES: [&str; 6] = [
    "XDG_RUNTIME_DIR",
    "WAYLAND_DISPLAY",
    "SWAYSOCK",
    "I3SOCK",
    "DBUS_SESSION_BUS_ADDRESS",
    "PULSE_SERVER",
];

/// Sockets a program finds in its runtime directory when no variable names them.
const IMPLIED_SOCKETS: [&str; 4] = ["bus", "pulse/native", "pipewire-0", "pipewire-0-manager"];

/// Decision D1 (`docs/M2C_PLAN.md` 6): the one link from the sealed runtime dir into the
/// live one, to the user manager's private socket.
pub(crate) const D1_LINK: &str = "systemd/private";

/// What the live desktop session is, as the environment of the test process says.
///
/// Library code reads no environment (`AGENTS.md` 3.5), so a test builds this with
/// [`from_vars`](Self::from_vars) from the variables of its own process.
#[derive(Debug, Clone)]
pub struct LiveEnv {
    /// `XDG_RUNTIME_DIR` of the test process, else `/run/user/<uid>`.
    runtime_dir: PathBuf,
    /// Text that no child value may contain: live socket paths, bus addresses, and the
    /// live runtime directory followed by `/`.
    markers: Vec<String>,
}

impl LiveEnv {
    /// Keeps the session variables of `vars` and ignores every other one. Without
    /// `XDG_RUNTIME_DIR` the live runtime dir is still `/run/user/<uid>`, and that dir is
    /// refused whatever `XDG_RUNTIME_DIR` says.
    pub fn from_vars<K, V>(vars: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        Self::of_user(vars, getuid().as_raw())
    }

    fn of_user<K, V>(vars: impl IntoIterator<Item = (K, V)>, uid: u32) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let vars: HashMap<String, OsString> = vars
            .into_iter()
            .filter_map(|(name, value)| {
                let name = name.as_ref().to_str()?;
                SESSION_VARIABLES
                    .contains(&name)
                    .then(|| (name.to_owned(), value.as_ref().to_owned()))
            })
            .filter(|(_, value)| !value.is_empty())
            .collect();
        let user_run_dir = PathBuf::from(format!("/run/user/{uid}"));
        let runtime_dir = vars
            .get("XDG_RUNTIME_DIR")
            .map_or_else(|| user_run_dir.clone(), PathBuf::from);
        let mut markers = vec![
            format!("{}/", runtime_dir.display()),
            format!("{}/", user_run_dir.display()),
        ];
        markers.extend(IMPLIED_SOCKETS.map(|name| text(&runtime_dir.join(name))));
        if let Some(display) = vars.get("WAYLAND_DISPLAY") {
            markers.extend(wayland_socket(display, Some(&runtime_dir)).map(|socket| text(&socket)));
        }
        for name in ["SWAYSOCK", "I3SOCK", "DBUS_SESSION_BUS_ADDRESS"] {
            markers.extend(
                vars.get(name)
                    .map(|value| value.to_string_lossy().into_owned()),
            );
        }
        if let Some(address) = vars.get("DBUS_SESSION_BUS_ADDRESS") {
            markers.extend(unix_paths(&address.to_string_lossy()));
        }
        if let Some(server) = vars.get("PULSE_SERVER") {
            markers.extend(pulse_paths(&server.to_string_lossy()));
        }
        Self {
            runtime_dir,
            markers,
        }
    }

    pub(crate) fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    /// Whether `path`, with every symlink resolved, is the live runtime dir, lies in it, or
    /// is a live socket.
    fn resolves_into(&self, path: &Path) -> bool {
        let resolved = canonical(path);
        resolved.starts_with(canonical(&self.runtime_dir))
            || self.markers.contains(&text(&resolved))
    }
}

/// Refuses a sealed runtime directory that is, holds or lies inside the live one.
pub(crate) fn check_runtime_dir(sealed: &Path, live: &LiveEnv) -> Result<(), String> {
    let sealed = canonical(sealed);
    let live_dir = canonical(live.runtime_dir());
    if sealed.starts_with(&live_dir) || live_dir.starts_with(&sealed) {
        return Err(format!(
            "sealed runtime dir {} overlaps the live runtime dir {}",
            sealed.display(),
            live_dir.display()
        ));
    }
    Ok(())
}

/// Refuses a symlink in the sealed runtime dir, or one level below it, that resolves into
/// the live runtime dir (`<rt>/wayland-1 -> /run/user/1000/wayland-1`). The one exception
/// is decision D1, once `user_systemd` is set: `<rt>/systemd/private` to the live user
/// manager's `systemd/private`.
pub(crate) fn check_runtime_links(
    sealed: &Path,
    live: &LiveEnv,
    user_systemd: bool,
) -> Result<(), String> {
    let d1_link = sealed.join(D1_LINK);
    let d1_target = canonical(&live.runtime_dir().join(D1_LINK));
    for link in symlinks_within(sealed, 2) {
        if !live.resolves_into(&link_target(&link)) {
            continue;
        }
        if user_systemd && link == d1_link && canonical(&link) == d1_target {
            continue;
        }
        return Err(format!(
            "{} links into the live session ({})",
            link.display(),
            link_target(&link).display()
        ));
    }
    Ok(())
}

/// Refuses a child environment that names any part of the live session, also through a
/// symlink.
pub(crate) fn check_child_env<'a>(
    env: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>,
    live: &LiveEnv,
) -> Result<(), String> {
    let env: Vec<(&OsStr, &OsStr)> = env.into_iter().collect();
    let child_runtime_dir = env
        .iter()
        .find(|(name, _)| *name == "XDG_RUNTIME_DIR")
        .map(|(_, value)| Path::new(value));
    for (name, value) in &env {
        let value_text = value.to_string_lossy();
        let name = name.to_string_lossy();
        if let Some(marker) = live
            .markers
            .iter()
            .find(|marker| value_text.contains(*marker))
        {
            return Err(format!(
                "{name}={value_text} names the live session ({marker})"
            ));
        }
        if let Some(path) = named_paths(value).find(|path| live.resolves_into(path)) {
            return Err(format!(
                "{name}={value_text} resolves into the live session ({})",
                canonical(&path).display()
            ));
        }
        if name == "WAYLAND_DISPLAY" {
            let socket = wayland_socket(value, child_runtime_dir);
            if let Some(socket) = socket.filter(|socket| live.resolves_into(socket)) {
                return Err(format!(
                    "WAYLAND_DISPLAY resolves to the live {}",
                    canonical(&socket).display()
                ));
            }
        }
    }
    Ok(())
}

/// The absolute paths a value names: itself, the `path=` of a D-Bus address, or the
/// sockets of a PulseAudio server list.
fn named_paths(value: &OsStr) -> impl Iterator<Item = PathBuf> {
    let text = value.to_string_lossy();
    let mut paths: Vec<PathBuf> = unix_paths(&text)
        .into_iter()
        .chain(pulse_paths(&text))
        .map(PathBuf::from)
        .collect();
    if Path::new(value).is_absolute() {
        paths.push(PathBuf::from(value));
    }
    paths.into_iter()
}

/// The symlinks in `dir` and in its subdirectories down to `depth` levels. Entries that
/// vanish meanwhile are skipped: the session owns the dir, so nothing hides there.
fn symlinks_within(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            links.push(entry.path());
        } else if kind.is_dir() && depth > 1 {
            links.extend(symlinks_within(&entry.path(), depth - 1));
        }
    }
    links
}

/// Where a symlink points: resolved when its target exists, else its text joined to the
/// link's directory.
fn link_target(link: &Path) -> PathBuf {
    if let Ok(resolved) = link.canonicalize() {
        return resolved;
    }
    let target = fs::read_link(link).unwrap_or_default();
    link.parent()
        .map_or(target.clone(), |dir| dir.join(&target))
}

/// libwayland's rule: an absolute name is the socket, else it lies in the runtime dir.
fn wayland_socket(display: &OsStr, runtime_dir: Option<&Path>) -> Option<PathBuf> {
    let display = Path::new(display);
    if display.is_absolute() {
        Some(display.to_owned())
    } else {
        runtime_dir.map(|dir| dir.join(display))
    }
}

/// The `path=` values of the `unix:` entries of a D-Bus address list.
fn unix_paths(address: &str) -> Vec<String> {
    address
        .split(';')
        .filter_map(|entry| entry.strip_prefix("unix:"))
        .flat_map(|keys| keys.split(','))
        .filter_map(|key| key.strip_prefix("path="))
        .map(str::to_owned)
        .collect()
}

/// The socket paths in a PulseAudio server list (`unix:/path`, `/path`).
fn pulse_paths(server: &str) -> Vec<String> {
    server
        .split_whitespace()
        .map(|entry| entry.strip_prefix("unix:").unwrap_or(entry))
        .filter(|entry| entry.starts_with('/'))
        .map(str::to_owned)
        .collect()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE_SWAYSOCK: &str = "/run/user/1000/sway-ipc.1000.1148.sock";

    fn live() -> LiveEnv {
        LiveEnv::from_vars([
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("SWAYSOCK", LIVE_SWAYSOCK),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("HOME", "/home/owner"),
        ])
    }

    fn check(env: &[(&str, &str)]) -> Result<(), String> {
        check_child_env(
            env.iter()
                .map(|(name, value)| (OsStr::new(name), OsStr::new(value))),
            &live(),
        )
    }

    fn sealed_env() -> Vec<(&'static str, &'static str)> {
        vec![
            ("XDG_RUNTIME_DIR", "/tmp/swayconnect-real.x/rt"),
            ("WAYLAND_DISPLAY", "wayland-1"),
            (
                "SWAYSOCK",
                "/tmp/swayconnect-real.x/rt/sway-ipc.1000.7.sock",
            ),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/tmp/swayconnect-real.x/rt/bus",
            ),
            (
                "PULSE_SERVER",
                "unix:/tmp/swayconnect-real.x/rt/pulse/native",
            ),
            ("PATH", "/usr/bin:/bin"),
        ]
    }

    #[test]
    fn a_sealed_env_passes_even_with_the_same_wayland_display_name() {
        assert_eq!(check(&sealed_env()), Ok(()));
    }

    #[test]
    fn a_leaked_swaysock_trips_the_guard() {
        let mut env = sealed_env();
        env.push(("SWAYSOCK", LIVE_SWAYSOCK));

        let refused = check(&env).expect_err("leaked SWAYSOCK");

        assert!(refused.starts_with("SWAYSOCK="), "{refused}");
    }

    #[test]
    fn a_leaked_swaysock_outside_the_runtime_dir_trips_the_guard() {
        let live = LiveEnv::from_vars([("SWAYSOCK", "/var/tmp/sway.sock")]);

        let refused = check_child_env(
            [(OsStr::new("I3SOCK"), OsStr::new("/var/tmp/sway.sock"))],
            &live,
        );

        assert!(refused.is_err());
    }

    #[test]
    fn the_live_runtime_dir_as_child_runtime_dir_trips_the_guard() {
        let mut env = sealed_env();
        env[0] = ("XDG_RUNTIME_DIR", "/run/user/1000");

        assert!(check(&env).is_err());
    }

    #[test]
    fn a_relative_wayland_display_that_resolves_to_the_live_socket_trips_the_guard() {
        // No text of the child env contains the live socket path; only resolving does.
        let live = LiveEnv::from_vars([("WAYLAND_DISPLAY", "/var/w/wayland-9")]);
        let env = [
            (OsStr::new("WAYLAND_DISPLAY"), OsStr::new("wayland-9")),
            (OsStr::new("XDG_RUNTIME_DIR"), OsStr::new("/var/w")),
        ];

        assert!(check_child_env(env, &live).is_err());
    }

    #[test]
    fn the_live_bus_and_sound_server_trip_the_guard() {
        for leak in [
            (
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/run/user/1000/bus,guid=1",
            ),
            ("PULSE_SERVER", "unix:/run/user/1000/pulse/native"),
            ("PIPEWIRE_REMOTE", "/run/user/1000/pipewire-0"),
        ] {
            let mut env = sealed_env();
            env.push(leak);
            assert!(check(&env).is_err(), "{leak:?}");
        }
    }

    #[test]
    fn a_bus_or_sound_server_outside_the_runtime_dir_is_still_live() {
        let live = LiveEnv::from_vars([
            (
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/var/bus;unix:abstract=x",
            ),
            ("PULSE_SERVER", "tcp:host unix:/var/pulse"),
        ]);

        for value in ["unix:path=/var/bus", "/var/pulse"] {
            let env = [(OsStr::new("X"), OsStr::new(value))];
            assert!(check_child_env(env, &live).is_err(), "{value}");
        }
    }

    #[test]
    fn a_runtime_dir_that_overlaps_the_live_one_is_refused() {
        let live = live();

        assert!(check_runtime_dir(Path::new("/run/user/1000"), &live).is_err());
        assert!(check_runtime_dir(Path::new("/run/user/1000/sealed"), &live).is_err());
        assert!(check_runtime_dir(Path::new("/run/user"), &live).is_err());
        assert_eq!(
            check_runtime_dir(Path::new("/tmp/sealed/rt"), &live),
            Ok(())
        );
    }

    #[test]
    fn without_xdg_runtime_dir_the_users_run_dir_is_still_live() {
        let live = LiveEnv::of_user(std::iter::empty::<(&str, &str)>(), 1000);

        assert_eq!(check_runtime_dir(Path::new("/tmp/x"), &live), Ok(()));
        assert!(check_runtime_dir(Path::new("/run/user/1000/x"), &live).is_err());
        let env = |value| [(OsStr::new("A"), OsStr::new(value))];
        assert!(check_child_env(env("b"), &live).is_ok());
        assert!(check_child_env(env("/run/user/1000/wayland-1"), &live).is_err());
        assert!(check_child_env(env("unix:path=/run/user/1000/bus"), &live).is_err());
    }

    #[test]
    fn the_users_run_dir_is_live_even_when_xdg_runtime_dir_names_another() {
        let live = LiveEnv::of_user([("XDG_RUNTIME_DIR", "/var/rt")], 1000);

        let env = [(OsStr::new("A"), OsStr::new("/run/user/1000/pulse/native"))];
        assert!(check_child_env(env, &live).is_err());
    }

    /// A temp "live" runtime dir with a socket file, and a sealed root beside it.
    struct Linked {
        live_dir: tempfile::TempDir,
        sealed: tempfile::TempDir,
    }

    impl Linked {
        fn new() -> Linked {
            let live_dir = tempfile::tempdir().expect("live dir");
            fs::create_dir(live_dir.path().join("systemd")).expect("systemd dir");
            for socket in ["wayland-1", "systemd/private"] {
                fs::write(live_dir.path().join(socket), "").expect("socket stand-in");
            }
            let sealed = tempfile::tempdir().expect("sealed dir");
            Linked { live_dir, sealed }
        }

        fn live(&self) -> LiveEnv {
            LiveEnv::of_user([("XDG_RUNTIME_DIR", self.live_dir.path())], 1000)
        }

        fn link(&self, target: &str, link: &str) {
            let link = self.sealed.path().join(link);
            fs::create_dir_all(link.parent().expect("parent")).expect("link dir");
            std::os::unix::fs::symlink(self.live_dir.path().join(target), link).expect("link");
        }
    }

    #[test]
    fn a_runtime_dir_that_is_a_symlink_to_the_live_one_trips_the_guard() {
        let linked = Linked::new();
        let rt = linked.sealed.path().join("rt");
        std::os::unix::fs::symlink(linked.live_dir.path(), &rt).expect("link");
        let env = [(OsStr::new("XDG_RUNTIME_DIR"), rt.as_os_str())];

        assert!(check_runtime_dir(&rt, &linked.live()).is_err());
        assert!(check_child_env(env, &linked.live()).is_err());
    }

    #[test]
    fn a_wayland_display_that_links_to_the_live_socket_trips_the_guard() {
        let linked = Linked::new();
        linked.link("wayland-1", "wayland-9");
        let env = [
            (
                OsStr::new("XDG_RUNTIME_DIR"),
                linked.sealed.path().as_os_str(),
            ),
            (OsStr::new("WAYLAND_DISPLAY"), OsStr::new("wayland-9")),
        ];

        let refused = check_child_env(env, &linked.live()).expect_err("linked socket");

        assert!(refused.starts_with("WAYLAND_DISPLAY resolves"), "{refused}");
    }

    #[test]
    fn a_link_into_the_live_runtime_dir_is_refused_except_the_d1_link() {
        let linked = Linked::new();
        let live = linked.live();
        let sealed = linked.sealed.path();
        assert_eq!(check_runtime_links(sealed, &live, false), Ok(()));

        linked.link("systemd/private", D1_LINK);
        assert!(
            check_runtime_links(sealed, &live, false).is_err(),
            "D1 not allowed yet"
        );
        assert_eq!(check_runtime_links(sealed, &live, true), Ok(()));

        linked.link("wayland-1", "nested/wayland-1");
        let refused = check_runtime_links(sealed, &live, true).expect_err("second link");
        assert!(refused.contains("nested/wayland-1"), "{refused}");
    }

    #[test]
    fn the_d1_name_pointing_elsewhere_in_the_live_dir_is_refused() {
        let linked = Linked::new();
        linked.link("wayland-1", D1_LINK);

        assert!(check_runtime_links(linked.sealed.path(), &linked.live(), true).is_err());
    }
}
