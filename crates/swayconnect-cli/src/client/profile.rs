//! The stored credential of one paired server (`docs/DESIGN.md` 7.2).

use std::fmt;

use serde_json::Value;
use swayconnect_protocol::fingerprint::{decode_base64url, encode_base64url};
use swayconnect_protocol::{DeviceId, JsonObject};

/// What a client keeps after `pairing.confirmed` (spec 5.4 step 10). `key` is the device key
/// `K`: a secret that is never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Profile {
    pub host: String,
    pub port: u16,
    pub fp: [u8; 32],
    pub device_id: DeviceId,
    pub key: [u8; 32],
    pub server_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("profile member `{0}` is missing or not valid")]
pub struct ProfileError(pub &'static str);

impl Profile {
    /// The content of the profile file. It holds `K` in the clear.
    pub fn to_json(&self) -> JsonObject {
        let members = [
            ("host", Value::from(self.host.as_str())),
            ("port", Value::from(self.port)),
            ("fp", Value::from(encode_base64url(&self.fp))),
            ("device_id", Value::from(self.device_id.as_str())),
            ("key", Value::from(encode_base64url(&self.key))),
            ("server_name", Value::from(self.server_name.as_str())),
        ];
        members
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    }

    pub fn from_json(object: &JsonObject) -> Result<Profile, ProfileError> {
        let port = object
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .ok_or(ProfileError("port"))?;
        let device_id =
            DeviceId::parse(text(object, "device_id")?).map_err(|_| ProfileError("device_id"))?;
        Ok(Profile {
            host: text(object, "host")?.to_owned(),
            port,
            fp: bytes32(object, "fp")?,
            device_id,
            key: bytes32(object, "key")?,
            server_name: text(object, "server_name")?.to_owned(),
        })
    }
}

fn text<'a>(object: &'a JsonObject, name: &'static str) -> Result<&'a str, ProfileError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(ProfileError(name))
}

fn bytes32(object: &JsonObject, name: &'static str) -> Result<[u8; 32], ProfileError> {
    decode_base64url(text(object, name)?).map_err(|_| ProfileError(name))
}

impl fmt::Debug for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Profile")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("device_id", &self.device_id)
            .field("server_name", &self.server_name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        Profile {
            host: "desk.example".to_owned(),
            port: 28414,
            fp: [1; 32],
            device_id: DeviceId::from_bytes([2; 16]),
            key: [3; 32],
            server_name: "desk".to_owned(),
        }
    }

    #[test]
    fn json_form_round_trips() {
        assert_eq!(Profile::from_json(&profile().to_json()), Ok(profile()));
    }

    #[test]
    fn a_missing_or_malformed_member_is_named() {
        for (name, bad_value) in [
            ("host", Value::Null),
            ("port", Value::from(65536)),
            ("fp", Value::from("AAAA")),
            ("device_id", Value::from("not base64url!")),
            ("key", Value::from(7)),
            ("server_name", Value::Null),
        ] {
            let mut object = profile().to_json();
            object.insert(name.to_owned(), bad_value);
            assert_eq!(Profile::from_json(&object), Err(ProfileError(name)));
            object.remove(name);
            assert_eq!(Profile::from_json(&object), Err(ProfileError(name)));
        }
    }

    #[test]
    fn debug_output_leaves_out_the_key() {
        let key_text = encode_base64url(&profile().key);
        let debug = format!("{:?}", profile());
        assert!(!debug.contains(&key_text));
        assert!(!debug.contains("key"));
        assert!(!debug.contains("3, 3"));
    }
}
