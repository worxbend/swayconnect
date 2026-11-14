//! Argument decoders shared by several operations (`docs/PROTOCOL.md` 7.2, 17.5 to 17.11).

use crate::icon::IconHash;
use crate::json::{self, InvalidMember, Member};
use crate::limits::{
    MAX_FOCUS_NUMBER, MAX_ICON_HASHES, MAX_TOPICS_PER_SUBSCRIBE, MIN_KEEP_AWAKE_DURATION_S,
};
use crate::message::JsonObject;

use super::{ContainerLayout, DURATION_S, KeepAwake, WorkspaceTarget};

/// 1 to 16 topic names, no duplicates.
pub(super) fn decode_topics(args: &JsonObject) -> Member<Vec<String>> {
    const NAME: &str = "topics";
    let topics = json::string_array(args, NAME)?;
    if (1..=MAX_TOPICS_PER_SUBSCRIBE).contains(&topics.len()) && !has_duplicate(&topics) {
        Ok(topics)
    } else {
        Err(InvalidMember(NAME))
    }
}

/// 1 to 8 distinct canonical hashes (17.5). A string that is not one is a bad request; a
/// well-formed hash the server never issued is answered in `missing`.
pub(super) fn decode_hashes(args: &JsonObject) -> Member<Vec<IconHash>> {
    const NAME: &str = "hashes";
    let hashes = json::array(args, NAME)?
        .iter()
        .map(|hash| IconHash::decode_value(hash, NAME))
        .collect::<Member<Vec<IconHash>>>()?;
    if (1..=MAX_ICON_HASHES).contains(&hashes.len()) && !has_duplicate(&hashes) {
        Ok(hashes)
    } else {
        Err(InvalidMember(NAME))
    }
}

pub(crate) fn has_duplicate<T: PartialEq>(entries: &[T]) -> bool {
    entries
        .iter()
        .enumerate()
        .any(|(index, entry)| entries[..index].contains(entry))
}

pub(super) fn decode_layout(args: &JsonObject) -> Member<ContainerLayout> {
    const NAME: &str = "layout";
    ContainerLayout::from_wire(json::string(args, NAME)?).ok_or(InvalidMember(NAME))
}

/// `duration_s` is read only when `enabled` is `true` (17.8: "Ignored when `enabled` is
/// `false`"). When present it is an `int` of at least 60; `null` is not allowed.
pub(super) fn decode_keep_awake(args: &JsonObject) -> Member<KeepAwake> {
    if !json::boolean(args, "enabled")? {
        return Ok(KeepAwake::Off);
    }
    let duration_s = match args.get(DURATION_S) {
        None => None,
        Some(duration_s) => Some(
            json::as_int(duration_s)
                .filter(|duration_s| *duration_s >= MIN_KEEP_AWAKE_DURATION_S)
                .ok_or(InvalidMember(DURATION_S))?,
        ),
    };
    Ok(KeepAwake::On { duration_s })
}

pub(super) fn decode_workspace_target(args: &JsonObject) -> Member<WorkspaceTarget> {
    match (args.get("workspace_id"), args.get("number")) {
        (Some(id), None) => json::as_int(id)
            .map(WorkspaceTarget::Id)
            .ok_or(InvalidMember("workspace_id")),
        (None, Some(number)) => json::as_int(number)
            .and_then(|number| u16::try_from(number).ok())
            .filter(|number| *number <= MAX_FOCUS_NUMBER)
            .map(WorkspaceTarget::Number)
            .ok_or(InvalidMember("number")),
        (Some(_), Some(_)) | (None, None) => Err(InvalidMember("workspace_id")),
    }
}

/// The optional `number` of `app.launch`: 0 to 999, `null` refused.
pub(super) fn decode_launch_number(args: &JsonObject) -> Member<Option<u16>> {
    const NAME: &str = "number";
    if args.contains_key(NAME) {
        json::int_in(args, NAME, 0..=MAX_FOCUS_NUMBER).map(Some)
    } else {
        Ok(None)
    }
}
