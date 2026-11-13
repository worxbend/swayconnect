//! JSON object to message, with the validation of spec 5.7 step 1.

use serde_json::Value;

use super::{
    Auth, Bye, DecodeError, DeviceId, ErrorBody, Evt, Hello, JsonObject, Limits, Message, Pair,
    ProtocolRange, Reject, Req, Res, Scope, ServerInfo, Welcome,
};
use crate::code::{ByeCode, ErrorCode, RejectCode};
use crate::json::{self, InvalidMember, Member};
use crate::limits::{MAX_NAME_BYTES, MAX_PLATFORM_BYTES};
use crate::text::{is_display_name, is_info_text};

const CONFIRM_CODE_DIGITS: usize = 6;

pub(super) fn message(object: &JsonObject) -> Result<Message, DecodeError> {
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(DecodeError::NoType)?;
    let message = match kind {
        "hello" => hello(object).map(Message::Hello),
        "pair" => pair(object).map(Message::Pair),
        "auth" => auth(object).map(Message::Auth),
        "welcome" => welcome(object).map(Message::Welcome),
        "reject" => reject(object).map(Message::Reject),
        "req" => req(object).map(Message::Req),
        "res" => res(object).map(Message::Res),
        "evt" => evt(object).map(Message::Evt),
        "bye" => bye(object).map(Message::Bye),
        _ => return Err(DecodeError::UnknownType),
    };
    Ok(message?)
}

fn hello(object: &JsonObject) -> Member<Hello> {
    let range = json::object(object, "protocol")?;
    let protocol = ProtocolRange {
        min: json::saturating_u32(range, "min")?,
        max: json::saturating_u32(range, "max")?,
    };
    if protocol.min > protocol.max {
        return Err(InvalidMember("protocol"));
    }
    Ok(Hello {
        protocol,
        nonce: json::bytes(object, "nonce")?,
    })
}

fn pair(object: &JsonObject) -> Member<Pair> {
    Ok(Pair {
        protocol: json::saturating_u32(object, "protocol")?,
        nonce: json::bytes(object, "nonce")?,
        proof: json::bytes(object, "proof")?,
        device_name: display_name(object, "device_name")?,
        platform: info_text(object, "platform", MAX_PLATFORM_BYTES)?,
        client: info_text(object, "client", MAX_NAME_BYTES)?,
    })
}

fn auth(object: &JsonObject) -> Member<Auth> {
    Ok(Auth {
        protocol: json::saturating_u32(object, "protocol")?,
        device_id: device_id(object)?,
        nonce: json::bytes(object, "nonce")?,
        proof: json::bytes(object, "proof")?,
        client: info_text(object, "client", MAX_NAME_BYTES)?,
    })
}

fn welcome(object: &JsonObject) -> Member<Welcome> {
    let server = json::object(object, "server")?;
    let limits = json::object(object, "limits")?;
    Ok(Welcome {
        protocol: json::saturating_u32(object, "protocol")?,
        device_id: device_id(object)?,
        server_proof: json::bytes(object, "server_proof")?,
        confirm_code: confirm_code(object)?,
        scopes: known_scopes(object)?,
        ops: json::string_array(object, "ops")?,
        topics: json::string_array(object, "topics")?,
        known_scopes: json::optional_string_array(object, "known_scopes")?,
        known_ops: json::optional_string_array(object, "known_ops")?,
        read_only: json::boolean(object, "read_only")?,
        server: ServerInfo {
            name: display_name(server, "name")?,
            version: json::string(server, "version")?.to_owned(),
        },
        limits: Limits {
            max_frame_c2s: json::saturating_u32(limits, "max_frame_c2s")?,
            max_frame_s2c: json::saturating_u32(limits, "max_frame_s2c")?,
            idle_timeout_ms: json::int(limits, "idle_timeout_ms")?,
        },
    })
}

fn reject(object: &JsonObject) -> Member<Reject> {
    let code = json::string(object, "code")?;
    Ok(Reject {
        code: RejectCode::from_wire(code).unwrap_or(RejectCode::BadMessage),
        message: json::string(object, "message")?.to_owned(),
    })
}

fn req(object: &JsonObject) -> Member<Req> {
    Ok(Req {
        id: json::int(object, "id")?,
        op: json::string(object, "op")?.to_owned(),
        args: object.get("args").cloned(),
    })
}

fn res(object: &JsonObject) -> Member<Res> {
    let id = json::int(object, "id")?;
    if json::boolean(object, "ok")? {
        json::absent(object, "error")?;
        Ok(Res::success(id, json::object(object, "result")?.clone()))
    } else {
        json::absent(object, "result")?;
        Ok(Res::failure(
            id,
            error_body(json::object(object, "error")?)?,
        ))
    }
}

fn error_body(error: &JsonObject) -> Member<ErrorBody> {
    let code = json::string(error, "code")?;
    Ok(ErrorBody {
        code: ErrorCode::from_wire(code).unwrap_or(ErrorCode::Unknown),
        message: json::string(error, "message")?.to_owned(),
        detail: json::optional_object(error, "detail")?,
    })
}

fn evt(object: &JsonObject) -> Member<Evt> {
    Ok(Evt {
        event: json::string(object, "event")?.to_owned(),
        data: json::object(object, "data")?.clone(),
    })
}

fn bye(object: &JsonObject) -> Member<Bye> {
    let code = json::string(object, "code")?;
    Ok(Bye {
        code: ByeCode::from_wire(code).unwrap_or(ByeCode::Shutdown),
        message: json::string(object, "message")?.to_owned(),
    })
}

fn device_id(object: &JsonObject) -> Member<DeviceId> {
    DeviceId::parse(json::string(object, "device_id")?).map_err(|_| InvalidMember("device_id"))
}

fn display_name(object: &JsonObject, name: &'static str) -> Member<String> {
    let text = json::string(object, name)?;
    if is_display_name(text) {
        Ok(text.to_owned())
    } else {
        Err(InvalidMember(name))
    }
}

fn info_text(object: &JsonObject, name: &'static str, max_bytes: usize) -> Member<Option<String>> {
    match json::optional_string(object, name)? {
        None => Ok(None),
        Some(text) if is_info_text(text, max_bytes) => Ok(Some(text.to_owned())),
        Some(_) => Err(InvalidMember(name)),
    }
}

fn confirm_code(object: &JsonObject) -> Member<Option<String>> {
    const NAME: &str = "confirm_code";
    match json::optional_string(object, NAME)? {
        None => Ok(None),
        Some(code) if is_confirm_code(code) => Ok(Some(code.to_owned())),
        Some(_) => Err(InvalidMember(NAME)),
    }
}

fn is_confirm_code(code: &str) -> bool {
    code.len() == CONFIRM_CODE_DIGITS && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn known_scopes(object: &JsonObject) -> Member<Vec<Scope>> {
    let names = json::string_array(object, "scopes")?;
    Ok(names
        .iter()
        .filter_map(|name| Scope::from_wire(name))
        .collect())
}
