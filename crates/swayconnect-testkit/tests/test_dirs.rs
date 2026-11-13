//! `TestDirs` layout, the cleared child environment, and the crate's own isolation rules.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use swayconnect_testkit::TestDirs;

#[test]
fn every_path_is_under_the_one_temp_root() {
    let dirs = TestDirs::new();

    for path in [
        dirs.config_home(),
        dirs.data_home(),
        dirs.runtime_dir(),
        dirs.config_file(),
        dirs.data_dir(),
        dirs.sway_socket(),
    ] {
        assert!(path.starts_with(dirs.root()), "{}", path.display());
    }
    assert!(dirs.config_file().parent().expect("parent").is_dir());
    assert!(dirs.data_home().is_dir());
    assert!(!dirs.data_dir().exists());
    assert_eq!(
        dirs.sway_socket().parent(),
        Some(dirs.runtime_dir().as_path())
    );
}

#[test]
fn the_runtime_dir_has_mode_0700() {
    let dirs = TestDirs::new();

    let mode = dirs
        .runtime_dir()
        .metadata()
        .expect("metadata")
        .permissions()
        .mode();

    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn the_socket_path_fits_a_unix_socket_address() {
    assert!(TestDirs::new().sway_socket().as_os_str().len() < 108);
}

#[test]
fn the_root_is_removed_on_drop() {
    let dirs = TestDirs::new();
    let root = dirs.root().to_owned();

    drop(dirs);

    assert!(!root.exists());
}

#[test]
fn a_child_command_has_only_the_three_xdg_variables() {
    let dirs = TestDirs::new();

    let command = dirs.command("/bin/true");
    let mut names: Vec<&OsStr> = command.get_envs().map(|(name, _)| name).collect();
    names.sort();

    assert_eq!(
        names,
        ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"]
    );
}

#[test]
fn a_child_process_sees_a_cleared_environment() {
    let dirs = TestDirs::new();

    let output = dirs.command("/usr/bin/env").output().expect("run env");

    let mut lines: Vec<&str> = std::str::from_utf8(&output.stdout)
        .expect("UTF-8")
        .lines()
        .collect();
    lines.sort();
    assert_eq!(
        lines,
        [
            format!("XDG_CONFIG_HOME={}", dirs.config_home().display()),
            format!("XDG_DATA_HOME={}", dirs.data_home().display()),
            format!("XDG_RUNTIME_DIR={}", dirs.runtime_dir().display()),
        ]
    );
}

fn rust_sources(dir: &Path) -> Vec<String> {
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension() == Some(OsStr::new("rs")) {
            sources.push(std::fs::read_to_string(&path).expect("read source"));
        }
    }
    sources
}

#[test]
fn the_crate_reads_no_environment_variable() {
    let sources = rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));

    assert!(!sources.is_empty());
    for source in sources {
        // `env::var` also matches `var_os` and `vars`; `temp_dir` reads TMPDIR.
        for needle in ["env::var", "env!(", "option_env!", "env::temp_dir"] {
            assert!(!source.contains(needle), "src uses {needle}");
        }
    }
}

#[test]
fn the_crate_depends_on_neither_daemon_nor_protocol() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read manifest");

    assert!(!manifest.contains("swayconnect-daemon"));
    assert!(!manifest.contains("swayconnect-protocol"));
}
