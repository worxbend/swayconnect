//! Message to JSON object.

use serde_json::Value;

use super::{
    Auth, Bye, ErrorBody, Evt, Hello, JsonObject, Message, Pair, Reject, Req, Res, Welcome,
};
use crate::fingerprint::encode_base64url;
use crate::json::{self, object_of};

pub(super) fn message(message: &Message) -> JsonObject {
    match message {
        Message::Hello(hello) => self::hello(hello),
        Message::Pair(pair) => self::pair(pair),
        Message::Auth(auth) => self::auth(auth),
        Message::Welcome(welcome) => self::welcome(welcome),
        Message::Reject(reject) => self::reject(reject),
        Message::Req(req) => self::req(req),
        Message::Res(res) => self::res(res),
        Message::Evt(evt) => self::evt(evt),
        Message::Bye(bye) => self::bye(bye),
    }
}

fn hello(hello: &Hello) -> JsonObject {
    let range = object_of([
        ("min", hello.protocol.min.into()),
        ("max", hello.protocol.max.into()),
    ]);
    object_of([
        ("type", "hello".into()),
        ("protocol", range.into()),
        ("nonce", encode_base64url(&hello.nonce).into()),
    ])
}

fn pair(pair: &Pair) -> JsonObject {
    let mut object = object_of([
        ("type", "pair".into()),
        ("protocol", pair.protocol.into()),
        ("nonce", encode_base64url(&pair.nonce).into()),
        ("proof", encode_base64url(&pair.proof).into()),
        ("device_name", pair.device_name.as_str().into()),
    ]);
    insert_optional(&mut object, "platform", pair.platform.as_deref());
    insert_optional(&mut object, "client", pair.client.as_deref());
    object
}

fn auth(auth: &Auth) -> JsonObject {
    let mut object = object_of([
        ("type", "auth".into()),
        ("protocol", auth.protocol.into()),
        ("device_id", auth.device_id.as_str().into()),
        ("nonce", encode_base64url(&auth.nonce).into()),
        ("proof", encode_base64url(&auth.proof).into()),
    ]);
    insert_optional(&mut object, "client", auth.client.as_deref());
    object
}

fn welcome(welcome: &Welcome) -> JsonObject {
    let scopes: Vec<Value> = welcome
        .scopes
        .iter()
        .map(|scope| scope.as_str().into())
        .collect();
    let server = object_of([
        ("name", welcome.server.name.as_str().into()),
        ("version", welcome.server.version.as_str().into()),
    ]);
    let limits = object_of([
        ("max_frame_c2s", welcome.limits.max_frame_c2s.into()),
        ("max_frame_s2c", welcome.limits.max_frame_s2c.into()),
        ("idle_timeout_ms", welcome.limits.idle_timeout_ms.into()),
    ]);
    let mut object = object_of([
        ("type", "welcome".into()),
        ("protocol", welcome.protocol.into()),
        ("device_id", welcome.device_id.as_str().into()),
        (
            "server_proof",
            encode_base64url(&welcome.server_proof).into(),
        ),
        ("scopes", scopes.into()),
        ("ops", welcome.ops.clone().into()),
        ("topics", welcome.topics.clone().into()),
        ("read_only", welcome.read_only.into()),
        ("server", server.into()),
        ("limits", limits.into()),
    ]);
    insert_optional(&mut object, "confirm_code", welcome.confirm_code.as_deref());
    let known_scopes = welcome.known_scopes.clone().map(Value::from);
    let known_ops = welcome.known_ops.clone().map(Value::from);
    json::insert_present(&mut object, "known_scopes", known_scopes);
    json::insert_present(&mut object, "known_ops", known_ops);
    object
}

fn reject(reject: &Reject) -> JsonObject {
    object_of([
        ("type", "reject".into()),
        ("code", reject.code.as_str().into()),
        ("message", reject.message.as_str().into()),
    ])
}

fn req(req: &Req) -> JsonObject {
    let mut object = object_of([
        ("type", "req".into()),
        ("id", req.id.into()),
        ("op", req.op.as_str().into()),
    ]);
    if let Some(args) = &req.args {
        object.insert("args".to_owned(), args.clone());
    }
    object
}

fn res(res: &Res) -> JsonObject {
    let mut object = object_of([
        ("type", "res".into()),
        ("id", res.id.into()),
        ("ok", res.ok.into()),
    ]);
    if let Some(result) = &res.result {
        object.insert("result".to_owned(), result.clone().into());
    }
    if let Some(error) = &res.error {
        object.insert("error".to_owned(), error_body(error).into());
    }
    object
}

fn error_body(error: &ErrorBody) -> JsonObject {
    let mut object = object_of([
        ("code", error.code.as_str().into()),
        ("message", error.message.as_str().into()),
    ]);
    if let Some(detail) = &error.detail {
        object.insert("detail".to_owned(), detail.clone().into());
    }
    object
}

fn evt(evt: &Evt) -> JsonObject {
    object_of([
        ("type", "evt".into()),
        ("event", evt.event.as_str().into()),
        ("data", evt.data.clone().into()),
    ])
}

fn bye(bye: &Bye) -> JsonObject {
    object_of([
        ("type", "bye".into()),
        ("code", bye.code.as_str().into()),
        ("message", bye.message.as_str().into()),
    ])
}

fn insert_optional(object: &mut JsonObject, name: &str, text: Option<&str>) {
    if let Some(text) = text {
        object.insert(name.to_owned(), text.into());
    }
}
