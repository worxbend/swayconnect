//! The temp `XDG_DATA_DIRS` of `docs/DESIGN.md` 11.12: one desktop entry and one small
//! PNG in the `hicolor` theme, so icon hashes are the same in every test run.

use std::fs;
use std::path::{Path, PathBuf};

/// Desktop entry id of the one entry. Equals the `app_id` of window 6 in both trees of
/// [`crate::fixtures`], so that window resolves to [`NOTES_ICON_PNG`].
pub const NOTES_APP_ID: &str = "notes";

/// Contents of `applications/notes.desktop`. `Exec` names a program that does not exist.
pub const NOTES_DESKTOP_ENTRY: &str = "[Desktop Entry]\n\
    Type=Application\n\
    Name=Notes\n\
    Exec=/nonexistent/notes %U\n\
    Icon=notes\n";

/// The icon file: a 48 x 48 RGBA PNG of one colour, 124 bytes.
pub const NOTES_ICON_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00, 0x30, 0x08, 0x06, 0x00, 0x00, 0x00, 0x57, 0x02, 0xf9,
    0x87, 0x00, 0x00, 0x00, 0x43, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xcf, 0x21, 0x0d, 0x00,
    0x00, 0x08, 0x00, 0x30, 0x12, 0x21, 0xe8, 0x47, 0x76, 0xa8, 0x80, 0x65, 0xbb, 0x78, 0x80, 0x47,
    0x76, 0xcd, 0x67, 0x21, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
    0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
    0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x70, 0xb5, 0xce, 0x6f, 0xbc, 0xf1, 0x1f, 0x13, 0xdc, 0xe5,
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A minimal `hicolor/index.theme` with the one directory the icon sits in.
const HICOLOR_INDEX: &str = "[Icon Theme]\n\
    Name=Hicolor\n\
    Directories=48x48/apps\n\
    \n\
    [48x48/apps]\n\
    Size=48\n\
    Type=Fixed\n";

/// Path of the desktop entry under the data dir `share`.
pub(crate) fn desktop_entry_path(share: &Path) -> PathBuf {
    share
        .join("applications")
        .join(format!("{NOTES_APP_ID}.desktop"))
}

/// Path of the icon file under the data dir `share`.
pub(crate) fn icon_path(share: &Path) -> PathBuf {
    hicolor(share)
        .join("48x48/apps")
        .join(format!("{NOTES_APP_ID}.png"))
}

/// Writes the entry, the theme index and the icon under `share`. Panics when the file
/// system refuses, like the rest of `TestDirs::new`.
pub(crate) fn populate(share: &Path) {
    write(&desktop_entry_path(share), NOTES_DESKTOP_ENTRY.as_bytes());
    write(
        &hicolor(share).join("index.theme"),
        HICOLOR_INDEX.as_bytes(),
    );
    write(&icon_path(share), NOTES_ICON_PNG);
}

fn hicolor(share: &Path) -> PathBuf {
    share.join("icons").join("hicolor")
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
    }
    fs::write(path, contents).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_png_is_48_by_48_and_ends_with_iend() {
        let (signature, rest) = NOTES_ICON_PNG.split_at(8);
        assert_eq!(signature, b"\x89PNG\r\n\x1a\n");
        // IHDR: length 13, type, width, height.
        assert_eq!(&rest[4..8], b"IHDR");
        assert_eq!(rest[8..12], 48u32.to_be_bytes());
        assert_eq!(rest[12..16], 48u32.to_be_bytes());
        assert!(NOTES_ICON_PNG.ends_with(b"IEND\xae\x42\x60\x82"));
    }

    #[test]
    fn the_entry_names_the_icon_of_the_same_id() {
        assert!(NOTES_DESKTOP_ENTRY.starts_with("[Desktop Entry]\n"));
        assert!(
            NOTES_DESKTOP_ENTRY
                .lines()
                .any(|line| line == format!("Icon={NOTES_APP_ID}"))
        );
    }
}
