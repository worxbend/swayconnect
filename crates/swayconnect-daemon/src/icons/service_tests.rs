use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::TempDir;

use super::*;

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>"#;

/// Answers a fixed PNG, or fails when `output` is `None`; counts its calls.
struct FakeRaster {
    calls: Arc<AtomicUsize>,
    output: Option<Vec<u8>>,
}

impl Rasteriser for FakeRaster {
    fn rasterise(&self, _source: &[u8]) -> Result<Vec<u8>, RasterError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.output.clone().ok_or(RasterError::Empty)
    }
}

struct Fixture {
    root: TempDir,
    calls: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Fixture {
        let fixture = Fixture {
            root: TempDir::new().expect("tempdir"),
            calls: Arc::default(),
        };
        fs::create_dir_all(fixture.share().join("applications")).expect("mkdir");
        fs::create_dir_all(fixture.share().join("pixmaps")).expect("mkdir");
        fixture
    }

    fn share(&self) -> PathBuf {
        self.root.path().join("share")
    }

    fn add_app(&self, id: &str, file_name: &str, icon: &[u8]) {
        let entry = format!("[Desktop Entry]\nType=Application\nIcon={id}\n");
        fs::write(
            self.share().join(format!("applications/{id}.desktop")),
            entry,
        )
        .expect("entry");
        fs::write(self.share().join("pixmaps").join(file_name), icon).expect("icon");
    }

    fn resolver(&self, output: Option<Vec<u8>>) -> Resolver<FakeRaster> {
        let settings = IconSettings {
            search_dirs: vec![self.share()],
            cache_dir: Some(self.root.path().join("cache")),
            uid: rustix::process::getuid().as_raw(),
            proc_root: self.root.path().join("proc"),
            config: IconsConfig::default(),
        };
        let disk = DiskCache::open(settings.cache_dir.as_deref().expect("set"), settings.uid);
        Resolver {
            settings,
            rasteriser: FakeRaster {
                calls: Arc::clone(&self.calls),
                output,
            },
            disk: Some(disk.expect("disk cache")),
            index: EntryIndex::default(),
            built_at: None,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn job(app_id: &str) -> Job {
    Job {
        key: IconKey {
            app_id: Some(app_id.to_owned()),
            ..IconKey::default()
        },
        pid: None,
    }
}

#[test]
fn a_small_png_is_served_as_it_is_without_the_rasteriser() {
    let fixture = Fixture::new();
    let mut small = png::header(48, 48);
    small.extend_from_slice(b"rest of the file");
    fixture.add_app("small", "small.png", &small);
    let produced = fixture.resolver(None).produce(&job("small")).expect("icon");
    assert_eq!(produced, small);
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn a_huge_png_header_is_refused_before_the_rasteriser() {
    let fixture = Fixture::new();
    fixture.add_app("huge", "huge.png", &png::header(100_000, 100_000));
    let outcome = fixture
        .resolver(Some(png::header(8, 8)))
        .produce(&job("huge"));
    assert!(matches!(outcome, Err(IconError::PngHeader)));
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn a_source_over_1_mib_is_refused() {
    let fixture = Fixture::new();
    let mut big = SVG.to_vec();
    big.resize(MAX_SOURCE_BYTES + 1, b' ');
    fixture.add_app("big", "big.svg", &big);
    let outcome = fixture
        .resolver(Some(png::header(8, 8)))
        .produce(&job("big"));
    assert!(matches!(outcome, Err(IconError::SourceTooLarge)));
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn rasterised_icons_come_from_the_disk_cache_after_a_restart() {
    let fixture = Fixture::new();
    fixture.add_app("vector", "vector.svg", SVG);
    let png = png::header(128, 128);
    let first = fixture.resolver(Some(png.clone())).produce(&job("vector"));
    assert_eq!(first.expect("icon"), png);
    let second = fixture.resolver(None).produce(&job("vector"));
    assert_eq!(second.expect("cached icon"), png);
    assert_eq!(fixture.calls(), 1);
}

#[test]
fn an_unservable_rasteriser_output_is_refused() {
    let fixture = Fixture::new();
    fixture.add_app("vector", "vector.svg", SVG);
    let outcome = fixture
        .resolver(Some(png::header(129, 8)))
        .produce(&job("vector"));
    assert!(matches!(
        outcome,
        Err(IconError::Raster(RasterError::OutputNotServable))
    ));
}

#[test]
fn a_new_entry_is_seen_only_after_the_rebuild_interval() {
    let fixture = Fixture::new();
    let mut resolver = fixture.resolver(Some(png::header(8, 8)));
    assert!(matches!(
        resolver.produce(&job("late")),
        Err(IconError::NoEntry)
    ));
    fixture.add_app("late", "late.svg", SVG);
    assert!(matches!(
        resolver.produce(&job("late")),
        Err(IconError::NoEntry)
    ));

    resolver.built_at = Instant::now().checked_sub(INDEX_REBUILD_INTERVAL);
    assert!(resolver.produce(&job("late")).is_ok());
}

#[test]
fn the_flatpak_hint_is_read_from_the_process_cgroup() {
    let fixture = Fixture::new();
    fixture.add_app("org.flat.App", "org.flat.App.svg", SVG);
    let cgroup = fixture.root.path().join("proc/4242/cgroup");
    fs::create_dir_all(cgroup.parent().expect("parent")).expect("mkdir");
    fs::write(&cgroup, "0::/app.slice/app-flatpak-org.flat.App-77.scope\n").expect("write");
    let mut flatpak = job("unrelated");
    flatpak.pid = Some(4242);
    assert!(
        fixture
            .resolver(Some(png::header(8, 8)))
            .produce(&flatpak)
            .is_ok()
    );
}

#[test]
fn keys_without_names_or_with_huge_names_are_not_looked_up() {
    assert!(!IconKey::default().is_lookup_worthy());
    let huge = IconKey {
        x11_class: Some("x".repeat(MAX_KEY_FIELD_BYTES + 1)),
        ..IconKey::default()
    };
    assert!(!huge.is_lookup_worthy());
    assert!(job("notes").key.is_lookup_worthy());
}

#[test]
fn search_dirs_put_the_data_home_first_and_default_the_system_dirs() {
    let home = Path::new("/home/u/.local/share");
    let listed = search_dirs(home, Some(OsStr::new("/a:relative::/b")));
    assert_eq!(listed, [home, Path::new("/a"), Path::new("/b")]);
    let defaulted = search_dirs(home, Some(OsStr::new("")));
    assert_eq!(
        defaulted,
        [home, Path::new("/usr/local/share"), Path::new("/usr/share")]
    );
    assert_eq!(search_dirs(home, None), defaulted);
}

#[test]
fn a_window_node_gives_every_hint_and_an_xwayland_instance_alone_finds_the_entry() {
    let node: Node = serde_json::from_str(
        r#"{"id":5,"type":"con","app_id":null,"pid":4242,"sandbox_app_id":"org.example.Notes",
            "window_properties":{"class":"Unrelated-Class","instance":"small"}}"#,
    )
    .expect("node");
    let key = IconKey::of_window(&node);
    assert_eq!(key.app_id, None);
    assert_eq!(key.x11_class.as_deref(), Some("Unrelated-Class"));
    assert_eq!(key.x11_instance.as_deref(), Some("small"));
    assert_eq!(key.sandbox_app_id.as_deref(), Some("org.example.Notes"));

    let fixture = Fixture::new();
    let small = png::header(48, 48);
    fixture.add_app("small", "small.png", &small);
    let produced = fixture
        .resolver(None)
        .produce(&Job { key, pid: None })
        .expect("matched by the instance");
    assert_eq!(produced, small);
}

#[test]
fn a_fifo_is_refused_by_the_open_itself_without_blocking() {
    use rustix::fs::{CWD, FileType, Mode, mknodat};
    let fixture = Fixture::new();
    let fifo = fixture.share().join("pixmaps/pipe.png");
    mknodat(CWD, &fifo, FileType::Fifo, Mode::RUSR | Mode::WUSR, 0).expect("mkfifo");
    let (done, finished) = mpsc::channel();
    // A plain blocking open of a FIFO with no writer never returns: it runs on its own thread.
    thread::spawn(move || done.send(open_regular(&fifo).is_err()).expect("send"));
    let refused = finished
        .recv_timeout(Duration::from_secs(3))
        .expect("the open must not block on a FIFO");
    assert!(refused, "a FIFO is not a regular file");
}
