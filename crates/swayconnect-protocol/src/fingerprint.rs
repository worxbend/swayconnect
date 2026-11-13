//! Server fingerprint (`docs/PROTOCOL.md` 3.2) and base64url (spec 2.2).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The value is not canonical base64url of the expected number of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not canonical base64url of the expected length")]
pub struct Base64UrlError;

/// `fp`: SHA-256 of the DER encoding of the leaf certificate.
pub fn fingerprint(certificate_der: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, certificate_der);
    let mut fp = [0u8; 32];
    fp.copy_from_slice(digest.as_ref());
    fp
}

/// base64url without padding.
pub fn encode_base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes exactly `N` bytes. Refuses padding, a character outside the alphabet, a wrong
/// length and non-zero trailing bits.
pub fn decode_base64url<const N: usize>(text: &str) -> Result<[u8; N], Base64UrlError> {
    let decoded = URL_SAFE_NO_PAD.decode(text).map_err(|_| Base64UrlError)?;
    <[u8; N]>::try_from(decoded).map_err(|_| Base64UrlError)
}

/// Decodes any number of bytes, with the same strictness as [`decode_base64url`].
pub fn decode_base64url_bytes(text: &str) -> Result<Vec<u8>, Base64UrlError> {
    URL_SAFE_NO_PAD.decode(text).map_err(|_| Base64UrlError)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP_B64URL: &str = "qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg";

    #[test]
    fn round_trip() {
        let fp: [u8; 32] = decode_base64url(FP_B64URL).expect("vector value decodes");
        assert_eq!(encode_base64url(&fp), FP_B64URL);
    }

    #[test]
    fn refuses_non_canonical_values() {
        let padded = format!("{FP_B64URL}=");
        let trailing_bits = FP_B64URL.replace("wLg", "wLh");
        let standard_alphabet = FP_B64URL.replace('_', "/");
        for bad in [
            padded.as_str(),
            &trailing_bits,
            &standard_alphabet,
            "qlEI",
            "",
        ] {
            assert_eq!(decode_base64url::<32>(bad), Err(Base64UrlError), "{bad}");
        }
        assert_eq!(decode_base64url::<16>(FP_B64URL), Err(Base64UrlError));
    }
}
