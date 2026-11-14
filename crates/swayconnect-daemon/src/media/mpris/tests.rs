//! Property parsing and the no-bus cases. No test connects to a real bus.

use super::*;

fn owned<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
    OwnedVariant::try_from(value.into()).expect("owned value")
}

fn map(entries: Vec<(&str, OwnedVariant)>) -> HashMap<String, OwnedVariant> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn metadata(entries: Vec<(&str, OwnedVariant)>) -> OwnedVariant {
    OwnedVariant::from(map(entries))
}

fn root() -> HashMap<String, OwnedVariant> {
    map(vec![
        ("Identity", owned("Spotify")),
        ("DesktopEntry", owned("spotify")),
        ("CanQuit", owned(true)),
    ])
}

#[test]
fn a_player_report_is_read_from_the_two_property_maps() {
    let player = map(vec![
        ("PlaybackStatus", owned("Paused")),
        (
            "Metadata",
            metadata(vec![
                ("xesam:title", owned("Clair de Lune")),
                ("xesam:artist", owned(vec!["Claude Debussy", "", "Other"])),
                ("xesam:album", owned("Suite bergamasque")),
                ("mpris:length", owned(300_000_000_i64)),
            ]),
        ),
        ("CanPlay", owned(true)),
        ("CanPause", owned(true)),
        ("CanGoNext", owned(false)),
        ("CanGoPrevious", owned(true)),
    ]);
    let report = report_of(
        "org.mpris.MediaPlayer2.spotify".to_owned(),
        &root(),
        &player,
    );
    assert_eq!(
        report,
        PlayerReport {
            bus_name: "org.mpris.MediaPlayer2.spotify".to_owned(),
            identity: Some("Spotify".to_owned()),
            desktop_entry: Some("spotify".to_owned()),
            status: "Paused".to_owned(),
            title: Some("Clair de Lune".to_owned()),
            artist: Some("Claude Debussy, Other".to_owned()),
            album: Some("Suite bergamasque".to_owned()),
            can_play: true,
            can_pause: true,
            can_go_next: false,
            can_go_previous: true,
        }
    );
}

#[test]
fn can_control_false_turns_every_command_off() {
    let player = map(vec![
        ("PlaybackStatus", owned("Playing")),
        ("CanControl", owned(false)),
        ("CanPlay", owned(true)),
        ("CanPause", owned(true)),
        ("CanGoNext", owned(true)),
        ("CanGoPrevious", owned(true)),
    ]);
    let report = report_of("org.mpris.MediaPlayer2.x".to_owned(), &root(), &player);
    assert!(
        !report.can_play && !report.can_pause && !report.can_go_next && !report.can_go_previous
    );
}

#[test]
fn missing_mistyped_and_empty_properties_count_as_absent() {
    let root = map(vec![
        ("Identity", owned(7_u32)),
        ("DesktopEntry", owned("")),
    ]);
    let player = map(vec![
        ("CanPlay", owned("yes")),
        (
            "Metadata",
            metadata(vec![
                ("xesam:title", owned("")),
                ("xesam:artist", owned("Solo")),
            ]),
        ),
    ]);
    let report = report_of("org.mpris.MediaPlayer2.x".to_owned(), &root, &player);
    assert_eq!(report.identity, None);
    assert_eq!(report.desktop_entry, None);
    assert_eq!(report.status, "");
    assert_eq!(report.title, None);
    assert_eq!(
        report.artist.as_deref(),
        Some("Solo"),
        "a plain string artist is kept"
    );
    assert!(!report.can_play);
}

#[test]
fn metadata_that_is_not_a_map_is_ignored() {
    let player = map(vec![("Metadata", owned("not a map"))]);
    let report = report_of("org.mpris.MediaPlayer2.x".to_owned(), &root(), &player);
    assert_eq!(
        (report.title, report.artist, report.album),
        (None, None, None)
    );
}

#[test]
fn each_command_has_its_mpris_method() {
    assert_eq!(method_name(MediaCommand::PlayPause), "PlayPause");
    assert_eq!(method_name(MediaCommand::Next), "Next");
    assert_eq!(method_name(MediaCommand::Previous), "Previous");
}

#[tokio::test]
async fn without_a_bus_every_call_is_unavailable() {
    let (_stop, shutdown) = watch::channel(false);
    let backend = MprisBackend::spawn(None, shutdown);
    assert!(matches!(
        backend.players().await,
        Err(BackendError::Unavailable(_))
    ));
    assert!(matches!(
        backend
            .send("org.mpris.MediaPlayer2.x", MediaCommand::PlayPause)
            .await,
        Err(BackendError::Unavailable(_))
    ));
}

#[tokio::test]
async fn an_address_with_no_bus_behind_it_is_unavailable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let address = format!("unix:path={}/no-bus", dir.path().display());
    let (stop, shutdown) = watch::channel(false);
    let backend = MprisBackend::spawn(Some(address), shutdown);
    assert!(matches!(
        backend.players().await,
        Err(BackendError::Unavailable(_))
    ));
    stop.send(true).expect("the task listens");
}
