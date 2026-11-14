use super::*;

const OWNER: u32 = 1000;
const OTHER: u32 = 1001;

fn scanner(root: &Path) -> ProcScanner {
    ProcScanner::new(root.to_owned(), OWNER, &SessionConfig::default())
}

#[test]
fn a_locker_of_the_own_uid_means_locked_and_grabbed() {
    let root = tempfile::tempdir().expect("temp dir");
    add_fake_process(root.path(), 1, "systemd", 0);
    add_fake_process(root.path(), 4321, "swaylock", OWNER);

    let facts = scanner(root.path()).scan().expect("scan");

    assert!(facts.locked);
    assert_eq!(
        facts.grabbers,
        [Grabber {
            pid: 4321,
            name: ProcessName("swaylock".to_owned()),
        }]
    );
}

#[test]
fn processes_of_another_uid_do_not_count() {
    let root = tempfile::tempdir().expect("temp dir");
    add_fake_process(root.path(), 10, "swaylock", OTHER);
    add_fake_process(root.path(), 11, "fuzzel", OTHER);

    assert_eq!(
        scanner(root.path()).scan().expect("scan"),
        DesktopFacts::default()
    );
}

#[test]
fn a_launcher_is_a_grabber_but_not_a_lock() {
    let root = tempfile::tempdir().expect("temp dir");
    add_fake_process(root.path(), 77, "fuzzel", OWNER);
    add_fake_process(root.path(), 78, "alacritty", OWNER);

    let facts = scanner(root.path()).scan().expect("scan");

    assert!(!facts.locked);
    let pids: Vec<u32> = facts.grabbers.iter().map(|grabber| grabber.pid).collect();
    assert_eq!(pids, [77]);
}

#[test]
fn entries_that_are_not_processes_or_have_ended_are_skipped() {
    let root = tempfile::tempdir().expect("temp dir");
    fs::create_dir(root.path().join("self")).expect("dir");
    fs::write(root.path().join("uptime"), "1.0 1.0\n").expect("file");
    fs::create_dir(root.path().join("12a")).expect("dir");
    // A process directory without status: the process ended during the scan.
    fs::create_dir(root.path().join("55")).expect("dir");
    add_fake_process(root.path(), 56, "fuzzel", OWNER);

    let facts = scanner(root.path()).scan().expect("scan");

    assert_eq!(facts.grabbers.len(), 1);
}

#[test]
fn an_unreadable_proc_is_an_error_so_callers_fail_closed() {
    let root = tempfile::tempdir().expect("temp dir");
    assert!(scanner(&root.path().join("missing")).scan().is_err());

    // `status` that cannot be read as a file.
    fs::create_dir_all(root.path().join("90/status")).expect("dir");
    assert!(scanner(root.path()).scan().is_err());
}

#[test]
fn a_status_without_uid_is_an_error() {
    let root = tempfile::tempdir().expect("temp dir");
    fs::create_dir(root.path().join("91")).expect("dir");
    fs::write(root.path().join("91/status"), "Name:\tswaylock\n").expect("file");
    assert!(scanner(root.path()).scan().is_err());
}

/// Read-only: finds this test process in the real `/proc` by its own name.
#[test]
fn the_real_proc_shows_this_process() {
    let own_name = fs::read_to_string("/proc/self/comm").expect("own comm");
    let config = SessionConfig {
        keyboard_grabbers: vec![
            ProcessName::try_from(own_name.trim_end().to_owned()).expect("name"),
        ],
        ..SessionConfig::default()
    };
    let uid = rustix::process::getuid().as_raw();

    let facts = ProcScanner::new(PathBuf::from("/proc"), uid, &config)
        .scan()
        .expect("scan of /proc");

    assert!(
        facts
            .grabbers
            .iter()
            .any(|grabber| grabber.pid == std::process::id())
    );
}

#[test]
fn the_session_section_has_the_documented_defaults_and_checks() {
    let parse = |text: &str| toml::from_str::<SessionConfig>(text);
    let defaults = parse("").expect("empty section");
    assert_eq!(defaults, SessionConfig::default());
    assert_eq!(defaults.lock, None);
    let grabbers: Vec<&str> = defaults
        .keyboard_grabbers
        .iter()
        .map(ProcessName::as_str)
        .collect();
    assert_eq!(grabbers, DEFAULT_KEYBOARD_GRABBERS);

    let set = parse(
        r#"lock = ["/home/owner/.config/sway/scripts/lock.sh"]
lock_processes = ["gtklock"]
keyboard_grabbers = []"#,
    )
    .expect("valid");
    assert!(set.lock.is_some());
    assert_eq!(set.lock_processes[0].as_str(), "gtklock");
    assert!(set.keyboard_grabbers.is_empty());

    for refused in [
        "lock_processes = []",
        r#"lock_processes = ["/usr/bin/swaylock"]"#,
        r#"lock_processes = ["a-name-of-16-byt"]"#,
        r#"keyboard_grabbers = [""]"#,
        r#"keyboard_grabbers = ["a\\b"]"#,
        r#"lock = ["lock.sh"]"#,
        "lock = []",
        "unknown = 1",
    ] {
        assert!(parse(refused).is_err(), "{refused}");
    }
}
