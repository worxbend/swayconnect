//! Icons: the content hash and the result of `icon.get` (`docs/PROTOCOL.md` 17.5).

use std::fmt;

use serde_json::Value;

use crate::fingerprint::{
    Base64UrlError, decode_base64url, decode_base64url_bytes, encode_base64url,
};
use crate::json::{self, InvalidMember, Member, object_of};
use crate::limits::{MAX_CLIENT_ICON_SIDE_PX, MAX_ICON_PNG_BYTES};
use crate::message::JsonObject;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
/// Signature, then the `IHDR` chunk: length 13, type, width, height.
const PNG_IHDR_PREFIX: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];

/// Base64url of the first 16 bytes of SHA-256 over the PNG bytes (17.5 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconHash([u8; 16]);

impl IconHash {
    /// Accepts canonical base64url of exactly 16 bytes, as every `bytes16`.
    pub fn parse(text: &str) -> Result<IconHash, Base64UrlError> {
        decode_base64url(text).map(IconHash)
    }

    /// The hash of `png`.
    pub fn of(png: &[u8]) -> IconHash {
        let digest = ring::digest::digest(&ring::digest::SHA256, png);
        let mut hash = [0u8; 16];
        hash.copy_from_slice(&digest.as_ref()[..16]);
        IconHash(hash)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub(crate) fn decode_value(value: &Value, name: &'static str) -> Member<IconHash> {
        value
            .as_str()
            .and_then(|text| IconHash::parse(text).ok())
            .ok_or(InvalidMember(name))
    }

    pub(crate) fn encode_value(self) -> Value {
        self.to_string().into()
    }
}

impl fmt::Display for IconHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&encode_base64url(&self.0))
    }
}

/// One entry of `icons` in the result of `icon.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub hash: IconHash,
    /// The PNG bytes, at most 65536. Untrusted until [`Icon::verify`] accepts them.
    pub png: Vec<u8>,
}

/// Why a client drops an icon (17.5 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IconRejection {
    #[error("SHA-256 of the PNG does not match the hash")]
    HashMismatch,
    #[error("not a PNG with an IHDR chunk")]
    NotPng,
    #[error("PNG is larger than 256 x 256 pixels")]
    Bounds,
}

impl Icon {
    /// The client check of 17.5 rule 3: the hash matches and the PNG header, read before any
    /// decoding, gives at most 256 x 256 pixels. The caller still decodes the image as untrusted.
    pub fn verify(&self) -> Result<(), IconRejection> {
        if IconHash::of(&self.png) != self.hash {
            return Err(IconRejection::HashMismatch);
        }
        let (width, height) = png_size(&self.png).ok_or(IconRejection::NotPng)?;
        if width > MAX_CLIENT_ICON_SIDE_PX || height > MAX_CLIENT_ICON_SIDE_PX {
            return Err(IconRejection::Bounds);
        }
        Ok(())
    }
}

/// Width and height from the `IHDR` chunk, which a valid PNG has first. Zero is not valid.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.get(..24)?;
    if header[..8] != PNG_SIGNATURE || header[8..16] != PNG_IHDR_PREFIX {
        return None;
    }
    let read = |start: usize| {
        let bytes: [u8; 4] = header[start..start + 4].try_into().ok()?;
        Some(u32::from_be_bytes(bytes)).filter(|side| *side != 0)
    };
    Some((read(16)?, read(20)?))
}

/// The `result` of `icon.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconGetResult {
    /// One entry per hash the server has, in request order.
    pub icons: Vec<Icon>,
    /// The hashes the server does not have.
    pub missing: Vec<IconHash>,
}

impl IconGetResult {
    pub fn encode(&self) -> JsonObject {
        let icons: Vec<Value> = self
            .icons
            .iter()
            .map(|icon| {
                Value::Object(object_of([
                    ("hash", icon.hash.encode_value()),
                    ("png", encode_base64url(&icon.png).into()),
                ]))
            })
            .collect();
        let missing: Vec<Value> = self
            .missing
            .iter()
            .map(|hash| hash.encode_value())
            .collect();
        object_of([("icons", icons.into()), ("missing", missing.into())])
    }

    /// Checks presence, types and the PNG size limit. It does not run [`Icon::verify`].
    pub fn decode(result: &JsonObject) -> Result<IconGetResult, InvalidMember> {
        Ok(IconGetResult {
            icons: json::object_array(result, "icons", decode_icon)?,
            missing: json::array(result, "missing")?
                .iter()
                .map(|hash| IconHash::decode_value(hash, "missing"))
                .collect::<Member<Vec<IconHash>>>()?,
        })
    }
}

fn decode_icon(icon: &JsonObject) -> Member<Icon> {
    const PNG: &str = "png";
    let png = decode_base64url_bytes(json::string(icon, PNG)?)
        .ok()
        .filter(|png| png.len() <= MAX_ICON_PNG_BYTES)
        .ok_or(InvalidMember(PNG))?;
    Ok(Icon {
        hash: IconHash::parse(json::string(icon, "hash")?).map_err(|_| InvalidMember("hash"))?,
        png,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&PNG_IHDR_PREFIX);
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png
    }

    fn icon(png: Vec<u8>) -> Icon {
        Icon {
            hash: IconHash::of(&png),
            png,
        }
    }

    #[test]
    fn hash_round_trip_and_strictness() {
        let hash = IconHash::of(b"x");
        assert_eq!(IconHash::parse(&hash.to_string()), Ok(hash));
        for bad in [
            "",
            "firefox",
            "AAAAAAAAAAAAAAAAAAAAAB",
            "AAAAAAAAAAAAAAAAAAAAAA==",
        ] {
            assert!(IconHash::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn verify_reads_the_bounds_from_the_header() {
        assert_eq!(icon(png_header(256, 256)).verify(), Ok(()));
        assert_eq!(
            icon(png_header(257, 1)).verify(),
            Err(IconRejection::Bounds)
        );
        assert_eq!(
            icon(png_header(1, 257)).verify(),
            Err(IconRejection::Bounds)
        );
        assert_eq!(icon(png_header(0, 1)).verify(), Err(IconRejection::NotPng));
        assert_eq!(
            icon(png_header(1, 1)[..23].to_vec()).verify(),
            Err(IconRejection::NotPng)
        );
        assert_eq!(
            icon(b"GIF89a".to_vec()).verify(),
            Err(IconRejection::NotPng)
        );
        let mut wrong = icon(png_header(1, 1));
        wrong.hash = IconHash::of(b"other");
        assert_eq!(wrong.verify(), Err(IconRejection::HashMismatch));
    }

    #[test]
    fn decode_refuses_a_png_above_the_limit() {
        let result = |size: usize| {
            let png = encode_base64url(&vec![0u8; size]);
            let hash = IconHash::of(b"x").to_string();
            let text = format!(r#"{{"icons":[{{"hash":"{hash}","png":"{png}"}}],"missing":[]}}"#);
            IconGetResult::decode(&serde_json::from_str(&text).expect("test JSON"))
        };
        assert!(result(MAX_ICON_PNG_BYTES).is_ok());
        assert_eq!(result(MAX_ICON_PNG_BYTES + 1), Err(InvalidMember("png")));
    }
}
