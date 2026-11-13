use std::fs;
use std::os::unix::fs::symlink;

use tempfile::TempDir;

use super::*;

const HICOLOR: &str = "[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps,scalable/apps\n\n\
    [48x48/apps]\nSize=48\nType=Fixed\n\n\
    [scalable/apps]\nSize=128\nType=Scalable\nMinSize=16\nMaxSize=512\n";
const CUSTOM: &str = "[Icon Theme]\nName=Custom\nInherits=Parent\nDirectories=128x128/apps\n\n\
    [128x128/apps]\nSize=128\nType=Fixed\n";
const PARENT: &str = "[Icon Theme]\nName=Parent\nInherits=Custom,hicolor\n\
    Directories=128x128/apps,../escape\n\n[128x128/apps]\nSize=128\n\n[../escape]\nSize=128\n";

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, b"icon").expect("write");
}

fn share() -> TempDir {
    let share = TempDir::new().expect("tempdir");
    let root = share.path();
    for (theme, index) in [("hicolor", HICOLOR), ("Custom", CUSTOM), ("Parent", PARENT)] {
        let file = root.join("icons").join(theme).join("index.theme");
        touch(&file);
        fs::write(file, index).expect("write index");
    }
    for file in [
        "icons/Custom/128x128/apps/both.png",
        "icons/Parent/128x128/apps/both.png",
        "icons/Parent/128x128/apps/parent.png",
        "icons/hicolor/48x48/apps/small.png",
        "icons/hicolor/48x48/apps/twice.png",
        "icons/hicolor/scalable/apps/twice.svg",
        "icons/hicolor/scalable/apps/vector.svg",
        "icons/escape.png",
        "pixmaps/pix.png",
    ] {
        touch(&root.join(file));
    }
    share
}

fn found(share: &TempDir, icon: &str, theme: &str) -> Option<String> {
    let dirs = [share.path().to_path_buf()];
    let file = resolve(icon, theme, &dirs)?;
    Some(file.strip_prefix(share.path()).ok()?.display().to_string())
}

#[test]
fn the_theme_then_inherits_then_hicolor_then_pixmaps() {
    let share = share();
    let found = |icon: &str| found(&share, icon, "Custom");
    assert_eq!(
        found("both").as_deref(),
        Some("icons/Custom/128x128/apps/both.png")
    );
    assert_eq!(
        found("parent").as_deref(),
        Some("icons/Parent/128x128/apps/parent.png")
    );
    assert_eq!(
        found("vector").as_deref(),
        Some("icons/hicolor/scalable/apps/vector.svg")
    );
    assert_eq!(found("pix").as_deref(), Some("pixmaps/pix.png"));
    assert_eq!(found("pix.png").as_deref(), Some("pixmaps/pix.png"));
    assert_eq!(found("missing"), None);
}

#[test]
fn an_exact_size_match_beats_the_closest_directory() {
    let share = share();
    assert_eq!(
        found(&share, "twice", FALLBACK_THEME).as_deref(),
        Some("icons/hicolor/scalable/apps/twice.svg")
    );
    assert_eq!(
        found(&share, "small", FALLBACK_THEME).as_deref(),
        Some("icons/hicolor/48x48/apps/small.png")
    );
}

#[test]
fn names_and_directories_cannot_leave_the_theme() {
    let share = share();
    for icon in ["../escape", "apps/both", ".hidden", ""] {
        assert_eq!(found(&share, icon, "Custom"), None, "{icon}");
    }
    assert_eq!(found(&share, "escape", "Parent"), None);
    assert_eq!(found(&share, "both", "../icons"), None);
}

#[test]
fn absolute_paths_must_stay_under_the_roots_after_canonicalising() {
    let share = share();
    let outside = TempDir::new().expect("tempdir");
    let secret = outside.path().join("secret.png");
    touch(&secret);
    let root = share.path();
    symlink(&secret, root.join("pixmaps/link.png")).expect("symlink");
    touch(&root.join("pixmaps/notes.txt"));
    let dirs = [root.to_path_buf()];

    let inside = root.join("icons/hicolor/48x48/apps/small.png");
    let canonical = fs::canonicalize(&inside).expect("canonical");
    assert_eq!(
        resolve(inside.to_str().expect("utf-8"), "x", &dirs),
        Some(canonical)
    );
    for refused in [
        root.join("pixmaps/link.png"),
        root.join("pixmaps/../..")
            .join(outside.path().file_name().expect("name"))
            .join("secret.png"),
        secret.clone(),
        root.join("pixmaps/notes.txt"),
        root.join("pixmaps/absent.png"),
    ] {
        assert_eq!(
            allowed_absolute(&refused, &dirs),
            None,
            "{}",
            refused.display()
        );
    }
}
