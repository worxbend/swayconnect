//! Typed member access on a decoded JSON object (`docs/PROTOCOL.md` 2.2).
//!
//! JSON `null` is a wrong type everywhere except where a function says `nullable`.

use serde_json::Value;

use crate::fingerprint::decode_base64url;
use crate::limits::MAX_INT;
use crate::message::JsonObject;

/// A member that is missing, has the wrong type, or is outside its stated limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("member `{0}` is missing or invalid")]
pub struct InvalidMember(pub &'static str);

pub(crate) type Member<T> = Result<T, InvalidMember>;

pub(crate) fn string<'a>(object: &'a JsonObject, name: &'static str) -> Member<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(InvalidMember(name))
}

pub(crate) fn optional_string<'a>(
    object: &'a JsonObject,
    name: &'static str,
) -> Member<Option<&'a str>> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value.as_str().map(Some).ok_or(InvalidMember(name)),
    }
}

/// A member of type `string or null`.
pub(crate) fn nullable_string(object: &JsonObject, name: &'static str) -> Member<Option<String>> {
    match object.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        _ => Err(InvalidMember(name)),
    }
}

/// The type `int`: no fraction, no exponent, 0 to 2^53 - 1.
pub(crate) fn as_int(value: &Value) -> Option<u64> {
    value.as_u64().filter(|number| *number <= MAX_INT)
}

pub(crate) fn int(object: &JsonObject, name: &'static str) -> Member<u64> {
    object.get(name).and_then(as_int).ok_or(InvalidMember(name))
}

/// An `int` held as `u32`. A larger value is a valid `int`, so it is not refused here; it
/// becomes `u32::MAX`, which no version range and no real limit reaches.
pub(crate) fn saturating_u32(object: &JsonObject, name: &'static str) -> Member<u32> {
    int(object, name).map(|number| u32::try_from(number).unwrap_or(u32::MAX))
}

pub(crate) fn boolean(object: &JsonObject, name: &'static str) -> Member<bool> {
    object
        .get(name)
        .and_then(Value::as_bool)
        .ok_or(InvalidMember(name))
}

pub(crate) fn object<'a>(object: &'a JsonObject, name: &'static str) -> Member<&'a JsonObject> {
    object
        .get(name)
        .and_then(Value::as_object)
        .ok_or(InvalidMember(name))
}

pub(crate) fn optional_object(
    object: &JsonObject,
    name: &'static str,
) -> Member<Option<JsonObject>> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_object()
            .cloned()
            .map(Some)
            .ok_or(InvalidMember(name)),
    }
}

pub(crate) fn array<'a>(object: &'a JsonObject, name: &'static str) -> Member<&'a [Value]> {
    object
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(InvalidMember(name))
}

pub(crate) fn string_array(object: &JsonObject, name: &'static str) -> Member<Vec<String>> {
    array(object, name)?
        .iter()
        .map(|entry| entry.as_str().map(str::to_owned))
        .collect::<Option<Vec<String>>>()
        .ok_or(InvalidMember(name))
}

/// An array whose entries are all objects, each decoded by `decode`.
pub(crate) fn object_array<T>(
    object: &JsonObject,
    name: &'static str,
    decode: impl Fn(&JsonObject) -> Member<T>,
) -> Member<Vec<T>> {
    array(object, name)?
        .iter()
        .map(|entry| decode(entry.as_object().ok_or(InvalidMember(name))?))
        .collect()
}

/// The types `bytes32` and `bytes16`.
pub(crate) fn bytes<const N: usize>(object: &JsonObject, name: &'static str) -> Member<[u8; N]> {
    decode_base64url(string(object, name)?).map_err(|_| InvalidMember(name))
}

pub(crate) fn absent(object: &JsonObject, name: &'static str) -> Member<()> {
    if object.contains_key(name) {
        Err(InvalidMember(name))
    } else {
        Ok(())
    }
}

pub(crate) fn object_of<const N: usize>(members: [(&str, Value); N]) -> JsonObject {
    members
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}
