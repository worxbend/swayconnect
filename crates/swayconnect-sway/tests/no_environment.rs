//! No source file of this crate reads the process environment (`docs/DESIGN.md` 9.2).

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read a source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_source_file_reads_the_process_environment() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() >= 7, "the scan found only {files:?}");

    for file in files {
        let text = fs::read_to_string(&file).expect("read a source file");
        for needle in ["env::var", "env::vars", "var_os", "std::env"] {
            assert!(
                !text.contains(needle),
                "{} contains {needle}",
                file.display()
            );
        }
    }
}
