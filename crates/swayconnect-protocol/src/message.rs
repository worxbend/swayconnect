//! Message types (`docs/PROTOCOL.md` sections 5.2 and 6).
//!
//! `bytes32` members are held decoded. An `Option` member is absent on the wire when `None`;
//! JSON `null` is a decode error for it (spec 2.2).
//!
//! Nonces and proofs are never logged (`docs/DESIGN.md` 4.8), so the `Debug` output of the
//! handshake messages leaves them out.

mod decode;
mod encode;
#[cfg(test)]
mod tests;

use std::fmt;

use crate::code::{ByeCode, ErrorCode, RejectCode, wire_names};
use crate::fingerprint::{Base64UrlError, decode_base64url, encode_base64url};
use crate::json::InvalidMember;

pub type JsonObject = serde_json::Map<String, serde_json::Value>;

/// A device id exactly as it appears in JSON: 22 characters of canonical base64url.
///
/// Kept as text because the proofs use its ASCII bytes and the server looks it up by exact
/// string (spec 5.3, 5.5).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    /// Accepts canonical base64url of exactly 16 bytes (spec 2.2).
    pub fn parse(text: &str) -> Result<DeviceId, Base64UrlError> {
        decode_base64url::<16>(text).map(|_| DeviceId(text.to_owned()))
    }

    /// The id of a new device, from 16 random bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> DeviceId {
        DeviceId(encode_base64url(&bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Spec 7.1. Wire names: `state.read`, `state.titles`, `control`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    StateRead,
    StateTitles,
    Control,
}

wire_names!(Scope {
    StateRead => "state.read",
    StateTitles => "state.titles",
    Control => "control",
});

/// `hello.protocol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolRange {
    pub min: u32,
    pub max: u32,
}

/// Server to client, first frame.
#[derive(Clone, PartialEq, Eq)]
pub struct Hello {
    pub protocol: ProtocolRange,
    /// `server_nonce`.
    pub nonce: [u8; 32],
}

/// Client to server, first pairing.
#[derive(Clone, PartialEq, Eq)]
pub struct Pair {
    pub protocol: u32,
    /// `client_nonce`.
    pub nonce: [u8; 32],
    /// `pair_proof`.
    pub proof: [u8; 32],
    pub device_name: String,
    pub platform: Option<String>,
    pub client: Option<String>,
}

/// Client to server, every later connection.
#[derive(Clone, PartialEq, Eq)]
pub struct Auth {
    pub protocol: u32,
    pub device_id: DeviceId,
    /// `client_nonce`.
    pub nonce: [u8; 32],
    /// `client_proof`.
    pub proof: [u8; 32],
    pub client: Option<String>,
}

/// Server to client, answer to a valid `pair` or `auth`.
#[derive(Clone, PartialEq, Eq)]
pub struct Welcome {
    pub protocol: u32,
    pub device_id: DeviceId,
    pub server_proof: [u8; 32],
    /// Present after `pair` only.
    pub confirm_code: Option<String>,
    pub scopes: Vec<Scope>,
    pub ops: Vec<String>,
    pub topics: Vec<String>,
    /// Every scope name the server knows, granted or not (17.3). `None` when absent.
    pub known_scopes: Option<Vec<String>>,
    /// Every operation the server implements and is configured for (17.3). `None` when absent.
    pub known_ops: Option<Vec<String>>,
    pub read_only: bool,
    pub server: ServerInfo,
    pub limits: Limits,
}

/// `welcome.server`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// `welcome.limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_frame_c2s: u32,
    pub max_frame_s2c: u32,
    pub idle_timeout_ms: u64,
}

/// Server to client, answer to a refused first frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    pub code: RejectCode,
    pub message: String,
}

/// Client to server, after `welcome`.
#[derive(Debug, Clone, PartialEq)]
pub struct Req {
    pub id: u64,
    pub op: String,
    /// Any JSON value: a present `args` that is not an object is answered with `bad_request`,
    /// not refused at decode (spec 6.1).
    pub args: Option<serde_json::Value>,
}

/// Server to client, one per accepted `req`.
#[derive(Debug, Clone, PartialEq)]
pub struct Res {
    pub id: u64,
    pub ok: bool,
    /// Present exactly when `ok` is true.
    pub result: Option<JsonObject>,
    /// Present exactly when `ok` is false.
    pub error: Option<ErrorBody>,
}

/// `res.error`.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub detail: Option<JsonObject>,
}

/// Server to client, no id and no answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Evt {
    pub event: String,
    pub data: JsonObject,
}

/// Server to client, before a deliberate close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bye {
    pub code: ByeCode,
    pub message: String,
}

/// One decoded message of any type.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello(Hello),
    Pair(Pair),
    Auth(Auth),
    Welcome(Welcome),
    Reject(Reject),
    Req(Req),
    Res(Res),
    Evt(Evt),
    Bye(Bye),
}

/// Why a JSON object is not a message. The three cases get different answers (spec 3.4, 5.7,
/// 6.4, 13.2): before `welcome` each one is `bad_message`; after it `UnknownType` is ignored
/// and the other two are a `protocol_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("member `type` is missing or is not a string")]
    NoType,
    #[error("unknown message type")]
    UnknownType,
    #[error(transparent)]
    Invalid(#[from] InvalidMember),
}

impl Message {
    /// Decodes and validates one message: every required member is present, every defined
    /// member has its stated type, length, range and character set (spec 5.7 step 1).
    /// Unknown members are ignored.
    ///
    /// Unknown values follow spec 13.2: an unknown `reject.code` becomes
    /// `BadMessage`, an unknown `bye.code` becomes `Shutdown`, an unknown `error.code` becomes
    /// `ErrorCode::Unknown`, and unknown scope names are dropped.
    pub fn decode(object: &JsonObject) -> Result<Message, DecodeError> {
        decode::message(object)
    }

    pub fn encode(&self) -> JsonObject {
        encode::message(self)
    }
}

impl Res {
    pub fn success(id: u64, result: JsonObject) -> Res {
        Res {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, error: ErrorBody) -> Res {
        Res {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

impl fmt::Debug for Hello {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Hello")
            .field("protocol", &self.protocol)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Pair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Pair")
            .field("protocol", &self.protocol)
            .field("device_name", &self.device_name)
            .field("platform", &self.platform)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Auth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Auth")
            .field("protocol", &self.protocol)
            .field("device_id", &self.device_id)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Welcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Welcome")
            .field("protocol", &self.protocol)
            .field("device_id", &self.device_id)
            .field("confirm_code", &self.confirm_code)
            .field("scopes", &self.scopes)
            .field("ops", &self.ops)
            .field("topics", &self.topics)
            .field("known_scopes", &self.known_scopes)
            .field("known_ops", &self.known_ops)
            .field("read_only", &self.read_only)
            .field("server", &self.server)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
