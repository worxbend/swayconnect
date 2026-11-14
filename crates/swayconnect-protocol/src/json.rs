//! Typed member access on a decoded JSON object (`docs/PROTOCOL.md` 2.2).
//!
//! JSON `null` is a wrong type everywhere except where a function says `nullable`.

use std::ops::RangeInclusive;

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

/// An `int` in a narrower stated range (spec 2.2), held as the smallest type that fits it.
pub(crate) fn int_in<T: TryFrom<u64> + PartialOrd>(
    object: &JsonObject,
    name: &'static str,
    range: RangeInclusive<T>,
) -> Member<T> {
    int(object, name)
        .ok()
        .and_then(|number| T::try_from(number).ok())
        .filter(|number| range.contains(number))
        .ok_or(InvalidMember(name))
}

/// A member of type `int or null`.
pub(crate) fn nullable_int(object: &JsonObject, name: &'static str) -> Member<Option<u64>> {
    match object.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => as_int(value).map(Some).ok_or(InvalidMember(name)),
        None => Err(InvalidMember(name)),
    }
}

/// The type `sint` (spec 17.1 rule 6): no fraction, no exponent, -(2^53 - 1) to 2^53 - 1.
pub(crate) fn sint(object: &JsonObject, name: &'static str) -> Member<i64> {
    object
        .get(name)
        .and_then(Value::as_i64)
        .filter(|number| number.unsigned_abs() <= MAX_INT)
        .ok_or(InvalidMember(name))
}

/// An array of exactly `N` values of type `int`.
pub(crate) fn int_array<const N: usize>(
    object: &JsonObject,
    name: &'static str,
) -> Member<[u64; N]> {
    let numbers = array(object, name)?
        .iter()
        .map(as_int)
        .collect::<Option<Vec<u64>>>()
        .ok_or(InvalidMember(name))?;
    <[u64; N]>::try_from(numbers).map_err(|_| InvalidMember(name))
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

/// A member of type `bool` that is optional: absent is `None`, `null` is an error.
pub(crate) fn optional_boolean(object: &JsonObject, name: &'static str) -> Member<Option<bool>> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value.as_bool().map(Some).ok_or(InvalidMember(name)),
    }
}

/// A member of type `T or null`, decoded by `decode` when it is not `null`.
pub(crate) fn nullable<T>(
    object: &JsonObject,
    name: &'static str,
    decode: impl FnOnce(&Value) -> Member<T>,
) -> Member<Option<T>> {
    match object.get(name) {
        Some(Value::Null) => Ok(None),
        Some(value) => decode(value).map(Some),
        None => Err(InvalidMember(name)),
    }
}

/// A member that is optional and of type `T or null`: absent is `None`, `null` is `Some(None)`.
/// The two stay apart so that a state re-encodes to the same JSON value.
pub(crate) fn optional_nullable<T>(
    object: &JsonObject,
    name: &'static str,
    decode: impl FnOnce(&Value) -> Member<T>,
) -> Member<Option<Option<T>>> {
    if object.contains_key(name) {
        nullable(object, name, decode).map(Some)
    } else {
        Ok(None)
    }
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

/// An optional member whose entries are all objects: absent is `None`, `null` is an error.
pub(crate) fn optional_object_array<T>(
    object: &JsonObject,
    name: &'static str,
    decode: impl Fn(&JsonObject) -> Member<T>,
) -> Member<Option<Vec<T>>> {
    if object.contains_key(name) {
        object_array(object, name, decode).map(Some)
    } else {
        Ok(None)
    }
}

/// An optional member of type `string[]`: absent is `None`, `null` is an error.
pub(crate) fn optional_string_array(
    object: &JsonObject,
    name: &'static str,
) -> Member<Option<Vec<String>>> {
    if object.contains_key(name) {
        string_array(object, name).map(Some)
    } else {
        Ok(None)
    }
}

/// Inserts `value` under `name` when it is `Some`; leaves the member out otherwise.
pub(crate) fn insert_present(object: &mut JsonObject, name: &str, value: Option<Value>) {
    if let Some(value) = value {
        object.insert(name.to_owned(), value);
    }
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
