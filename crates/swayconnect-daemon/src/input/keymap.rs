//! Pure: the generated XKB keymap and the key events of one part (`docs/DESIGN.md` 11.7
//! item 2), after wtype's method.
//!
//! Every keymap holds the keys of `input.key` and the four modifiers at their real evdev
//! codes, each with one keysym on one level. Each distinct character of a text part gets its
//! own key from a separate block of 16 codes, so text never lands on Escape, BackSpace, Tab,
//! Return or a modifier. A character is written as `U<hex>`; xkbcommon resolves that name to
//! the Latin-1 keysym for U+0020..U+007E and U+00A0..U+00FF and to `0x01000000 | cp` for the
//! rest. No keysym of a generated keymap is a VT switch, `XF86Ungrab` or `Terminate_Server`,
//! and with one level no modifier can reach another keysym.
//!
//! The keymap of a text part names its characters, so it is never logged: its `Debug` gives
//! the length only.

use std::fmt::{self, Write as _};

use swayconnect_protocol::limits::MAX_INPUT_PART_SCALARS;
use swayconnect_protocol::{Key, KeyPress, Modifier};
use xkeysym::key;

/// Wayland key events carry evdev codes; XKB keycodes are 8 higher.
const XKB_OFFSET: u32 = 8;
/// Evdev codes for the characters of a text part: `KEY_F13`..`KEY_F24` and four unassigned
/// codes. All are below 248, so Xwayland clients see them too.
const FIRST_TEXT_CODE: u32 = 183;
const TEXT_CODES: u32 = 16;
const _: () = assert!(TEXT_CODES as usize == MAX_INPUT_PART_SCALARS);

/// A key at its real evdev code with one keysym.
#[derive(Debug, Clone, Copy)]
struct FixedKey {
    evdev: u32,
    /// The X keysym name written into the keymap.
    name: &'static str,
    /// The value of `name`; tests check it against the forbidden keysyms.
    #[cfg_attr(not(test), expect(dead_code, reason = "only tests read the value"))]
    keysym: u32,
}

const fn fixed(evdev: u32, name: &'static str, keysym: u32) -> FixedKey {
    FixedKey {
        evdev,
        name,
        keysym,
    }
}

/// The named keys of `input.key` (`docs/PROTOCOL.md` 17.13).
const NAMED_KEYS: [FixedKey; 27] = [
    fixed(1, "Escape", key::Escape),
    fixed(15, "Tab", key::Tab),
    fixed(28, "Return", key::Return),
    fixed(14, "BackSpace", key::BackSpace),
    fixed(111, "Delete", key::Delete),
    fixed(110, "Insert", key::Insert),
    fixed(105, "Left", key::Left),
    fixed(106, "Right", key::Right),
    fixed(103, "Up", key::Up),
    fixed(108, "Down", key::Down),
    fixed(102, "Home", key::Home),
    fixed(107, "End", key::End),
    fixed(104, "Page_Up", key::Page_Up),
    fixed(109, "Page_Down", key::Page_Down),
    fixed(57, "space", key::space),
    fixed(59, "F1", key::F1),
    fixed(60, "F2", key::F2),
    fixed(61, "F3", key::F3),
    fixed(62, "F4", key::F4),
    fixed(63, "F5", key::F5),
    fixed(64, "F6", key::F6),
    fixed(65, "F7", key::F7),
    fixed(66, "F8", key::F8),
    fixed(67, "F9", key::F9),
    fixed(68, "F10", key::F10),
    fixed(87, "F11", key::F11),
    fixed(88, "F12", key::F12),
];

/// The letter and digit keys of `input.key` with their evdev codes (US layout positions).
const CHARACTER_KEYS: [(char, u32); 36] = [
    ('a', 30),
    ('b', 48),
    ('c', 46),
    ('d', 32),
    ('e', 18),
    ('f', 33),
    ('g', 34),
    ('h', 35),
    ('i', 23),
    ('j', 36),
    ('k', 37),
    ('l', 38),
    ('m', 50),
    ('n', 49),
    ('o', 24),
    ('p', 25),
    ('q', 16),
    ('r', 19),
    ('s', 31),
    ('t', 20),
    ('u', 22),
    ('v', 47),
    ('w', 17),
    ('x', 45),
    ('y', 21),
    ('z', 44),
    ('1', 2),
    ('2', 3),
    ('3', 4),
    ('4', 5),
    ('5', 6),
    ('6', 7),
    ('7', 8),
    ('8', 9),
    ('9', 10),
    ('0', 11),
];

/// A modifier key, the real modifier it maps to, and that modifier's bit in the mask of
/// `zwp_virtual_keyboard_v1.modifiers`.
struct ModifierKey {
    modifier: Modifier,
    key: FixedKey,
    real: &'static str,
    mask: u32,
}

const MODIFIER_KEYS: [ModifierKey; 4] = [
    ModifierKey {
        modifier: Modifier::Shift,
        key: fixed(42, "Shift_L", key::Shift_L),
        real: "Shift",
        mask: 1 << 0,
    },
    ModifierKey {
        modifier: Modifier::Ctrl,
        key: fixed(29, "Control_L", key::Control_L),
        real: "Control",
        mask: 1 << 2,
    },
    ModifierKey {
        modifier: Modifier::Alt,
        key: fixed(56, "Alt_L", key::Alt_L),
        real: "Mod1",
        mask: 1 << 3,
    },
    ModifierKey {
        modifier: Modifier::Super,
        key: fixed(125, "Super_L", key::Super_L),
        real: "Mod4",
        mask: 1 << 6,
    },
];

/// One event for the virtual keyboard. Codes are evdev codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Press(u32),
    Release(u32),
    /// The depressed real-modifier mask.
    Modifiers(u32),
}

/// An XKB keymap in text form (`XKB_V1`).
#[derive(Clone, PartialEq, Eq)]
pub struct Keymap(String);

impl Keymap {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Keymap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Keymap")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// One part for the keyboard: the keymap the events refer to, then the events. The events
/// end with every key released and no modifier held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub keymap: Keymap,
    pub events: Vec<KeyEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("a text part holds a control character")]
    ControlCharacter,
    #[error("a text part holds more than {TEXT_CODES} distinct characters")]
    TooManyCharacters,
    #[error("key {0:?} has no key code")]
    UnknownKey(&'static str),
}

/// The keymap and events that type `part`: one press and release per character.
pub fn text_part(part: &str) -> Result<Delivery, KeymapError> {
    let mut distinct: Vec<char> = Vec::new();
    let mut events = Vec::with_capacity(part.len() * 2);
    for character in part.chars() {
        if character.is_control() {
            return Err(KeymapError::ControlCharacter);
        }
        let index = match distinct.iter().position(|known| *known == character) {
            Some(index) => index,
            None if distinct.len() < TEXT_CODES as usize => {
                distinct.push(character);
                distinct.len() - 1
            }
            None => return Err(KeymapError::TooManyCharacters),
        };
        let code = FIRST_TEXT_CODE + index as u32;
        events.extend([KeyEvent::Press(code), KeyEvent::Release(code)]);
    }
    Ok(Delivery {
        keymap: keymap(&distinct),
        events,
    })
}

/// The keymap and events of one `input.key`: the modifiers, `repeat` presses and releases,
/// then no modifier.
pub fn key_press(press: &KeyPress) -> Result<Delivery, KeymapError> {
    let code = key_code(press.key()).ok_or(KeymapError::UnknownKey(press.key().as_str()))?;
    let mask = press
        .modifiers()
        .iter()
        .map(|modifier| modifier_key(*modifier).mask)
        .fold(0, |mask, bit| mask | bit);
    let presses =
        (0..press.repeat()).flat_map(|_| [KeyEvent::Press(code), KeyEvent::Release(code)]);
    let events = if mask == 0 {
        presses.collect()
    } else {
        std::iter::once(KeyEvent::Modifiers(mask))
            .chain(presses)
            .chain(std::iter::once(KeyEvent::Modifiers(0)))
            .collect()
    };
    Ok(Delivery {
        keymap: keymap(&[]),
        events,
    })
}

fn key_code(key: Key) -> Option<u32> {
    let name = key.as_str();
    if let Some(named) = NAMED_KEYS.iter().find(|named| named.name == name) {
        return Some(named.evdev);
    }
    let mut characters = name.chars();
    let character = characters.next().filter(|_| characters.next().is_none())?;
    CHARACTER_KEYS
        .iter()
        .find(|(known, _)| *known == character)
        .map(|(_, evdev)| *evdev)
}

fn modifier_key(modifier: Modifier) -> &'static ModifierKey {
    MODIFIER_KEYS
        .iter()
        .find(|key| key.modifier == modifier)
        .expect("MODIFIER_KEYS holds every Modifier")
}

/// The keysym name xkbcommon resolves for `character` (see the module docs).
fn unicode_name(character: char) -> String {
    format!("U{:04X}", u32::from(character))
}

/// Every key of a keymap: (evdev code, keysym name).
fn keys(text: &[char]) -> Vec<(u32, String)> {
    let named = NAMED_KEYS
        .iter()
        .map(|key| (key.evdev, key.name.to_owned()));
    let characters = CHARACTER_KEYS
        .iter()
        .map(|(character, evdev)| (*evdev, unicode_name(*character)));
    let modifiers = MODIFIER_KEYS
        .iter()
        .map(|modifier| (modifier.key.evdev, modifier.key.name.to_owned()));
    let text = (FIRST_TEXT_CODE..)
        .zip(text)
        .map(|(evdev, character)| (evdev, unicode_name(*character)));
    named
        .chain(characters)
        .chain(modifiers)
        .chain(text)
        .collect()
}

fn keymap(text: &[char]) -> Keymap {
    let keys = keys(text);
    let mut out = String::with_capacity(4096);
    out.push_str("xkb_keymap {\nxkb_keycodes \"swayconnect\" {\nminimum = 8;\nmaximum = 255;\n");
    for (evdev, _) in &keys {
        let code = evdev + XKB_OFFSET;
        // Writing into a String cannot fail.
        let _ = writeln!(out, "<C{code}> = {code};");
    }
    out.push_str("};\n");
    out.push_str("xkb_types \"swayconnect\" { include \"complete\" };\n");
    out.push_str("xkb_compatibility \"swayconnect\" { include \"complete\" };\n");
    out.push_str("xkb_symbols \"swayconnect\" {\n");
    for (evdev, name) in &keys {
        let _ = writeln!(out, "key <C{}> {{ [ {name} ] }};", evdev + XKB_OFFSET);
    }
    for modifier in &MODIFIER_KEYS {
        let code = modifier.key.evdev + XKB_OFFSET;
        let _ = writeln!(out, "modifier_map {} {{ <C{code}> }};", modifier.real);
    }
    out.push_str("};\n};\n");
    Keymap(out)
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
