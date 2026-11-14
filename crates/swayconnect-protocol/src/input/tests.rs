use super::*;
use crate::ops::{Args, Operation};

fn args_of(value: Value) -> Option<Value> {
    Some(value)
}

fn key(name: &str) -> Key {
    Key::from_wire(name).expect("known key")
}

#[test]
fn key_names_are_a_closed_list_matched_exactly() {
    let all: Vec<&str> = NAMED_KEYS
        .iter()
        .chain(&FUNCTION_KEYS)
        .copied()
        .chain(
            CHARACTER_KEYS
                .char_indices()
                .map(|(index, _)| &CHARACTER_KEYS[index..=index]),
        )
        .collect();
    assert_eq!(all.len(), 63);
    for name in all {
        assert_eq!(Key::from_wire(name).map(Key::as_str), Some(name));
    }
    for bad in [
        "",
        "A",
        "return",
        "Enter",
        "F0",
        "F13",
        "F01",
        "ab",
        "\u{e9}",
        " ",
        "Escape ",
        "KP_Enter",
        "Super_L",
        "ctrl",
        "0xff0d",
        "XF86Switch_VT_1",
        "XF86Ungrab",
        "Terminate_Server",
    ] {
        assert_eq!(Key::from_wire(bad), None, "{bad:?}");
    }
    assert!(key("F12").is_function_key());
    assert!(!key("1").is_function_key());
}

#[test]
fn ctrl_alt_function_key_is_refused_in_any_order_and_with_more_modifiers() {
    use Modifier::{Alt, Ctrl, Shift, Super};
    for function_key in FUNCTION_KEYS {
        for modifiers in [
            vec![Ctrl, Alt],
            vec![Alt, Ctrl],
            vec![Ctrl, Alt, Shift],
            vec![Super, Ctrl, Alt],
        ] {
            assert_eq!(
                KeyPress::new(key(function_key), modifiers.clone(), 1),
                Err(InvalidMember(MODIFIERS)),
                "{function_key} {modifiers:?}"
            );
        }
    }
    for (name, modifiers) in [
        ("Delete", vec![Ctrl, Alt]),
        ("2", vec![Ctrl, Alt]),
        ("F2", vec![Ctrl]),
        ("F2", vec![Alt, Shift, Super]),
    ] {
        assert!(KeyPress::new(key(name), modifiers, 1).is_ok(), "{name}");
    }
}

#[test]
fn key_press_repeat_and_duplicates() {
    let press = |modifiers, repeat| KeyPress::new(key("a"), modifiers, repeat);
    assert!(press(Vec::new(), 1).is_ok());
    assert!(press(Vec::new(), 32).is_ok());
    assert_eq!(press(Vec::new(), 0), Err(InvalidMember(REPEAT)));
    assert_eq!(press(Vec::new(), 33), Err(InvalidMember(REPEAT)));
    assert_eq!(
        press(vec![Modifier::Ctrl, Modifier::Ctrl], 1),
        Err(InvalidMember(MODIFIERS))
    );
}

#[test]
fn key_args_absent_members_take_their_defaults_and_null_is_refused() {
    let decode = |value| Operation::InputKey.decode_args(args_of(value).as_ref());
    let Ok(Args::InputKey { target, press }) =
        decode(serde_json::json!({"window_id": 42, "key": "a"}))
    else {
        panic!("decodes");
    };
    assert_eq!(target, InputTarget::Window(42));
    assert_eq!((press.modifiers(), press.repeat()), (&[][..], 1));
    for bad in [
        serde_json::json!({"window_id": 42, "key": "a", "modifiers": null}),
        serde_json::json!({"window_id": 42, "key": "a", "repeat": null}),
        serde_json::json!({"window_id": 42, "key": "a", "modifiers": ["meta"]}),
        serde_json::json!({"window_id": 42, "key": "a", "modifiers": "ctrl"}),
    ] {
        assert!(decode(bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn target_is_exactly_one_of_window_and_launcher_true() {
    let target = |value: Value| InputTarget::decode(value.as_object().expect("object"));
    assert_eq!(
        target(serde_json::json!({"window_id": 7})),
        Ok(InputTarget::Window(7))
    );
    assert_eq!(
        target(serde_json::json!({"launcher": true})),
        Ok(InputTarget::Launcher)
    );
    for bad in [
        serde_json::json!({}),
        serde_json::json!({"window_id": 7, "launcher": true}),
        serde_json::json!({"launcher": false}),
        serde_json::json!({"launcher": "true"}),
        serde_json::json!({"launcher": null}),
        serde_json::json!({"window_id": null}),
        serde_json::json!({"window_id": "7"}),
    ] {
        assert!(target(bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn parts_hold_at_most_sixteen_scalar_values() {
    let parts = |text: &str| -> Vec<String> {
        let text = InputText::parse(text).expect("valid text");
        text.parts().map(str::to_owned).collect()
    };
    assert_eq!(parts("git status --short"), ["git status --sho", "rt"]);
    assert_eq!(parts(&"a".repeat(16)), ["a".repeat(16)]);
    assert_eq!(parts(&"a".repeat(17)), ["a".repeat(16), "a".to_owned()]);
    let emoji = "\u{1F469}\u{200D}\u{1F4BB}".repeat(6);
    let split = parts(&emoji);
    assert_eq!(split.len(), 2);
    assert_eq!(split[0].chars().count(), 16);
    assert!(split[0].ends_with('\u{1F469}'), "cuts inside the sequence");
    assert_eq!(split.concat(), emoji);
}

#[test]
fn debug_output_of_text_has_no_text() {
    let text = InputText::parse("hunter2").expect("valid text");
    let shown = format!("{text:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
    assert!(shown.contains('7'), "{shown}");
}

#[test]
fn refusals_round_trip_through_an_error_body() {
    let refusals = [
        InputRefusal::Locked { delivered_chars: 0 },
        InputRefusal::LauncherClosed { delivered_chars: 3 },
        InputRefusal::KeyboardGrabbed {
            delivered_chars: 32,
        },
        InputRefusal::FocusChanged {
            delivered_chars: 16,
            focused_window_id: Some(24),
        },
        InputRefusal::FocusChanged {
            delivered_chars: 0,
            focused_window_id: None,
        },
    ];
    for refusal in refusals {
        let body = refusal.error_body("text");
        assert_eq!(body.code, refusal.code());
        assert_eq!(InputRefusal::decode(&body), Ok(Some(refusal)));
    }
}

#[test]
fn refusal_detail_is_required_for_the_input_codes_only() {
    let body = |code, detail: Option<Value>| ErrorBody {
        code,
        message: String::new(),
        detail: detail.map(|detail| detail.as_object().expect("object").clone()),
    };
    assert_eq!(
        InputRefusal::decode(&body(ErrorCode::NotFound, None)),
        Ok(None)
    );
    assert!(InputRefusal::decode(&body(ErrorCode::Locked, None)).is_err());
    let no_focus = serde_json::json!({"delivered_chars": 1});
    assert_eq!(
        InputRefusal::decode(&body(ErrorCode::FocusChanged, Some(no_focus))),
        Err(InvalidMember(FOCUSED_WINDOW_ID))
    );
}
