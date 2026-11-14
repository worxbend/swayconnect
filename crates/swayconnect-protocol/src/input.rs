//! Remote keyboard: targets, text, key presses, the `input.text` result and the `detail` of
//! the input errors (`docs/PROTOCOL.md` 17.13).
//!
//! Typed text is never logged (`AGENTS.md` section 5 rule 6), so the `Debug` output of
//! [`InputText`] gives its length only.

use std::fmt;

use serde_json::Value;

use crate::code::{ErrorCode, wire_names};
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::{MAX_INPUT_PART_SCALARS, MAX_KEY_REPEAT};
use crate::message::{ErrorBody, JsonObject};
use crate::ops::has_duplicate;
use crate::text::is_input_text;

const WINDOW_ID: &str = "window_id";
const LAUNCHER: &str = "launcher";
const KEY: &str = "key";
const MODIFIERS: &str = "modifiers";
const REPEAT: &str = "repeat";
const DELIVERED_CHARS: &str = "delivered_chars";
const FOCUSED_WINDOW_ID: &str = "focused_window_id";

/// Where the input goes: exactly one of the two args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTarget {
    /// The window the user saw as focused. It must be focused when each part is delivered.
    Window(u64),
    /// The launcher this device opened. On the wire `launcher: true`; `false` is refused.
    Launcher,
}

impl InputTarget {
    pub(crate) fn decode(args: &JsonObject) -> Member<InputTarget> {
        match (args.get(WINDOW_ID), args.get(LAUNCHER)) {
            (Some(id), None) => json::as_int(id)
                .map(InputTarget::Window)
                .ok_or(InvalidMember(WINDOW_ID)),
            (None, Some(Value::Bool(true))) => Ok(InputTarget::Launcher),
            (None, Some(_)) => Err(InvalidMember(LAUNCHER)),
            (Some(_), Some(_)) | (None, None) => Err(InvalidMember(WINDOW_ID)),
        }
    }

    pub(crate) fn member(self) -> (&'static str, Value) {
        match self {
            InputTarget::Window(id) => (WINDOW_ID, id.into()),
            InputTarget::Launcher => (LAUNCHER, true.into()),
        }
    }
}

/// The `text` of `input.text`: 1 to 1024 bytes of UTF-8 without a control or format character
/// except U+200C and U+200D. It goes only to the virtual keyboard.
#[derive(Clone, PartialEq, Eq)]
pub struct InputText(String);

impl InputText {
    /// `None` when the text breaks the rule; one bad character refuses all of it.
    pub fn parse(text: &str) -> Option<InputText> {
        is_input_text(text).then(|| InputText(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The parts of 17.13 rule 1, in order: at most 16 Unicode scalar values each. A part may
    /// end inside an emoji sequence.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        let mut rest = self.0.as_str();
        std::iter::from_fn(move || {
            if rest.is_empty() {
                return None;
            }
            let end = rest
                .char_indices()
                .nth(MAX_INPUT_PART_SCALARS)
                .map_or(rest.len(), |(index, _)| index);
            let (part, tail) = rest.split_at(end);
            rest = tail;
            Some(part)
        })
    }

    pub(crate) fn decode(args: &JsonObject) -> Member<InputText> {
        const NAME: &str = "text";
        InputText::parse(json::string(args, NAME)?).ok_or(InvalidMember(NAME))
    }
}

impl fmt::Debug for InputText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputText")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// A key name of `input.key`. The names are X keysym names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(&'static str);

const NAMED_KEYS: [&str; 15] = [
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
const FUNCTION_KEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];
/// One ASCII letter `a`-`z` or digit `0`-`9`; the name is the character.
const CHARACTER_KEYS: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

impl Key {
    /// `None` for a name outside the closed list, matched exactly.
    pub fn from_wire(name: &str) -> Option<Key> {
        NAMED_KEYS
            .iter()
            .chain(&FUNCTION_KEYS)
            .copied()
            .find(|key| *key == name)
            .or_else(|| character_key(name))
            .map(Key)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// `F1` to `F12`.
    pub fn is_function_key(self) -> bool {
        FUNCTION_KEYS.contains(&self.0)
    }
}

fn character_key(name: &str) -> Option<&'static str> {
    if name.len() != 1 {
        return None;
    }
    let index = CHARACTER_KEYS.find(name)?;
    Some(&CHARACTER_KEYS[index..=index])
}

/// One value of `modifiers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

wire_names!(Modifier {
    Ctrl => "ctrl",
    Alt => "alt",
    Shift => "shift",
    Super => "super",
});

/// The key, `modifiers` and `repeat` of `input.key`. Built only through [`KeyPress::new`],
/// so it never holds duplicate modifiers, a repeat outside 1 to 32, or `ctrl`+`alt`+`F1`..`F12`
/// (a VT switch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    key: Key,
    modifiers: Vec<Modifier>,
    repeat: u8,
}

impl KeyPress {
    /// The error names the member a request would have wrong.
    pub fn new(key: Key, modifiers: Vec<Modifier>, repeat: u8) -> Result<KeyPress, InvalidMember> {
        if !(1..=MAX_KEY_REPEAT).contains(&repeat) {
            return Err(InvalidMember(REPEAT));
        }
        let has = |modifier| modifiers.contains(&modifier);
        let is_vt_switch = key.is_function_key() && has(Modifier::Ctrl) && has(Modifier::Alt);
        if has_duplicate(&modifiers) || is_vt_switch {
            return Err(InvalidMember(MODIFIERS));
        }
        Ok(KeyPress {
            key,
            modifiers,
            repeat,
        })
    }

    pub const fn key(&self) -> Key {
        self.key
    }

    /// In request order.
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    pub const fn repeat(&self) -> u8 {
        self.repeat
    }

    /// `modifiers` and `repeat` are optional; absent means none and 1. `null` is refused.
    pub(crate) fn decode(args: &JsonObject) -> Member<KeyPress> {
        let key = Key::from_wire(json::string(args, KEY)?).ok_or(InvalidMember(KEY))?;
        let modifiers = json::optional_string_array(args, MODIFIERS)?
            .unwrap_or_default()
            .iter()
            .map(|name| Modifier::from_wire(name).ok_or(InvalidMember(MODIFIERS)))
            .collect::<Member<Vec<Modifier>>>()?;
        let repeat = match args.get(REPEAT) {
            None => 1,
            Some(_) => json::int_in(args, REPEAT, 1..=MAX_KEY_REPEAT)?,
        };
        KeyPress::new(key, modifiers, repeat)
    }

    /// The members beside the target. `modifiers` and `repeat` are left out at their defaults.
    pub(crate) fn encode_into(&self, object: &mut JsonObject) {
        object.insert(KEY.to_owned(), self.key.as_str().into());
        if !self.modifiers.is_empty() {
            let names: Vec<Value> = self
                .modifiers
                .iter()
                .map(|modifier| modifier.as_str().into())
                .collect();
            object.insert(MODIFIERS.to_owned(), names.into());
        }
        if self.repeat != 1 {
            object.insert(REPEAT.to_owned(), self.repeat.into());
        }
    }
}

/// The `result` of `input.text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputTextResult {
    /// Unicode scalar values delivered.
    pub delivered_chars: u64,
}

impl InputTextResult {
    /// `delivered_chars` is a required `int`.
    pub fn decode(result: &JsonObject) -> Result<InputTextResult, InvalidMember> {
        Ok(InputTextResult {
            delivered_chars: json::int(result, DELIVERED_CHARS)?,
        })
    }

    pub fn encode(self) -> JsonObject {
        object_of([(DELIVERED_CHARS, self.delivered_chars.into())])
    }
}

/// An input request that stopped at a failed check of 17.13 rule 2, with what was delivered
/// before it. Each variant is one error code; its fields are the `detail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRefusal {
    Locked {
        delivered_chars: u64,
    },
    LauncherClosed {
        delivered_chars: u64,
    },
    KeyboardGrabbed {
        delivered_chars: u64,
    },
    FocusChanged {
        delivered_chars: u64,
        /// The window sway reported as focused; `None` for none.
        focused_window_id: Option<u64>,
    },
}

impl InputRefusal {
    pub const fn code(self) -> ErrorCode {
        match self {
            InputRefusal::Locked { .. } => ErrorCode::Locked,
            InputRefusal::LauncherClosed { .. } => ErrorCode::LauncherClosed,
            InputRefusal::KeyboardGrabbed { .. } => ErrorCode::KeyboardGrabbed,
            InputRefusal::FocusChanged { .. } => ErrorCode::FocusChanged,
        }
    }

    pub const fn delivered_chars(self) -> u64 {
        match self {
            InputRefusal::Locked { delivered_chars }
            | InputRefusal::LauncherClosed { delivered_chars }
            | InputRefusal::KeyboardGrabbed { delivered_chars }
            | InputRefusal::FocusChanged {
                delivered_chars, ..
            } => delivered_chars,
        }
    }

    /// The `res.error` a server sends. `message` is its own fixed text.
    pub fn error_body(self, message: &str) -> ErrorBody {
        let mut detail = object_of([(DELIVERED_CHARS, self.delivered_chars().into())]);
        if let InputRefusal::FocusChanged {
            focused_window_id, ..
        } = self
        {
            detail.insert(FOCUSED_WINDOW_ID.to_owned(), focused_window_id.into());
        }
        ErrorBody {
            code: self.code(),
            message: message.to_owned(),
            detail: Some(detail),
        }
    }

    /// `Ok(None)` when `error` has another code. For one of the four codes, `detail` with an
    /// `int` `delivered_chars` is required, and for `focus_changed` also `focused_window_id`
    /// (`int or null`). Unknown detail members are ignored.
    pub fn decode(error: &ErrorBody) -> Result<Option<InputRefusal>, InvalidMember> {
        let detail = || error.detail.as_ref().ok_or(InvalidMember("detail"));
        let delivered = || json::int(detail()?, DELIVERED_CHARS);
        let refusal = match error.code {
            ErrorCode::Locked => InputRefusal::Locked {
                delivered_chars: delivered()?,
            },
            ErrorCode::LauncherClosed => InputRefusal::LauncherClosed {
                delivered_chars: delivered()?,
            },
            ErrorCode::KeyboardGrabbed => InputRefusal::KeyboardGrabbed {
                delivered_chars: delivered()?,
            },
            ErrorCode::FocusChanged => InputRefusal::FocusChanged {
                delivered_chars: delivered()?,
                focused_window_id: json::nullable_int(detail()?, FOCUSED_WINDOW_ID)?,
            },
            _ => return Ok(None),
        };
        Ok(Some(refusal))
    }
}

#[cfg(test)]
mod tests;
