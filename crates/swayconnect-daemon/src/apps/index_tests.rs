use std::fs;
use std::path::{Path, PathBuf};

use super::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/desktop-entries"
);
const PATH_TOOL: &str = "swayconnect-fixture-tool";

fn fixture_dirs(exec_path: Vec<PathBuf>) -> IndexDirs {
    IndexDirs {
        data_dirs: vec![
            Path::new(FIXTURES).join("high"),
            Path::new(FIXTURES).join("low"),
        ],
        exec_path,
    }
}

fn test_config() -> AppsConfig {
    AppsConfig {
        deny: vec!["denied".to_owned()],
        risky: vec!["gparted".to_owned()],
        ..AppsConfig::default()
    }
}

fn write_file(path: &Path, text: &str, mode: u32) {
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(path, text).expect("write");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

fn ids(apps: &[IndexedApp]) -> Vec<&str> {
    apps.iter().map(|app| app.entry_id.as_str()).collect()
}

fn find<'a>(apps: &'a [IndexedApp], id: &str) -> &'a IndexedApp {
    apps.iter()
        .find(|app| app.entry_id == id)
        .unwrap_or_else(|| panic!("{id} not listed"))
}

/// A temp data dir with one entry, scanned alone.
fn scan_one(entry: &str, exec_path: Vec<PathBuf>) -> Vec<IndexedApp> {
    let dir = tempfile::tempdir().expect("temp dir");
    write_file(&dir.path().join("applications/one.desktop"), entry, 0o644);
    let dirs = IndexDirs {
        data_dirs: vec![dir.path().to_owned()],
        exec_path,
    };
    scan(&dirs, &AppsConfig::default())
}

#[test]
fn filters_hide_what_the_design_lists_and_sorting_ignores_case() {
    let bin = tempfile::tempdir().expect("temp dir");
    write_file(&bin.path().join(PATH_TOOL), "", 0o755);
    let apps = scan(&fixture_dirs(vec![bin.path().to_owned()]), &test_config());
    assert_eq!(
        ids(&apps),
        [
            "firefox",
            "gparted",
            "htop",
            "mpv",
            "only-sway",
            "quoted",
            "sub-tool",
            "tryexec-path",
        ]
    );
}

#[test]
fn a_relative_try_exec_needs_the_program_on_the_path() {
    let apps = scan(&fixture_dirs(Vec::new()), &test_config());
    assert!(!ids(&apps).contains(&"tryexec-path"));
    let not_executable = tempfile::tempdir().expect("temp dir");
    write_file(&not_executable.path().join(PATH_TOOL), "", 0o644);
    let apps = scan(
        &fixture_dirs(vec![not_executable.path().to_owned()]),
        &test_config(),
    );
    assert!(!ids(&apps).contains(&"tryexec-path"));
}

#[test]
fn entry_fields_come_from_the_first_data_dir_unlocalised() {
    let apps = scan(&fixture_dirs(Vec::new()), &test_config());
    let firefox = find(&apps, "firefox");
    assert_eq!(
        firefox,
        &IndexedApp {
            entry_id: "firefox".to_owned(),
            name: "Firefox".to_owned(),
            generic_name: Some("Web Browser".to_owned()),
            keywords: vec!["internet".into(), "web".into(), "browser".into()],
            has_icon: true,
            terminal: false,
            risky: false,
            argv: vec!["/usr/lib/firefox/firefox".to_owned()],
        }
    );
    assert_eq!(
        find(&apps, "mpv").argv,
        ["mpv", "--player-operation-mode=pseudo-gui", "--"]
    );
    assert_eq!(
        find(&apps, "quoted").argv,
        [
            "/opt/My Tools/run",
            "--title",
            r#"a "b" c"#,
            "--file=",
            "%done"
        ]
    );
}

#[test]
fn terminals_are_always_risky_and_the_owner_marks_others() {
    let apps = scan(&fixture_dirs(Vec::new()), &test_config());
    let htop = find(&apps, "htop");
    assert!(htop.terminal && htop.risky && !htop.has_icon);
    let gparted = find(&apps, "gparted");
    assert!(!gparted.terminal && gparted.risky);
    let unmarked = scan(
        &fixture_dirs(Vec::new()),
        &AppsConfig {
            risky: Vec::new(),
            ..test_config()
        },
    );
    assert!(!find(&unmarked, "gparted").risky);
    assert!(find(&unmarked, "htop").risky);
}

#[test]
fn a_terminal_emulator_is_risky_without_the_terminal_key() {
    let apps = scan_one(
        "[Desktop Entry]\nType=Application\nName=Alacritty\nExec=alacritty\n\
         Terminal=false\nCategories=System;TerminalEmulator;\n",
        Vec::new(),
    );
    let app = find(&apps, "one");
    assert!(!app.terminal && app.risky);
}

#[test]
fn text_is_cleaned_then_cut() {
    let long_generic = "g".repeat(200);
    let keywords: Vec<String> = (0..20).map(|i| format!("k{i}")).collect();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Evil\u{1b}[2J\u{202e}name\\n\n\
         GenericName={long_generic}\nKeywords={}\nExec=evil\n",
        keywords.join(";")
    );
    let apps = scan_one(&entry, Vec::new());
    assert_eq!(apps[0].name, "Evil\u{fffd}[2J\u{fffd}name\u{fffd}");
    assert_eq!(apps[0].generic_name.as_deref(), Some(&long_generic[..128]));
    assert_eq!(apps[0].keywords, keywords[..16]);
}

#[test]
fn keywords_keep_to_64_scalars_each_and_512_bytes_together() {
    let long = "ж".repeat(70);
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=K\nExec=k\nKeywords={};{long};{long};{long};{long}\n",
        "x".repeat(3)
    );
    let apps = scan_one(&entry, Vec::new());
    let keywords = &apps[0].keywords;
    // "xxx" (3 bytes) plus three keywords of 64 x 2 bytes is 387; a fourth would be 515.
    assert_eq!(keywords.len(), 4);
    assert!(keywords[1..].iter().all(|k| k.chars().count() == 64));
    assert!(keywords.iter().map(String::len).sum::<usize>() <= 512);
}

#[test]
fn an_absolute_try_exec_must_be_an_executable_file() {
    let bin = tempfile::tempdir().expect("temp dir");
    let tool = bin.path().join("tool");
    let entry = |path: &Path| {
        format!(
            "[Desktop Entry]\nType=Application\nName=T\nTryExec={}\nExec=t\n",
            path.display()
        )
    };
    write_file(&tool, "", 0o755);
    assert_eq!(scan_one(&entry(&tool), Vec::new()).len(), 1);
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(scan_one(&entry(&tool), Vec::new()).is_empty());
    assert!(scan_one(&entry(bin.path()), Vec::new()).is_empty());
}

#[test]
fn an_oversized_entry_file_is_skipped() {
    let padding = format!("# {}\n", "x".repeat(MAX_ENTRY_BYTES as usize));
    let entry = format!("[Desktop Entry]\nType=Application\nName=Big\nExec=big\n{padding}");
    assert!(scan_one(&entry, Vec::new()).is_empty());
}
