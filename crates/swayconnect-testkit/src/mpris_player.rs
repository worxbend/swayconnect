//! Finds the `mpris-test-player` binary (`src/bin/mpris-test-player.rs`) for a test of
//! another package (`docs/M2C_PLAN.md` 4, slice R2).

use std::path::{Path, PathBuf};

/// The binary's name in the profile directory.
const PLAYER_NAME: &str = "mpris-test-player";

/// The player Cargo built into the profile directory, which holds the `deps` directory of
/// the running test binary. Cargo builds it only with the `real-backends` feature and when
/// this package's tests are built too, as `just real` does. Panics when it is missing.
pub fn mpris_test_player() -> PathBuf {
    let running = std::env::current_exe().expect("path of the running test binary");
    let profile_dir = running
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("no profile directory above {}", running.display()));
    let player = profile_dir.join(PLAYER_NAME);
    assert!(
        player.is_file(),
        "{} is missing; `just real` builds it (swayconnect-testkit, feature real-backends)",
        player.display()
    );
    player
}
