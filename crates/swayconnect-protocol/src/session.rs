//! Topic `session` and the results of `keep_awake.set` and `session.lock`
//! (`docs/PROTOCOL.md` 17.6, 17.8).

use serde_json::Value;

use crate::json::{self, InvalidMember, Member, object_of};
use crate::message::JsonObject;

/// Who holds keep-awake, the remote keyboard or the launcher. The server builds the state per
/// device, so the name is relative to the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Holder {
    ThisDevice,
    /// Also every holder name this version does not know (17.8).
    OtherDevice,
}

impl Holder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Holder::ThisDevice => "this_device",
            Holder::OtherDevice => "other_device",
        }
    }

    fn from_wire(name: &str) -> Holder {
        if name == Holder::ThisDevice.as_str() {
            Holder::ThisDevice
        } else {
            Holder::OtherDevice
        }
    }
}

/// The state object of topic `session` (17.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub keep_awake: KeepAwakeState,
    /// Windows that keep the desktop awake by themselves (sway `inhibit_idle`).
    pub windows_inhibiting: u64,
    /// `None` when the server cannot tell.
    pub locked: Option<bool>,
    /// `input.active`. Absent when the server does not offer `input.*`.
    pub input: Option<Held>,
    /// `launcher.open`. Absent when the server does not offer `launcher.open`.
    pub launcher: Option<Held>,
}

/// Member `keep_awake` of [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAwakeState {
    pub active: bool,
    /// At the moment the state was built. `None` when off.
    pub remaining_ms: Option<u64>,
    /// `None` when off.
    pub holder: Option<Holder>,
    pub default_duration_s: u64,
    pub max_duration_s: u64,
    pub max_continuous_s: u64,
    /// Set while `limit_reached` applies.
    pub cooldown_remaining_ms: Option<u64>,
}

/// Members `input` and `launcher` of [`Session`]: whether the feature runs, and for whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// `input.active` or `launcher.open`.
    pub running: bool,
    pub holder: Option<Holder>,
}

const INPUT: &str = "input";
const INPUT_RUNNING: &str = "active";
const LAUNCHER: &str = "launcher";
const LAUNCHER_RUNNING: &str = "open";

impl Session {
    /// Checks presence and types. An unknown `holder` string becomes
    /// [`Holder::OtherDevice`]; unknown members are ignored.
    pub fn decode(object: &JsonObject) -> Result<Session, InvalidMember> {
        Ok(Session {
            keep_awake: KeepAwakeState::decode(json::object(object, "keep_awake")?)?,
            windows_inhibiting: json::int(object, "windows_inhibiting")?,
            locked: json::nullable(object, "locked", |locked| {
                locked.as_bool().ok_or(InvalidMember("locked"))
            })?,
            input: optional_held(object, INPUT, INPUT_RUNNING)?,
            launcher: optional_held(object, LAUNCHER, LAUNCHER_RUNNING)?,
        })
    }

    pub fn encode(&self) -> JsonObject {
        let mut object = object_of([
            ("keep_awake", self.keep_awake.encode().into()),
            ("windows_inhibiting", self.windows_inhibiting.into()),
            ("locked", self.locked.into()),
        ]);
        let input = self.input.map(|input| input.encode(INPUT_RUNNING));
        let launcher = self
            .launcher
            .map(|launcher| launcher.encode(LAUNCHER_RUNNING));
        json::insert_present(&mut object, INPUT, input);
        json::insert_present(&mut object, LAUNCHER, launcher);
        object
    }
}

impl KeepAwakeState {
    fn decode(object: &JsonObject) -> Member<KeepAwakeState> {
        Ok(KeepAwakeState {
            active: json::boolean(object, "active")?,
            remaining_ms: json::nullable_int(object, "remaining_ms")?,
            holder: holder(object)?,
            default_duration_s: json::int(object, "default_duration_s")?,
            max_duration_s: json::int(object, "max_duration_s")?,
            max_continuous_s: json::int(object, "max_continuous_s")?,
            cooldown_remaining_ms: json::nullable_int(object, "cooldown_remaining_ms")?,
        })
    }

    fn encode(&self) -> JsonObject {
        object_of([
            ("active", self.active.into()),
            ("remaining_ms", self.remaining_ms.into()),
            ("holder", encode_holder(self.holder)),
            ("default_duration_s", self.default_duration_s.into()),
            ("max_duration_s", self.max_duration_s.into()),
            ("max_continuous_s", self.max_continuous_s.into()),
            ("cooldown_remaining_ms", self.cooldown_remaining_ms.into()),
        ])
    }
}

impl Held {
    fn encode(self, running: &str) -> Value {
        object_of([
            (running, self.running.into()),
            ("holder", encode_holder(self.holder)),
        ])
        .into()
    }
}

/// An optional object member: absent is `None`, `null` is an error.
fn optional_held(
    object: &JsonObject,
    name: &'static str,
    running: &'static str,
) -> Member<Option<Held>> {
    json::optional_object(object, name)?
        .map(|held| {
            Ok(Held {
                running: json::boolean(&held, running)?,
                holder: holder(&held)?,
            })
        })
        .transpose()
}

/// A member `holder` of type `string or null`.
fn holder(object: &JsonObject) -> Member<Option<Holder>> {
    Ok(json::nullable_string(object, "holder")?.map(|name| Holder::from_wire(&name)))
}

fn encode_holder(holder: Option<Holder>) -> Value {
    holder.map(Holder::as_str).into()
}

/// The `result` of `keep_awake.set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAwakeResult {
    /// Time left; `None` when off.
    pub remaining_ms: Option<u64>,
}

impl KeepAwakeResult {
    /// `remaining_ms` is required: `int` or `null`.
    pub fn decode(result: &JsonObject) -> Result<KeepAwakeResult, InvalidMember> {
        Ok(KeepAwakeResult {
            remaining_ms: json::nullable_int(result, "remaining_ms")?,
        })
    }

    pub fn encode(self) -> JsonObject {
        object_of([("remaining_ms", self.remaining_ms.into())])
    }
}

/// The `result` of `session.lock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockResult {
    /// The server started nothing: the screen was locked, or its lock command still runs.
    pub already_locked: bool,
}

impl LockResult {
    /// `already_locked` is a required `bool`.
    pub fn decode(result: &JsonObject) -> Result<LockResult, InvalidMember> {
        Ok(LockResult {
            already_locked: json::boolean(result, "already_locked")?,
        })
    }

    pub fn encode(self) -> JsonObject {
        object_of([("already_locked", self.already_locked.into())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> JsonObject {
        value.as_object().expect("test object").clone()
    }

    fn off() -> Value {
        serde_json::json!({"active": false, "remaining_ms": null, "holder": null,
            "default_duration_s": 3600, "max_duration_s": 7200, "max_continuous_s": 14400,
            "cooldown_remaining_ms": null})
    }

    #[test]
    fn input_and_launcher_are_optional_but_never_null() {
        let base =
            serde_json::json!({"keep_awake": off(), "windows_inhibiting": 0, "locked": null});
        let session = Session::decode(&object(base.clone())).expect("decodes");
        assert_eq!(
            (session.input, session.launcher, session.locked),
            (None, None, None)
        );
        assert_eq!(session.encode(), object(base.clone()));

        let mut with_null = object(base);
        with_null.insert(LAUNCHER.to_owned(), Value::Null);
        assert_eq!(Session::decode(&with_null), Err(InvalidMember(LAUNCHER)));
    }

    #[test]
    fn only_this_device_names_this_device() {
        for (name, expected) in [
            ("this_device", Holder::ThisDevice),
            ("other_device", Holder::OtherDevice),
            ("This_device", Holder::OtherDevice),
            ("", Holder::OtherDevice),
        ] {
            assert_eq!(Holder::from_wire(name), expected, "{name:?}");
        }
    }

    #[test]
    fn results_round_trip() {
        for remaining_ms in [None, Some(0), Some(3_600_000)] {
            let result = KeepAwakeResult { remaining_ms };
            assert_eq!(KeepAwakeResult::decode(&result.encode()), Ok(result));
        }
        for already_locked in [false, true] {
            let result = LockResult { already_locked };
            assert_eq!(LockResult::decode(&result.encode()), Ok(result));
        }
    }
}
