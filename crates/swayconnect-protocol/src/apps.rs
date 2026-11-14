//! Topic `apps`, desktop entry ids and the result of `launcher.open`
//! (`docs/PROTOCOL.md` 17.11).

use std::fmt;

use serde_json::Value;

use crate::icon::IconHash;
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::MAX_ENTRY_ID_BYTES;
use crate::message::JsonObject;

/// A desktop entry id as a request carries it: 1 to 255 bytes of `A-Z a-z 0-9 . _ -`.
/// The server matches it exactly against its own `apps` state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(String);

impl EntryId {
    /// `None` for a string outside the character set or the length.
    pub fn parse(text: &str) -> Option<EntryId> {
        let is_entry_byte = |byte: u8| byte.is_ascii_alphanumeric() || b"._-".contains(&byte);
        ((1..=MAX_ENTRY_ID_BYTES).contains(&text.len()) && text.bytes().all(is_entry_byte))
            .then(|| EntryId(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn decode(args: &JsonObject, name: &'static str) -> Member<EntryId> {
        EntryId::parse(json::string(args, name)?).ok_or(InvalidMember(name))
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The state object of topic `apps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apps {
    /// Sorted by `name`.
    pub apps: Vec<App>,
    /// `entry_id`s, most recent first.
    pub recent: Vec<String>,
    pub truncated: bool,
}

/// One launchable desktop entry. Every string is untrusted text. The topic never carries the
/// Exec line or a file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub entry_id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub keywords: Vec<String>,
    pub icon: Option<IconHash>,
    pub terminal: bool,
    /// Launching it needs `launch.risky`.
    pub risky: bool,
}

impl Apps {
    /// Checks presence and types. Unknown members are ignored.
    pub fn decode(object: &JsonObject) -> Result<Apps, InvalidMember> {
        Ok(Apps {
            apps: json::object_array(object, "apps", App::decode)?,
            recent: json::string_array(object, "recent")?,
            truncated: json::boolean(object, "truncated")?,
        })
    }

    pub fn encode(&self) -> JsonObject {
        let apps: Value = self
            .apps
            .iter()
            .map(|app| Value::Object(app.encode()))
            .collect();
        object_of([
            ("apps", apps),
            ("recent", self.recent.clone().into()),
            ("truncated", self.truncated.into()),
        ])
    }
}

impl App {
    fn decode(object: &JsonObject) -> Member<App> {
        Ok(App {
            entry_id: json::string(object, "entry_id")?.to_owned(),
            name: json::string(object, "name")?.to_owned(),
            generic_name: json::nullable_string(object, "generic_name")?,
            keywords: json::string_array(object, "keywords")?,
            icon: json::nullable(object, "icon", |hash| IconHash::decode_value(hash, "icon"))?,
            terminal: json::boolean(object, "terminal")?,
            risky: json::boolean(object, "risky")?,
        })
    }

    fn encode(&self) -> JsonObject {
        object_of([
            ("entry_id", self.entry_id.as_str().into()),
            ("name", self.name.as_str().into()),
            ("generic_name", self.generic_name.clone().into()),
            ("keywords", self.keywords.clone().into()),
            ("icon", self.icon.map(IconHash::encode_value).into()),
            ("terminal", self.terminal.into()),
            ("risky", self.risky.into()),
        ])
    }
}

/// The `result` of `launcher.open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherOpenResult {
    /// A launcher was already running; the caller did not become its holder.
    pub already_open: bool,
}

impl LauncherOpenResult {
    /// `already_open` is a required `bool`.
    pub fn decode(result: &JsonObject) -> Result<LauncherOpenResult, InvalidMember> {
        Ok(LauncherOpenResult {
            already_open: json::boolean(result, "already_open")?,
        })
    }

    pub fn encode(self) -> JsonObject {
        object_of([("already_open", self.already_open.into())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_id_charset_and_length() {
        for good in [
            "firefox",
            "org.gnome.Nautilus",
            "a-b_c.1",
            "a".repeat(255).as_str(),
        ] {
            assert_eq!(EntryId::parse(good).map(|id| id.0), Some(good.to_owned()));
        }
        let too_long = "a".repeat(256);
        let refused = [
            "",
            too_long.as_str(),
            "../firefox",
            "firefox ",
            "firefox;id",
            "f\u{456}refox",
            "/usr/share/applications/firefox.desktop",
            "fire\u{0}fox",
        ];
        for bad in refused {
            assert_eq!(EntryId::parse(bad), None, "{bad:?}");
        }
    }
}
