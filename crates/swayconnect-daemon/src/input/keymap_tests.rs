use std::collections::HashMap;

use swayconnect_protocol::{Key, KeyPress, Modifier};
use xkeysym::Keysym;

use super::*;

const VT_SWITCHES: std::ops::RangeInclusive<u32> = key::XF86_Switch_VT_1..=key::XF86_Switch_VT_12;
const FORBIDDEN: [u32; 2] = [key::XF86_Ungrab, key::Terminate_Server];
/// Codes text must never use (`docs/DESIGN.md` 11.7 item 2).
const PROTECTED_NAMES: [&str; 8] = [
    "Escape",
    "BackSpace",
    "Tab",
    "Return",
    "Shift_L",
    "Control_L",
    "Alt_L",
    "Super_L",
];

/// The keysym xkbcommon resolves `unicode_name(character)` to (see the module docs).
fn unicode_keysym(character: char) -> u32 {
    let code_point = u32::from(character);
    if matches!(code_point, 0x20..=0x7e | 0xa0..=0xff) {
        code_point
    } else {
        0x0100_0000 | code_point
    }
}

/// `<Cnnn>` -> keysym name, from the `xkb_symbols` lines.
fn symbols(keymap: &Keymap) -> HashMap<u32, String> {
    keymap
        .as_str()
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("key <C")?;
            let (code, rest) = rest.split_once("> { [ ")?;
            let name = rest.strip_suffix(" ] };")?;
            assert!(
                !name.contains(',') && !name.contains(' '),
                "more than one level: {line}"
            );
            Some((code.parse().expect("key code"), name.to_owned()))
        })
        .collect()
}

/// The keysym xkbcommon gives `name` in these keymaps.
fn keysym_of(name: &str) -> u32 {
    let unicode = name
        .strip_prefix('U')
        .filter(|hex| !hex.is_empty())
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .and_then(char::from_u32);
    if let Some(character) = unicode {
        return unicode_keysym(character);
    }
    NAMED_KEYS
        .iter()
        .chain(MODIFIER_KEYS.iter().map(|modifier| &modifier.key))
        .find(|key| key.name == name)
        .unwrap_or_else(|| panic!("unknown keysym name {name}"))
        .keysym
}

/// What a client types from `delivery`: each press resolved through the keymap and
/// translated as `xkb_keysym_to_utf32` does.
fn typed(delivery: &Delivery) -> String {
    let symbols = symbols(&delivery.keymap);
    delivery
        .events
        .iter()
        .filter_map(|event| match event {
            KeyEvent::Press(code) => Some(*code),
            _ => None,
        })
        .map(|code| {
            let name = &symbols[&(code + XKB_OFFSET)];
            Keysym::new(keysym_of(name))
                .key_char()
                .expect("a character keysym")
        })
        .collect()
}

fn assert_safe(keymap: &Keymap) {
    let symbols = symbols(keymap);
    let declared = keymap
        .as_str()
        .lines()
        .filter(|line| line.starts_with("<C"))
        .count();
    assert_eq!(
        symbols.len(),
        declared,
        "every key code has one symbol line"
    );
    for name in symbols.values() {
        let keysym = keysym_of(name);
        assert!(!VT_SWITCHES.contains(&keysym), "{name} is a VT switch");
        assert!(!FORBIDDEN.contains(&keysym), "{name} is forbidden");
    }
    let text = keymap.as_str();
    for word in [
        "type=",
        "type =",
        "action",
        "XF86",
        "Terminate",
        "Ungrab",
        "Switch",
        "Private",
    ] {
        assert!(!text.contains(word), "keymap mentions {word}");
    }
    assert_eq!(text.matches("include").count(), 2);
}

#[test]
fn text_of_every_script_types_back_exactly() {
    for part in [
        "git status --sho",
        "Grüße, café",
        "Привіт, світ",
        "😀",
        "👩\u{200d}💻 ok",
        "1\u{fe0f}\u{20e3} 👍🏽",
        "می\u{200c}خواهم",
        "rm -rf ~; $(id) ",
    ] {
        let delivery = text_part(part).expect(part);
        assert_eq!(typed(&delivery), part);
        assert_safe(&delivery.keymap);
    }
}

#[test]
fn latin_1_is_direct_and_everything_else_is_unicode_offset() {
    assert_eq!(unicode_keysym('a'), 0x61);
    assert_eq!(unicode_keysym('ü'), 0xfc);
    assert_eq!(unicode_keysym('\u{a0}'), 0xa0);
    assert_eq!(unicode_keysym('ж'), 0x0100_0436);
    assert_eq!(unicode_keysym('😀'), 0x0101_f600);
    assert_eq!(unicode_keysym('\u{200d}'), 0x0100_200d);
}

#[test]
fn text_never_uses_a_named_key_or_modifier_code() {
    let protected: Vec<u32> = NAMED_KEYS
        .iter()
        .chain(MODIFIER_KEYS.iter().map(|modifier| &modifier.key))
        .filter(|key| PROTECTED_NAMES.contains(&key.name))
        .map(|key| key.evdev)
        .collect();
    assert_eq!(protected.len(), PROTECTED_NAMES.len());
    let fixed_codes: Vec<u32> = NAMED_KEYS
        .iter()
        .map(|key| key.evdev)
        .chain(CHARACTER_KEYS.iter().map(|(_, evdev)| *evdev))
        .chain(MODIFIER_KEYS.iter().map(|modifier| modifier.key.evdev))
        .collect();

    let delivery = text_part("abcdefghijklmnop").expect("16 distinct");
    for event in &delivery.events {
        let (KeyEvent::Press(code) | KeyEvent::Release(code)) = *event else {
            panic!("text sets no modifiers");
        };
        assert!((FIRST_TEXT_CODE..FIRST_TEXT_CODE + TEXT_CODES).contains(&code));
        assert!(!fixed_codes.contains(&code));
    }
    for character in ['\u{1b}', '\u{8}', '\t', '\r', '\n', '\u{7f}', '\u{9b}'] {
        assert_eq!(
            text_part(&character.to_string()),
            Err(KeymapError::ControlCharacter)
        );
    }
    for keysym in "a é ж 😀".chars().map(unicode_keysym) {
        assert!(!(0xff00..=0xffff).contains(&keysym));
        assert!(!Keysym::new(keysym).is_modifier_key());
    }
}

#[test]
fn no_key_resolves_to_a_vt_switch_ungrab_or_terminate() {
    // Characters whose code point equals the low bits of a forbidden keysym.
    let near_misses: String = [0xfe01, 0xfe0c, 0xfe20, 0xfed5, 0x8fe0]
        .into_iter()
        .filter_map(char::from_u32)
        .collect();
    for part in [near_misses.as_str(), "", "x"] {
        let delivery = text_part(part).expect("typable");
        assert_safe(&delivery.keymap);
    }
    let press =
        KeyPress::new(Key::from_wire("F1").expect("F1"), vec![Modifier::Ctrl], 1).expect("ctrl+F1");
    assert_safe(&key_press(&press).expect("mapped").keymap);
}

#[test]
fn parts_hold_at_most_sixteen_distinct_characters() {
    let delivery = text_part("aaaaaaaaaaaaaaaa").expect("one distinct character");
    assert_eq!(delivery.events.len(), 32);
    assert_eq!(
        text_part("abcdefghijklmnopq"),
        Err(KeymapError::TooManyCharacters)
    );
}

#[test]
fn every_key_of_the_protocol_has_its_real_code() {
    let named = [
        "Escape",
        "Tab",
        "Return",
        "BackSpace",
        "Delete",
        "Insert",
        "Left",
        "Right",
        "Up",
        "Down",
        "Home",
        "End",
        "Page_Up",
        "Page_Down",
        "space",
    ];
    let names: Vec<String> = named
        .into_iter()
        .map(str::to_owned)
        .chain((1..=12).map(|n| format!("F{n}")))
        .chain(CHARACTER_KEYS.iter().map(|(c, _)| c.to_string()))
        .collect();
    for name in &names {
        let key = Key::from_wire(name).unwrap_or_else(|| panic!("{name}"));
        let press = KeyPress::new(key, Vec::new(), 1).expect("plain press");
        let delivery = key_press(&press).expect(name);
        let [KeyEvent::Press(code), KeyEvent::Release(released)] = delivery.events[..] else {
            panic!("{name}: one press and release");
        };
        assert_eq!(code, released);
        let symbol = &symbols(&delivery.keymap)[&(code + XKB_OFFSET)];
        let expected = if name.len() == 1 {
            unicode_name(name.chars().next().expect("one char"))
        } else {
            name.clone()
        };
        assert_eq!(symbol, &expected);
    }
    assert_eq!(
        key_code(Key::from_wire("Return").expect("Return")),
        Some(28)
    );
    assert_eq!(key_code(Key::from_wire("a").expect("a")), Some(30));
}

#[test]
fn modifiers_wrap_every_repeat_and_end_released() {
    let press = KeyPress::new(
        Key::from_wire("t").expect("t"),
        vec![Modifier::Ctrl, Modifier::Shift],
        3,
    )
    .expect("ctrl+shift+t");
    let delivery = key_press(&press).expect("mapped");
    let t = 20;
    assert_eq!(
        delivery.events,
        [
            KeyEvent::Modifiers(0b101),
            KeyEvent::Press(t),
            KeyEvent::Release(t),
            KeyEvent::Press(t),
            KeyEvent::Release(t),
            KeyEvent::Press(t),
            KeyEvent::Release(t),
            KeyEvent::Modifiers(0),
        ]
    );
    let every = KeyPress::new(
        Key::from_wire("a").expect("a"),
        vec![
            Modifier::Ctrl,
            Modifier::Alt,
            Modifier::Shift,
            Modifier::Super,
        ],
        1,
    )
    .expect("every modifier");
    assert_eq!(
        key_press(&every).expect("mapped").events[0],
        KeyEvent::Modifiers(0b100_1101)
    );
}

#[test]
fn modifier_keys_map_to_their_real_modifiers_only() {
    let keymap =
        key_press(&KeyPress::new(Key::from_wire("a").expect("a"), Vec::new(), 1).expect("press"))
            .expect("mapped")
            .keymap;
    let maps: Vec<&str> = keymap
        .as_str()
        .lines()
        .filter(|line| line.starts_with("modifier_map"))
        .collect();
    assert_eq!(
        maps,
        [
            "modifier_map Shift { <C50> };",
            "modifier_map Control { <C37> };",
            "modifier_map Mod1 { <C64> };",
            "modifier_map Mod4 { <C133> };",
        ]
    );
}

#[test]
fn debug_output_never_shows_the_text() {
    let delivery = text_part("hunter2").expect("typable");
    let shown = format!("{delivery:?}");
    assert!(!shown.contains("U0068"), "{shown}");
    assert!(!shown.contains("hunter"), "{shown}");
    assert!(shown.contains("bytes"), "{shown}");
}
