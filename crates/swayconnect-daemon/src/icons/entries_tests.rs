use std::fs;

use tempfile::TempDir;

use super::*;

fn write_entry(share: &Path, relative: &str, body: &str) {
    let path = share.join("applications").join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, format!("[Desktop Entry]\nType=Application\n{body}")).expect("write");
}

fn index_of(entries: &[(&str, &str)]) -> (TempDir, EntryIndex) {
    let share = TempDir::new().expect("tempdir");
    for (file, body) in entries {
        write_entry(share.path(), file, body);
    }
    let index = EntryIndex::scan(&[share.path().to_path_buf()]);
    (share, index)
}

fn found<'a>(index: &'a EntryIndex, hints: &Hints<'_>) -> Option<&'a str> {
    index.find(hints).map(|entry| entry.id.as_str())
}

fn named<'a>(names: &[&'a str]) -> Hints<'a> {
    Hints {
        names: names.to_vec(),
        ..Hints::default()
    }
}

#[test]
fn each_step_of_the_matching_order_beats_the_later_ones() {
    let (_share, index) = index_of(&[
        ("sandboxed.desktop", "Icon=a\n"),
        ("term.desktop", "Icon=b\n"),
        ("Mail.desktop", "Icon=c\n"),
        ("chat.desktop", "Icon=d\nStartupWMClass=ChatClass\n"),
        ("org.example.Viewer.desktop", "Icon=e\n"),
        ("instance.desktop", "Icon=f\n"),
        ("org.flat.App.desktop", "Icon=g\n"),
        ("jetbrains-studio.desktop", "Icon=h\n"),
    ]);
    let sandboxed = Hints {
        names: vec!["term"],
        sandbox_app_id: Some("sandboxed"),
        ..Hints::default()
    };
    assert_eq!(found(&index, &sandboxed), Some("sandboxed"));
    assert_eq!(found(&index, &named(&["term", "Mail"])), Some("term"));
    assert_eq!(found(&index, &named(&["mail"])), Some("Mail"));
    assert_eq!(found(&index, &named(&["chatclass"])), Some("chat"));
    assert_eq!(
        found(&index, &named(&["viewer"])),
        Some("org.example.Viewer")
    );
    let by_instance = Hints {
        names: vec!["unknown"],
        x11_instance: Some("INSTANCE"),
        ..Hints::default()
    };
    assert_eq!(found(&index, &by_instance), Some("instance"));
    let by_flatpak = Hints {
        names: vec!["unknown"],
        flatpak_id: Some("org.flat.App".to_owned()),
        alias: Some("jetbrains-studio"),
        ..Hints::default()
    };
    assert_eq!(found(&index, &by_flatpak), Some("org.flat.App"));
    let by_alias = Hints {
        names: vec!["Emulator"],
        alias: Some("jetbrains-studio"),
        ..Hints::default()
    };
    assert_eq!(found(&index, &by_alias), Some("jetbrains-studio"));
    assert_eq!(found(&index, &named(&["Emulator"])), None);
}

#[test]
fn a_reverse_dns_suffix_must_be_unique() {
    let (_share, index) = index_of(&[
        ("org.one.Player.desktop", "Icon=a\n"),
        ("org.two.Player.desktop", "Icon=b\n"),
    ]);
    assert_eq!(found(&index, &named(&["player"])), None);
}

#[test]
fn subdirectories_give_dashed_ids_and_earlier_dirs_win() {
    let first = TempDir::new().expect("tempdir");
    let second = TempDir::new().expect("tempdir");
    write_entry(first.path(), "hidden.desktop", "Hidden=true\nIcon=x\n");
    write_entry(first.path(), "kde/notes.desktop", "Icon=kde-notes\n");
    write_entry(second.path(), "hidden.desktop", "Icon=second\n");
    write_entry(second.path(), "plain.desktop", "Icon=plain\n");
    write_entry(second.path(), "noicon.desktop", "Name=None\n");
    let index = EntryIndex::scan(&[first.path().to_path_buf(), second.path().to_path_buf()]);

    assert_eq!(found(&index, &named(&["kde-notes"])), Some("kde-notes"));
    assert_eq!(found(&index, &named(&["hidden"])), None);
    assert_eq!(
        index
            .find(&named(&["plain"]))
            .map(|entry| entry.icon.as_str()),
        Some("plain")
    );
    assert_eq!(found(&index, &named(&["noicon"])), None);
}

#[test]
fn oversized_entries_are_skipped() {
    let padding = "X-Pad=".to_owned() + &"a".repeat(MAX_ENTRY_BYTES as usize);
    let (_share, index) = index_of(&[("big.desktop", &format!("Icon=big\n{padding}\n"))]);
    assert_eq!(found(&index, &named(&["big"])), None);
}

#[test]
fn the_flatpak_hint_comes_from_the_scope_name() {
    let cgroup = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/\
                  app-flatpak-org.mozilla.firefox-2731.scope\n";
    assert_eq!(
        flatpak_id_from_cgroup(cgroup).as_deref(),
        Some("org.mozilla.firefox")
    );
    for none in [
        "0::/user.slice/app.slice/app-sway-alacritty-1.scope\n",
        "0::/app.slice/app-flatpak--12.scope\n",
        "0::/app.slice/app-flatpak-org.x.Y-abc.scope\n",
        "",
    ] {
        assert_eq!(flatpak_id_from_cgroup(none), None, "{none}");
    }
}
