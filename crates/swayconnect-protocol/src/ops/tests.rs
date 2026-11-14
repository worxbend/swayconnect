use super::*;
use crate::input::{Key, Modifier};

fn args_of(text: &str) -> Value {
    serde_json::from_str(text).expect("test JSON")
}

#[test]
fn absent_args_mean_an_empty_object() {
    assert_eq!(Operation::Ping.decode_args(None), Ok(Args::Ping));
    assert!(Operation::WindowFocus.decode_args(None).is_err());
}

#[test]
fn args_that_are_not_an_object_are_refused_for_every_operation() {
    for bad in ["null", "5", "[]", "\"x\""] {
        assert_eq!(
            Operation::Ping.decode_args(Some(&args_of(bad))),
            Err(InvalidMember("args")),
            "{bad}"
        );
    }
}

#[test]
fn subscribe_topics_limits() {
    let decode = |text: &str| Operation::StateSubscribe.decode_args(Some(&args_of(text)));
    assert!(decode(r#"{"topics":["desktop"]}"#).is_ok());
    let sixteen: Vec<String> = (0..16).map(|index| format!("t{index}")).collect();
    let seventeen: Vec<String> = (0..17).map(|index| format!("t{index}")).collect();
    assert!(decode(&serde_json::json!({ "topics": sixteen }).to_string()).is_ok());
    assert!(decode(&serde_json::json!({ "topics": seventeen }).to_string()).is_err());
    for bad in [
        r#"{}"#,
        r#"{"topics":[]}"#,
        r#"{"topics":["desktop","desktop"]}"#,
        r#"{"topics":[1]}"#,
        r#"{"topics":"desktop"}"#,
        r#"{"topics":null}"#,
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
}

#[test]
fn workspace_number_range_and_types() {
    let decode = |text: &str| Operation::WorkspaceFocus.decode_args(Some(&args_of(text)));
    let number = |number| Ok(Args::WorkspaceFocus(WorkspaceTarget::Number(number)));
    assert_eq!(decode(r#"{"number":0}"#), number(0));
    assert_eq!(decode(r#"{"number":999}"#), number(999));
    for bad in [
        r#"{"number":1000}"#,
        r#"{"number":-1}"#,
        r#"{"number":2.0}"#,
        r#"{"number":1e2}"#,
        r#"{"number":"2"}"#,
        r#"{"number":null}"#,
        r#"{"number":2,"workspace_id":null}"#,
        r#"{"workspace_id":9007199254740992}"#,
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
}

#[test]
fn icon_hashes_count_duplicates_and_form() {
    let decode = |hashes: Value| {
        Operation::IconGet.decode_args(Some(&serde_json::json!({ "hashes": hashes })))
    };
    let hash = |index: u8| IconHash::of(&[index]).to_string();
    let eight: Vec<String> = (0..8).map(hash).collect();
    let nine: Vec<String> = (0..9).map(hash).collect();
    assert!(decode(eight.into()).is_ok());
    let refused = [
        nine.into(),
        Value::Array(Vec::new()),
        serde_json::json!([hash(1), hash(1)]),
        serde_json::json!(["firefox"]),
        serde_json::json!([null]),
        Value::Null,
        hash(1).into(),
    ];
    for bad in refused {
        assert_eq!(decode(bad.clone()), Err(InvalidMember("hashes")), "{bad}");
    }
    assert!(Operation::IconGet.decode_args(None).is_err());
}

#[test]
fn fullscreen_needs_a_real_boolean() {
    let decode = |text: &str| Operation::WindowFullscreen.decode_args(Some(&args_of(text)));
    assert!(decode(r#"{"window_id":5,"enabled":false}"#).is_ok());
    assert!(decode(r#"{"window_id":5,"enabled":0}"#).is_err());
    assert!(decode(r#"{"window_id":5}"#).is_err());
}

#[test]
fn every_args_value_survives_request_encoding() {
    let all = [
        Args::Ping,
        Args::StateSubscribe {
            topics: vec![TOPIC_DESKTOP.to_owned(), TOPIC_SESSION.to_owned()],
        },
        Args::IconGet {
            hashes: vec![IconHash::of(b"a"), IconHash::of(b"b")],
        },
        Args::WorkspaceFocus(WorkspaceTarget::Id(9)),
        Args::WorkspaceFocus(WorkspaceTarget::Number(2)),
        Args::WindowFocus { window_id: 5 },
        Args::WindowFullscreen {
            window_id: 5,
            enabled: true,
        },
        Args::WindowMove {
            window_id: 41,
            target: WorkspaceTarget::Id(31),
        },
        Args::WindowMove {
            window_id: 41,
            target: WorkspaceTarget::Number(0),
        },
        Args::WindowFloating {
            window_id: 46,
            enabled: false,
        },
        Args::WindowLayout {
            window_id: 43,
            layout: ContainerLayout::Stacking,
        },
        Args::WindowScratchpad { window_id: 45 },
        Args::ScratchpadShow { window_id: 50 },
        Args::SessionLock,
        Args::WindowClose { window_id: 41 },
        Args::KeepAwakeSet(KeepAwake::On { duration_s: None }),
        Args::KeepAwakeSet(KeepAwake::On {
            duration_s: Some(60),
        }),
        Args::KeepAwakeSet(KeepAwake::Off),
        Args::Audio {
            device_id: 1,
            change: AudioChange::Volume(150),
        },
        Args::Audio {
            device_id: 2,
            change: AudioChange::Mute(false),
        },
        Args::Audio {
            device_id: 3,
            change: AudioChange::Default,
        },
        Args::Media {
            player_id: 1,
            command: MediaCommand::PlayPause,
        },
        Args::Media {
            player_id: 1,
            command: MediaCommand::Next,
        },
        Args::Media {
            player_id: 1,
            command: MediaCommand::Previous,
        },
        Args::AppLaunch {
            entry_id: EntryId::parse("org.gnome.Nautilus").expect("entry id"),
            number: None,
        },
        Args::AppLaunch {
            entry_id: EntryId::parse("firefox").expect("entry id"),
            number: Some(0),
        },
        Args::LauncherOpen,
        Args::InputText {
            target: InputTarget::Window(42),
            text: InputText::parse("git status").expect("text"),
        },
        Args::InputText {
            target: InputTarget::Launcher,
            text: InputText::parse("fire").expect("text"),
        },
        Args::InputKey {
            target: InputTarget::Window(42),
            press: KeyPress::new(Key::from_wire("Return").expect("key"), Vec::new(), 1)
                .expect("press"),
        },
        Args::InputKey {
            target: InputTarget::Launcher,
            press: KeyPress::new(
                Key::from_wire("t").expect("key"),
                vec![Modifier::Shift, Modifier::Ctrl],
                32,
            )
            .expect("press"),
        },
    ];
    for args in all {
        let req = args.to_req(1);
        let operation = Operation::from_wire(&req.op).expect("known operation");
        assert_eq!(operation.decode_args(req.args.as_ref()), Ok(args));
    }
}

#[test]
fn window_move_takes_exactly_one_workspace_target() {
    let decode = |text: &str| Operation::WindowMove.decode_args(Some(&args_of(text)));
    assert!(decode(r#"{"window_id":41,"number":999}"#).is_ok());
    assert!(decode(r#"{"window_id":41,"workspace_id":31}"#).is_ok());
    for bad in [
        r#"{"window_id":41}"#,
        r#"{"window_id":41,"number":2,"workspace_id":31}"#,
        r#"{"window_id":41,"number":1000}"#,
        r#"{"window_id":41,"workspace":"music"}"#,
        r#"{"number":2}"#,
        r#"{"window_id":null,"number":2}"#,
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
}

#[test]
fn window_layout_is_a_closed_enum_matched_exactly() {
    let decode = |layout: Value| {
        Operation::WindowLayout.decode_args(Some(&serde_json::json!({
            "window_id": 43, "layout": layout
        })))
    };
    for layout in [
        ContainerLayout::SplitH,
        ContainerLayout::SplitV,
        ContainerLayout::Tabbed,
        ContainerLayout::Stacking,
    ] {
        let expected = Args::WindowLayout {
            window_id: 43,
            layout,
        };
        assert_eq!(decode(layout.as_str().into()), Ok(expected));
    }
    for bad in [
        "Tabbed",
        "tabbed ",
        "default",
        "toggle split",
        "tabbed; exec true",
        "t",
        "",
    ] {
        assert_eq!(decode(bad.into()), Err(InvalidMember("layout")), "{bad:?}");
    }
    assert!(decode(Value::Null).is_err());
}

#[test]
fn keep_awake_duration_range_and_types() {
    let decode = |text: &str| Operation::KeepAwakeSet.decode_args(Some(&args_of(text)));
    let on = |duration_s| Ok(Args::KeepAwakeSet(KeepAwake::On { duration_s }));
    assert_eq!(decode(r#"{"enabled":true}"#), on(None));
    assert_eq!(decode(r#"{"enabled":true,"duration_s":60}"#), on(Some(60)));
    for bad in [
        r#"{"enabled":true,"duration_s":59}"#,
        r#"{"enabled":true,"duration_s":"3600"}"#,
        r#"{"enabled":true,"duration_s":null}"#,
        r#"{"enabled":true,"duration_s":3600.0}"#,
        r#"{"enabled":true,"duration_s":3.6e3}"#,
        r#"{"enabled":true,"duration_s":9007199254740992}"#,
        r#"{"duration_s":3600}"#,
        r#"{"enabled":null}"#,
        r#"{"enabled":"on"}"#,
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
    assert!(Operation::KeepAwakeSet.decode_args(None).is_err());
}

#[test]
fn keep_awake_off_ignores_the_duration() {
    let decode = |text: &str| Operation::KeepAwakeSet.decode_args(Some(&args_of(text)));
    let off = Ok(Args::KeepAwakeSet(KeepAwake::Off));
    assert_eq!(decode(r#"{"enabled":false}"#), off);
    assert_eq!(decode(r#"{"enabled":false,"duration_s":5}"#), off);
    assert_eq!(decode(r#"{"enabled":false,"duration_s":"x"}"#), off);
}

#[test]
fn keep_awake_maximum_is_the_servers() {
    let on = |duration_s| KeepAwake::On { duration_s };
    assert_eq!(on(Some(7200)).check_max_duration(7200), Ok(on(Some(7200))));
    assert_eq!(on(None).check_max_duration(60), Ok(on(None)));
    assert_eq!(KeepAwake::Off.check_max_duration(60), Ok(KeepAwake::Off));
    assert_eq!(
        on(Some(7201)).check_max_duration(7200),
        Err(InvalidMember("duration_s"))
    );
}

#[test]
fn session_lock_takes_no_arguments_but_needs_an_object() {
    let decode = |args: Option<Value>| Operation::SessionLock.decode_args(args.as_ref());
    assert_eq!(decode(None), Ok(Args::SessionLock));
    assert_eq!(
        decode(Some(
            serde_json::json!({"command": "loginctl lock-session"})
        )),
        Ok(Args::SessionLock)
    );
    assert!(decode(Some(Value::Null)).is_err());
    assert!(decode(Some("now".into())).is_err());
}

#[test]
fn scopes_and_actions_follow_the_operation_table() {
    let reads = [
        (Operation::Ping, None, None),
        (Operation::StateSubscribe, Some(Scope::StateRead), None),
        (
            Operation::IconGet,
            Some(Scope::StateRead),
            Some(RateClass::Icons),
        ),
    ];
    for (read, scope, class) in reads {
        assert_eq!(read.required_scope(), scope, "{read:?}");
        assert_eq!(read.rate_class(), class, "{read:?}");
        assert!(!read.is_action(), "{read:?}");
    }
    let actions = [
        (Operation::WorkspaceFocus, Scope::Control),
        (Operation::WindowFocus, Scope::Control),
        (Operation::WindowFullscreen, Scope::Control),
        (Operation::WindowMove, Scope::Control),
        (Operation::WindowFloating, Scope::Control),
        (Operation::WindowLayout, Scope::Control),
        (Operation::WindowScratchpad, Scope::Control),
        (Operation::ScratchpadShow, Scope::Control),
        (Operation::SessionLock, Scope::Control),
        (Operation::WindowClose, Scope::Destructive),
        (Operation::KeepAwakeSet, Scope::IdleInhibit),
        (Operation::AudioSetMute, Scope::Audio),
        (Operation::AudioSetDefault, Scope::Audio),
        (Operation::MediaPlayPause, Scope::Media),
        (Operation::MediaNext, Scope::Media),
        (Operation::MediaPrevious, Scope::Media),
    ];
    for (action, scope) in actions {
        assert_eq!(action.required_scope(), Some(scope), "{action:?}");
        assert_eq!(action.rate_class(), Some(RateClass::Actions), "{action:?}");
        assert!(action.is_action(), "{action:?}");
    }
    let own_classes = [
        (
            Operation::AudioSetVolume,
            Scope::Audio,
            RateClass::Continuous,
        ),
        (Operation::AppLaunch, Scope::Launch, RateClass::Launch),
        (Operation::LauncherOpen, Scope::Launch, RateClass::Launch),
        (Operation::InputKey, Scope::InputKeys, RateClass::Input),
        (Operation::InputText, Scope::InputText, RateClass::InputText),
    ];
    for (action, scope, class) in own_classes {
        assert_eq!(action.required_scope(), Some(scope), "{action:?}");
        assert_eq!(action.rate_class(), Some(class), "{action:?}");
        assert!(action.is_action(), "{action:?}");
    }
}

#[test]
fn topic_scopes_follow_17_2() {
    assert_eq!(topic_scope(TOPIC_AUDIO), Some(Scope::Audio));
    assert_eq!(topic_scope(TOPIC_MEDIA), Some(Scope::Media));
    assert_eq!(topic_scope(TOPIC_APPS), Some(Scope::Launch));
    for topic in [TOPIC_DESKTOP, TOPIC_SESSION, "Audio", "nope"] {
        assert_eq!(topic_scope(topic), None, "{topic}");
    }
}

#[test]
fn volume_is_never_above_150_on_the_wire() {
    let decode = |volume: Value| {
        Operation::AudioSetVolume.decode_args(Some(&serde_json::json!({
            "device_id": 1, "volume_pct": volume
        })))
    };
    assert!(decode(150.into()).is_ok());
    assert!(decode(0.into()).is_ok());
    for bad in [
        151.into(),
        (-1).into(),
        "50".into(),
        Value::Null,
        serde_json::json!(50.5),
    ] {
        assert_eq!(
            decode(bad.clone()),
            Err(InvalidMember("volume_pct")),
            "{bad}"
        );
    }
}

#[test]
fn launch_number_is_optional_but_never_null() {
    let decode = |text: &str| Operation::AppLaunch.decode_args(Some(&args_of(text)));
    assert!(decode(r#"{"entry_id":"mpv","number":999}"#).is_ok());
    for bad in [
        r#"{"entry_id":"mpv","number":1000}"#,
        r#"{"entry_id":"mpv","number":null}"#,
        r#"{"entry_id":"mpv","number":"3"}"#,
        r#"{"entry_id":"../x"}"#,
        r#"{"number":3}"#,
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
}
