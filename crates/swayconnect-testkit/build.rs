//! Compiles `src/bin/fake-proc.rs` and places it at `<profile dir>/swayconnect-fake-proc`,
//! next to the `deps` directory that holds every test binary of the workspace.
//!
//! Cargo builds a package's binaries only for that package's own tests, so daemon and CLI
//! tests could not reach a plain bin target. The library finds this copy from the running
//! test's own path (`FakeProcs`), so library code reads no environment. A build script
//! gets its compiler and output directory only from the environment.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCES: [&str; 2] = ["src/bin/fake-proc.rs", "src/fake_proc_spec.rs"];
/// Must match `INSTALLED_NAME` in `src/fake_proc.rs`.
const INSTALLED_NAME: &str = "swayconnect-fake-proc";

fn main() {
    for source in SOURCES {
        println!("cargo::rerun-if-changed={source}");
    }
    let out_dir = PathBuf::from(required("OUT_DIR"));
    let built = out_dir.join("fake-proc");
    compile(&built);
    install(&built, &profile_dir(&out_dir));
}

fn compile(output: &Path) {
    let manifest_dir = PathBuf::from(required("CARGO_MANIFEST_DIR"));
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "bin"])
        .args(["--crate-name", "fake_proc"])
        .args(["--target", &required("TARGET")])
        .arg("-o")
        .arg(output)
        .arg(manifest_dir.join(SOURCES[0]))
        .status()
        .expect("start rustc for fake-proc");
    assert!(
        status.success(),
        "rustc failed to build fake-proc: {status}"
    );
}

/// `OUT_DIR` is `<profile dir>/build/<package>-<hash>/out`.
fn profile_dir(out_dir: &Path) -> PathBuf {
    let build_dir = out_dir.parent().and_then(Path::parent);
    match build_dir {
        Some(build_dir) if build_dir.file_name() == Some("build".as_ref()) => build_dir
            .parent()
            .expect("the build directory has a parent")
            .to_owned(),
        _ => panic!("unexpected OUT_DIR layout: {}", out_dir.display()),
    }
}

/// Copy, then rename: a test that runs the old file meanwhile keeps a whole binary.
fn install(built: &Path, profile_dir: &Path) {
    let target = profile_dir.join(INSTALLED_NAME);
    let partial = profile_dir.join(format!("{INSTALLED_NAME}.{}.partial", std::process::id()));
    fs::copy(built, &partial).expect("copy fake-proc next to the test binaries");
    fs::rename(&partial, &target).expect("move fake-proc into place");
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|error| panic!("cargo did not set {name}: {error}"))
}
