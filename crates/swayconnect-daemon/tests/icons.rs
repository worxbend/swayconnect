//! The icon service against the testkit's temp `XDG_DATA_DIRS` and the real
//! `swayconnectd --raster` child (`docs/DESIGN.md` 11.5, slice D-ICON).
//!
//! Every search dir is a temp dir: no test reads `/usr/share`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use swayconnect_daemon::icons::raster::{CHILD_WALL_TIME, ChildRasteriser, Rasteriser};
use swayconnect_daemon::icons::store::Lookup;
use swayconnect_daemon::icons::{IconKey, IconService, IconSettings, IconsConfig};
use swayconnect_protocol::IconHash;
use swayconnect_protocol::limits::MAX_ICON_SIDE_PX;
use swayconnect_testkit::TestDirs;
use swayconnect_testkit::fixtures::{NOTES_APP_ID, NOTES_ICON_PNG};
use tokio::sync::watch;

const DAEMON: &str = env!("CARGO_BIN_EXE_swayconnectd");
const PATIENCE: Duration = Duration::from_secs(30);
/// Spawning a debug-build child under parallel tests takes part of a second.
const SPAWN_MARGIN: Duration = Duration::from_secs(1);

const GOOD_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
    <circle cx="16" cy="16" r="12" fill="#c84"/></svg>"##;

/// A blur over a huge area: gigabytes and minutes in a renderer without the filter rule.
const BLUR_BOMB_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000">
    <filter id="f" x="-10" y="-10" width="20" height="20">
    <feGaussianBlur stdDeviation="100000"/></filter>
    <rect width="100000" height="100000" filter="url(#f)"/></svg>"#;

/// Ten levels of ten `<use>` each: 10^10 nodes once expanded.
fn use_fan_out_svg() -> String {
    let mut defs = String::from(r#"<rect id="l0" width="1" height="1"/>"#);
    for level in 1..=10 {
        let uses = format!(r##"<use href="#l{}"/>"##, level - 1).repeat(10);
        defs.push_str(&format!(r#"<g id="l{level}">{uses}</g>"#));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><defs>{defs}</defs><use href="#l10"/></svg>"##
    )
}

/// A PNG whose header declares 100000 x 100000 pixels.
fn huge_ihdr_png() -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png
}

fn settings(dirs: &TestDirs) -> IconSettings {
    IconSettings {
        search_dirs: vec![dirs.data_home(), dirs.data_dirs()],
        cache_dir: Some(dirs.cache_home().join("swayconnect")),
        uid: rustix::process::getuid().as_raw(),
        proc_root: dirs.root().join("proc"),
        config: IconsConfig::default(),
    }
}

fn start(dirs: &TestDirs) -> IconService {
    let rasteriser = ChildRasteriser::new(DAEMON.into());
    IconService::start(settings(dirs), rasteriser).expect("start icon service")
}

/// Adds `applications/<id>.desktop` and `pixmaps/<file_name>` to the temp data dir.
fn add_app(share: &Path, id: &str, file_name: &str, icon: &[u8]) {
    let entry = format!("[Desktop Entry]\nType=Application\nName={id}\nIcon={id}\n");
    fs::write(share.join(format!("applications/{id}.desktop")), entry).expect("entry");
    fs::create_dir_all(share.join("pixmaps")).expect("pixmaps");
    fs::write(share.join("pixmaps").join(file_name), icon).expect("icon");
}

fn window(app_id: &str) -> IconKey {
    IconKey {
        app_id: Some(app_id.to_owned()),
        ..IconKey::default()
    }
}

async fn wait_finished(finished: &mut watch::Receiver<u64>, count: u64) {
    tokio::time::timeout(PATIENCE, finished.wait_for(|done| *done >= count))
        .await
        .expect("icon jobs did not finish in time")
        .expect("icon service stopped");
}

async fn resolve(service: &IconService, key: &IconKey) -> Lookup {
    let mut finished = service.finished();
    let before = *finished.borrow();
    if service.icon_for(key, None).is_none() {
        wait_finished(&mut finished, before + 1).await;
    }
    service.lookup(key)
}

#[tokio::test]
async fn the_notes_window_gets_the_fixture_icon_by_hash() {
    let dirs = TestDirs::new();
    let service = start(&dirs);
    let notes = window(NOTES_APP_ID);

    let expected = IconHash::of(NOTES_ICON_PNG);
    assert_eq!(resolve(&service, &notes).await, Lookup::Ready(expected));
    assert_eq!(service.icon_for(&notes, None), Some(expected));

    let unknown = IconHash::of(b"never produced");
    let result = service.icons(&[unknown, expected]);
    assert_eq!(result.missing, [unknown]);
    assert_eq!(result.icons.len(), 1);
    assert_eq!(result.icons[0].png, NOTES_ICON_PNG);
    assert_eq!(result.icons[0].verify(), Ok(()));
}

#[tokio::test]
async fn an_svg_is_rasterised_in_the_child_to_a_servable_png() {
    let dirs = TestDirs::new();
    add_app(
        &dirs.data_dirs(),
        "vector",
        "vector.svg",
        GOOD_SVG.as_bytes(),
    );
    let service = start(&dirs);

    let Lookup::Ready(hash) = resolve(&service, &window("vector")).await else {
        panic!("the SVG gave no icon");
    };
    let icon = service.icons(&[hash]).icons.pop().expect("served");
    assert_eq!(icon.verify(), Ok(()));
    assert_eq!(IconHash::of(&icon.png), hash);
    assert!(icon.png.len() <= 65_536);
    let side = |offset: usize| u32::from_be_bytes(icon.png[offset..offset + 4].try_into().unwrap());
    assert_eq!((side(16), side(20)), (MAX_ICON_SIDE_PX, MAX_ICON_SIDE_PX));
}

#[tokio::test]
async fn hostile_icons_end_within_3_s_while_other_icons_are_served() {
    let dirs = TestDirs::new();
    let share = dirs.data_dirs();
    add_app(&share, "fanout", "fanout.svg", use_fan_out_svg().as_bytes());
    add_app(&share, "blur", "blur.svg", BLUR_BOMB_SVG.as_bytes());
    add_app(&share, "huge", "huge.png", &huge_ihdr_png());
    add_app(&share, "after", "after.svg", GOOD_SVG.as_bytes());
    let service = start(&dirs);
    let Lookup::Ready(notes) = resolve(&service, &window(NOTES_APP_ID)).await else {
        panic!("the fixture icon was not produced");
    };

    let mut finished = service.finished();
    let before = *finished.borrow();
    let queued = ["fanout", "blur", "huge", "after"];
    let mut previous = Instant::now();
    for app in queued {
        assert_eq!(service.icon_for(&window(app), None), None);
    }
    // The worker is busy with the fan-out SVG; icon.get answers at once anyway.
    assert_eq!(service.icons(&[notes]).icons.len(), 1);
    for (done, app) in (before + 1..).zip(queued) {
        wait_finished(&mut finished, done).await;
        let took = previous.elapsed();
        assert!(
            took <= CHILD_WALL_TIME + SPAWN_MARGIN,
            "{app} took {took:?}"
        );
        previous = Instant::now();
    }

    for hostile in ["fanout", "blur", "huge"] {
        assert_eq!(
            service.lookup(&window(hostile)),
            Lookup::Missing,
            "{hostile}"
        );
    }
    assert!(matches!(service.lookup(&window("after")), Lookup::Ready(_)));
}

#[tokio::test]
async fn a_failed_key_is_not_tried_again_within_the_negative_ttl() {
    let dirs = TestDirs::new();
    let service = start(&dirs);
    let unknown = window("no-such-app");
    assert_eq!(resolve(&service, &unknown).await, Lookup::Missing);

    let mut finished = service.finished();
    let before = *finished.borrow();
    assert_eq!(service.icon_for(&unknown, None), None);
    assert_eq!(service.icon_for(&window(NOTES_APP_ID), None), None);
    wait_finished(&mut finished, before + 1).await;
    // Jobs run in order: had the failed key been queued again, it would have been first.
    assert!(matches!(
        service.lookup(&window(NOTES_APP_ID)),
        Lookup::Ready(_)
    ));
    assert_eq!(service.lookup(&unknown), Lookup::Missing);
}

#[test]
fn the_raster_child_refuses_hostile_files_within_its_limits() {
    let child = ChildRasteriser::new(DAEMON.into());
    let fan_out = use_fan_out_svg();
    let huge = huge_ihdr_png();
    for (name, source) in [
        ("fan-out", fan_out.as_bytes()),
        ("blur", BLUR_BOMB_SVG.as_bytes()),
        ("huge IHDR", &huge[..]),
        ("garbage", b"\x00\x01 not an image"),
    ] {
        let started = Instant::now();
        let outcome = child.rasterise(source);
        let took = started.elapsed();
        assert!(outcome.is_err(), "{name} was rendered");
        assert!(
            took <= CHILD_WALL_TIME + SPAWN_MARGIN,
            "{name} took {took:?}"
        );
    }
    assert!(child.rasterise(GOOD_SVG.as_bytes()).is_ok());
}
