use clap::error::ErrorKind;

use super::*;

fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from([&["swayconnect"], arguments].concat())
}

#[test]
fn every_read_subcommand_parses() {
    for (word, command) in [
        ("info", Command::Info),
        ("state", Command::State),
        ("workspaces", Command::Workspaces),
        ("windows", Command::Windows),
        ("watch", Command::Watch),
    ] {
        let cli = parse(&[word]).expect(word);
        assert_eq!(cli.command, command);
        assert!(!cli.json);
        assert_eq!(cli.timeout(), Duration::from_secs(10));
    }
}

#[test]
fn global_flags_are_accepted_before_and_after_the_subcommand() {
    for arguments in [
        ["--json", "--timeout", "3", "windows"],
        ["windows", "--json", "--timeout", "3"],
    ] {
        let cli = parse(&arguments).expect("flags");
        assert!(cli.json);
        assert_eq!(cli.timeout(), Duration::from_secs(3));
    }
}

#[test]
fn a_timeout_of_zero_or_of_no_number_is_a_usage_error() {
    for value in ["0", "-1", "soon"] {
        let error = parse(&["--timeout", value, "info"]).expect_err(value);
        assert_eq!(error.exit_code(), 2);
    }
}

#[test]
fn pair_takes_an_optional_name() {
    let named = parse(&["pair", "--name", "laptop"]).expect("pair --name");
    let expected = Command::Pair {
        name: Some("laptop".to_owned()),
        uri_arguments: Vec::new(),
    };
    assert_eq!(named.command, expected);

    let unnamed = parse(&["pair"]).expect("pair");
    let expected = Command::Pair {
        name: None,
        uri_arguments: Vec::new(),
    };
    assert_eq!(unnamed.command, expected);
}

#[test]
fn a_uri_argument_of_pair_is_captured_and_not_reported_by_clap() {
    let cli = parse(&["pair", "swayconnect://pair?v=1"]).expect("captured");
    let Command::Pair { uri_arguments, .. } = cli.command else {
        panic!("not pair");
    };
    assert_eq!(uri_arguments, [RefusedUri]);
}

#[test]
fn debug_output_cannot_show_a_pairing_uri_or_its_secret() {
    let secret = "c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2U";
    let uri = format!("swayconnect://pair?v=1&host=desk&port=1&fp={secret}&s={secret}");

    let cli = parse(&["pair", "--name", "laptop", &uri, &uri]).expect("captured");

    let debug_text = format!("{cli:?} {cli:#?} {:?}", cli.command);
    assert!(debug_text.contains("laptop"), "{debug_text}");
    assert!(debug_text.contains("<refused pairing URI>"), "{debug_text}");
    for secret_text in [secret, "swayconnect://", "fp=", "s="] {
        assert!(!debug_text.contains(secret_text), "{debug_text}");
    }
}

#[test]
fn workspace_takes_a_number_or_an_id() {
    for (arguments, number, id) in [
        (&["workspace", "0"][..], Some(0), None),
        (&["workspace", "999"], Some(999), None),
        (&["workspace", "--id", "4"], None, Some(4)),
    ] {
        let cli = parse(arguments).expect("workspace");
        assert_eq!(cli.command, Command::Workspace { number, id });
    }
}

#[test]
fn workspace_without_one_valid_target_is_a_usage_error() {
    for arguments in [
        &["workspace"][..],
        &["workspace", "1000"],
        &["workspace", "-1"],
        &["workspace", "web"],
        &["workspace", "3", "--id", "4"],
        &["workspace", "--id", "web"],
        &["workspace", "--id"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn focus_and_fullscreen_take_a_window_id() {
    let focus = parse(&["focus", "9"]).expect("focus");
    assert_eq!(focus.command, Command::Focus { window_id: 9 });

    for (word, switch) in [("on", Switch::On), ("off", Switch::Off)] {
        let fullscreen = parse(&["fullscreen", "9", word]).expect(word);
        let expected = Command::Fullscreen {
            window_id: 9,
            switch,
        };
        assert_eq!(fullscreen.command, expected);
    }
}

#[test]
fn a_window_action_with_a_wrong_argument_is_a_usage_error() {
    for arguments in [
        &["focus"][..],
        &["focus", "title"],
        &["focus", "-9"],
        &["focus", "9", "10"],
        &["fullscreen", "9"],
        &["fullscreen", "9", "toggle"],
        &["fullscreen", "on", "9"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn layout_takes_a_number_or_an_id() {
    for (arguments, number, id) in [
        (&["layout", "3"][..], Some(3), None),
        (&["layout", "--id", "14"], None, Some(14)),
    ] {
        let cli = parse(arguments).expect("layout");
        assert_eq!(cli.command, Command::Layout { number, id });
    }
    for arguments in [
        &["layout"][..],
        &["layout", "web"],
        &["layout", "-1"],
        &["layout", "3", "--id", "14"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn icon_takes_a_strict_hash_and_an_output_file() {
    let hash = "q83vEjRWeJCrze8SNFZ4kA";
    for arguments in [
        &["icon", hash, "-o", "app.png"][..],
        &["icon", "--output", "app.png", hash],
    ] {
        let cli = parse(arguments).expect("icon");
        let expected = Command::Icon {
            hash: IconHash::parse(hash).expect("hash"),
            output: PathBuf::from("app.png"),
        };
        assert_eq!(cli.command, expected);
    }
    for arguments in [
        &["icon", hash][..],
        &["icon", "-o", "app.png"],
        &["icon", "firefox", "-o", "app.png"],
        &["icon", "q83vEjRWeJCrze8SNFZ4kA==", "-o", "app.png"],
        &["icon", "q83vEjRWeJCrze8SNFZ4kB", "-o", "app.png"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn move_takes_a_window_and_a_workspace_number_or_id() {
    for (arguments, number, id) in [
        (&["move", "6", "0"][..], Some(0), None),
        (&["move", "6", "999"], Some(999), None),
        (&["move", "6", "--id", "5"], None, Some(5)),
    ] {
        let cli = parse(arguments).expect("move");
        let expected = Command::Move {
            window_id: 6,
            number,
            id,
        };
        assert_eq!(cli.command, expected);
    }
}

#[test]
fn float_close_and_lock_parse() {
    let float = parse(&["float", "6", "on"]).expect("float");
    let expected = Command::Float {
        window_id: 6,
        switch: Switch::On,
    };
    assert_eq!(float.command, expected);
    for (arguments, yes) in [
        (&["close", "6"][..], false),
        (&["close", "6", "--yes"], true),
    ] {
        let close = parse(arguments).expect("close");
        assert_eq!(close.command, Command::Close { window_id: 6, yes });
    }
    assert_eq!(parse(&["lock"]).expect("lock").command, Command::Lock);
}

#[test]
fn a_window_or_lock_command_with_a_wrong_argument_is_a_usage_error() {
    for arguments in [
        &["move", "6"][..],
        &["move", "6", "1000"],
        &["move", "6", "web"],
        &["move", "6", "3", "--id", "5"],
        &["move", "title", "3"],
        &["float", "6"],
        &["float", "6", "toggle"],
        &["close"],
        &["close", "-6"],
        &["close", "6", "--yes=no"],
        &["lock", "now"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn awake_on_takes_an_optional_duration_with_a_unit() {
    for (arguments, duration_s) in [
        (&["awake", "on"][..], None),
        (&["awake", "on", "--for", "60s"], Some(60)),
        (&["awake", "on", "--for", "1m"], Some(60)),
        (&["awake", "on", "--for", "90m"], Some(5400)),
        (&["awake", "on", "--for", "2h"], Some(7200)),
    ] {
        let cli = parse(arguments).expect("awake on");
        let switch = AwakeSwitch::On { duration_s };
        assert_eq!(cli.command, Command::Awake { switch }, "{arguments:?}");
    }
    let off = parse(&["awake", "off"]).expect("awake off");
    let switch = AwakeSwitch::Off;
    assert_eq!(off.command, Command::Awake { switch });
}

#[test]
fn awake_with_a_bad_duration_or_switch_is_a_usage_error() {
    for duration in ["59s", "60", "1d"] {
        let error = parse(&["awake", "on", "--for", duration]).expect_err(duration);
        assert_eq!(error.exit_code(), 2, "{duration:?}");
    }
    for arguments in [
        &["awake"][..],
        &["awake", "toggle"],
        &["awake", "off", "--for", "1h"],
        &["awake", "on", "--for"],
    ] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn raw_takes_exactly_one_payload_and_does_not_check_it() {
    let cli = parse(&["raw", "{not json"]).expect("raw");
    let payload = "{not json".to_owned();
    assert_eq!(cli.command, Command::Raw { payload });

    for arguments in [&["raw"][..], &["raw", "{}", "{}"]] {
        let error = parse(arguments).expect_err("usage error");
        assert_eq!(error.exit_code(), 2, "{arguments:?}");
    }
}

#[test]
fn a_missing_or_unknown_subcommand_is_a_usage_error() {
    let missing = parse(&[]).expect_err("no subcommand");
    assert_eq!(missing.exit_code(), 2);
    let unknown = parse(&["reboot"]).expect_err("unknown subcommand");
    assert_eq!(unknown.kind(), ErrorKind::InvalidSubcommand);
    assert_eq!(unknown.exit_code(), 2);
    let extra = parse(&["info", "extra"]).expect_err("unexpected argument");
    assert_eq!(extra.exit_code(), 2);
}
