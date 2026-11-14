//! Tests of `main.rs`: argument parsing and the pairing prompt.

use super::*;

#[test]
fn only_y_and_yes_confirm_a_device() {
    for yes in ["y\n", "Y\n", "yes\n", " YES \r\n"] {
        assert!(is_yes(yes), "{yes:?}");
    }
    for no in ["", "\n", "n\n", "no\n", "yy\n", "ok\n", "1\n"] {
        assert!(!is_yes(no), "{no:?}");
    }
}

#[tokio::test]
async fn a_line_typed_before_the_device_was_shown_is_not_its_answer() {
    let typed_early = Instant::now();
    let (terminal, mut lines) = watch::channel(Some((typed_early, "y".to_owned())));
    let shown_at = typed_early + Duration::from_millis(1);

    let mut answer = std::pin::pin!(answer_typed_after(&mut lines, shown_at));
    let early = tokio::time::timeout(Duration::from_millis(50), &mut answer).await;
    assert!(early.is_err(), "the early line was taken as the answer");

    terminal.send_replace(Some((shown_at, "n".to_owned())));
    assert_eq!(answer.await.as_deref(), Some("n"));
}

#[tokio::test]
async fn a_closed_terminal_gives_no_answer_even_after_an_early_line() {
    let typed_early = Instant::now();
    let (terminal, mut lines) = watch::channel(Some((typed_early, "y".to_owned())));
    drop(terminal);

    let shown_at = typed_early + Duration::from_millis(1);
    assert_eq!(answer_typed_after(&mut lines, shown_at).await, None);
}

#[test]
fn the_firewall_hint_is_for_non_loopback_hosts() {
    assert!(is_loopback_host("127.0.0.1"));
    assert!(is_loopback_host("::1"));
    assert!(!is_loopback_host("192.168.1.200"));
    assert!(!is_loopback_host("desk.example"));
}

#[test]
fn pair_takes_scopes_ttl_and_host_and_refuses_values_outside_the_limits() {
    let cli = Cli::try_parse_from(["swayconnectd", "pair"]).expect("defaults");
    let Some(Command::Pair { scopes, ttl, host }) = cli.command else {
        panic!("pair was parsed as another command");
    };
    // The owner's default of 2026-11-14 (`docs/DESIGN.md` 11.8, D30). Never by default:
    // `destructive`, `audio.capture`, `launch.risky`.
    assert_eq!(
        scopes,
        [
            Scope::StateRead,
            Scope::StateTitles,
            Scope::Control,
            Scope::Audio,
            Scope::Media,
            Scope::IdleInhibit,
            Scope::Launch,
            Scope::InputKeys,
            Scope::InputText,
        ]
    );
    for by_name_only in [Scope::Destructive, Scope::AudioCapture, Scope::LaunchRisky] {
        assert!(!scopes.contains(&by_name_only), "{by_name_only:?}");
    }
    assert_eq!((ttl, host), (300, None));

    let narrowed = [
        "swayconnectd",
        "pair",
        "--scopes",
        "state.read,control",
        "--ttl",
        "30",
    ];
    let Some(Command::Pair { scopes, ttl, .. }) =
        Cli::try_parse_from(narrowed).expect("valid").command
    else {
        panic!("pair was parsed as another command");
    };
    assert_eq!((scopes, ttl), (vec![Scope::StateRead, Scope::Control], 30));

    for refused in [
        ["--ttl", "29"],
        ["--ttl", "601"],
        ["--scopes", "state.read,root"],
    ] {
        let arguments = ["swayconnectd", "pair"].into_iter().chain(refused);
        assert!(Cli::try_parse_from(arguments).is_err(), "{refused:?}");
    }
}

#[test]
fn devices_lists_and_grant_or_deny_need_a_prefix_and_known_scopes() {
    let listed = Cli::try_parse_from(["swayconnectd", "devices"]).expect("list");
    assert!(matches!(
        listed.command,
        Some(Command::Devices { change: None })
    ));
    let grant = [
        "swayconnectd",
        "devices",
        "grant",
        "wMHC",
        "state.titles",
        "control",
    ];
    let Some(Command::Devices {
        change: Some(DevicesChange::Grant(args)),
    }) = Cli::try_parse_from(grant).expect("grant").command
    else {
        panic!("grant was parsed as another command");
    };
    assert_eq!(args.prefix, "wMHC");
    assert_eq!(args.scopes, [Scope::StateTitles, Scope::Control]);
    let deny = ["swayconnectd", "devices", "deny", "wMHC", "control"];
    assert!(matches!(
        Cli::try_parse_from(deny).expect("deny").command,
        Some(Command::Devices {
            change: Some(DevicesChange::Deny(_))
        })
    ));

    for refused in [
        &["grant", "wMHC"][..],
        &["grant", "wMHC", "root"],
        &["deny"],
    ] {
        let arguments = ["swayconnectd", "devices"].iter().chain(refused);
        assert!(Cli::try_parse_from(arguments).is_err(), "{refused:?}");
    }
}

#[test]
fn no_subcommand_means_run() {
    let cli = Cli::try_parse_from(["swayconnectd"]).expect("valid");
    assert!(cli.command.is_none());
}
