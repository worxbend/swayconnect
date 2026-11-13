//! PNG header checks, made before any decoding.

use swayconnect_protocol::limits::{MAX_ICON_PNG_BYTES, MAX_ICON_SIDE_PX};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
/// The `IHDR` chunk that a valid PNG has first: length 13, then its type.
const IHDR_PREFIX: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];

pub(crate) fn has_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(&SIGNATURE)
}

/// Width and height the `IHDR` chunk declares. `None`: not a PNG, or a zero side.
pub(crate) fn declared_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = bytes.get(..24)?;
    if header[..8] != SIGNATURE || header[8..16] != IHDR_PREFIX {
        return None;
    }
    let side = |start: usize| {
        let field: [u8; 4] = header[start..start + 4].try_into().ok()?;
        Some(u32::from_be_bytes(field)).filter(|side| *side != 0)
    };
    Some((side(16)?, side(20)?))
}

/// A PNG the daemon may serve as it is (`docs/PROTOCOL.md` 17.5): at most 65536 bytes and
/// at most 128 x 128 pixels by its header.
pub(crate) fn is_servable(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_ICON_PNG_BYTES
        && declared_size(bytes)
            .is_some_and(|(width, height)| width <= MAX_ICON_SIDE_PX && height <= MAX_ICON_SIDE_PX)
}

#[cfg(test)]
pub(crate) fn header(width: u32, height: u32) -> Vec<u8> {
    let mut png = SIGNATURE.to_vec();
    png.extend_from_slice(&IHDR_PREFIX);
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_decides_before_any_decoding() {
        assert_eq!(declared_size(&header(48, 32)), Some((48, 32)));
        assert_eq!(declared_size(&header(0, 32)), None);
        assert_eq!(declared_size(b"<svg/>"), None);
        assert!(is_servable(&header(128, 128)));
        assert!(!is_servable(&header(129, 1)));
        let mut too_long = header(16, 16);
        too_long.resize(MAX_ICON_PNG_BYTES + 1, 0);
        assert!(!is_servable(&too_long));
    }
}
