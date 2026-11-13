//! Pairing URI (`docs/PROTOCOL.md` section 4).
//!
//! The URI holds the pairing secret. No error and no `Debug` output here contains URI text.

use std::net::IpAddr;

use crate::fingerprint::{decode_base64url, encode_base64url};
use crate::text::is_display_name;

const SCHEME: &str = "swayconnect";
const AUTHORITY: &str = "pair";
const URI_VERSION: &str = "1";
const KNOWN_PARAMETERS: [&str; 6] = ["v", "host", "port", "fp", "secret", "name"];
const MAX_DNS_NAME_BYTES: usize = 253;
const MAX_DNS_LABEL_BYTES: usize = 63;
const MAX_PORT_DIGITS: usize = 5;

#[derive(Clone, PartialEq, Eq)]
pub struct PairingUri {
    /// IPv4 literal, IPv6 literal without brackets, or DNS name.
    pub host: String,
    pub port: u16,
    pub fp: [u8; 32],
    /// The pairing secret `S`.
    pub secret: [u8; 32],
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    #[error("not a swayconnect pairing URI")]
    NotPairingUri,
    #[error("a parameter is not of the form name=value")]
    MalformedParameter,
    #[error("parameter `{0}` appears twice")]
    Duplicate(&'static str),
    #[error("parameter `{0}` is missing")]
    Missing(&'static str),
    #[error("parameter `{0}` has an invalid value")]
    Invalid(&'static str),
}

impl std::fmt::Debug for PairingUri {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingUri")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl PairingUri {
    pub fn parse(uri: &str) -> Result<PairingUri, UriError> {
        let [v, host, port, fp, secret, name] = known_values(query_of(uri)?)?;
        if required(v, "v")? != URI_VERSION {
            return Err(UriError::Invalid("v"));
        }
        let host = required(host, "host")?;
        if !is_host(&host) {
            return Err(UriError::Invalid("host"));
        }
        let port = parse_port(&required(port, "port")?).ok_or(UriError::Invalid("port"))?;
        let fp = decode_base64url(&required(fp, "fp")?).map_err(|_| UriError::Invalid("fp"))?;
        let secret = decode_base64url(&required(secret, "secret")?)
            .map_err(|_| UriError::Invalid("secret"))?;
        if name.as_deref().is_some_and(|name| !is_display_name(name)) {
            return Err(UriError::Invalid("name"));
        }
        Ok(PairingUri {
            host,
            port,
            fp,
            secret,
            name,
        })
    }

    /// The URI text. It holds the secret: show it to the owner, never log it.
    pub fn to_uri_string(&self) -> String {
        let mut uri = format!(
            "{SCHEME}://{AUTHORITY}?v={URI_VERSION}&host={}&port={}&fp={}&secret={}",
            percent_encode(&self.host),
            self.port,
            encode_base64url(&self.fp),
            encode_base64url(&self.secret),
        );
        if let Some(name) = &self.name {
            uri.push_str("&name=");
            uri.push_str(&percent_encode(name));
        }
        uri
    }
}

/// Checks scheme, authority, empty path and absent fragment; returns the query.
fn query_of(uri: &str) -> Result<&str, UriError> {
    let (scheme, rest) = uri.split_once("://").ok_or(UriError::NotPairingUri)?;
    let (authority, query) = rest.split_once('?').ok_or(UriError::NotPairingUri)?;
    let is_pairing_uri = scheme.eq_ignore_ascii_case(SCHEME)
        && authority.eq_ignore_ascii_case(AUTHORITY)
        && !query.contains('#');
    if is_pairing_uri {
        Ok(query)
    } else {
        Err(UriError::NotPairingUri)
    }
}

/// The percent-decoded values of the known parameters, in the order of `KNOWN_PARAMETERS`.
fn known_values(query: &str) -> Result<[Option<String>; 6], UriError> {
    let mut values: [Option<String>; 6] = Default::default();
    for parameter in query.split('&') {
        let (name, value) = parameter
            .split_once('=')
            .ok_or(UriError::MalformedParameter)?;
        let Some(index) = KNOWN_PARAMETERS.iter().position(|known| *known == name) else {
            continue;
        };
        let name = KNOWN_PARAMETERS[index];
        if values[index].is_some() {
            return Err(UriError::Duplicate(name));
        }
        values[index] = Some(percent_decode(value).ok_or(UriError::Invalid(name))?);
    }
    Ok(values)
}

fn required(value: Option<String>, name: &'static str) -> Result<String, UriError> {
    value.ok_or(UriError::Missing(name))
}

/// `None` for a bad percent sequence or a result that is not UTF-8. `+` stays a plus sign.
fn percent_decode(value: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = hex_digit(bytes.next()?)?;
            let low = hex_digit(bytes.next()?)?;
            decoded.push(high << 4 | low);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// Encodes every byte outside the unreserved set of RFC 3986.
fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// `[1-9][0-9]{0,4}`, at most 65535.
fn parse_port(text: &str) -> Option<u16> {
    let is_plain_decimal = (1..=MAX_PORT_DIGITS).contains(&text.len())
        && !text.starts_with('0')
        && text.bytes().all(|byte| byte.is_ascii_digit());
    if is_plain_decimal {
        text.parse().ok()
    } else {
        None
    }
}

fn is_host(text: &str) -> bool {
    text.parse::<IpAddr>().is_ok() || is_dns_name(text)
}

fn is_dns_name(text: &str) -> bool {
    text.len() <= MAX_DNS_NAME_BYTES && text.split('.').all(is_dns_label)
}

fn is_dns_label(label: &str) -> bool {
    (1..=MAX_DNS_LABEL_BYTES).contains(&label.len())
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "fp=qlEIrQGa3jl_vkIkT7o9ljZE2nx1PCmndOaEJ1yfwLg&secret=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

    fn parse_with(parameters: &str) -> Result<PairingUri, UriError> {
        PairingUri::parse(&format!("swayconnect://pair?{BASE}&{parameters}"))
    }

    #[test]
    fn port_edges() {
        assert_eq!(parse_with("v=1&host=a&port=1").map(|uri| uri.port), Ok(1));
        assert_eq!(
            parse_with("v=1&host=a&port=65535").map(|uri| uri.port),
            Ok(65535)
        );
        for bad in ["65536", "+80", "-1", "", "080", "8 0", "123456"] {
            let result = parse_with(&format!("v=1&host=a&port={bad}"));
            assert_eq!(result, Err(UriError::Invalid("port")), "{bad}");
        }
    }

    #[test]
    fn host_forms() {
        for good in ["192.168.1.200", "fd00%3A%3A1", "desk.lan", "my-desk"] {
            assert!(
                parse_with(&format!("v=1&port=1&host={good}")).is_ok(),
                "{good}"
            );
        }
        for bad in [
            "",
            "%5Bfd00%3A%3A1%5D",
            "fe80%3A%3A1%25eth0",
            "a..b",
            "-a",
            "a%20b",
            "a%2Fb",
        ] {
            let result = parse_with(&format!("v=1&port=1&host={bad}"));
            assert_eq!(result, Err(UriError::Invalid("host")), "{bad}");
        }
    }

    #[test]
    fn parameter_form_and_percent_sequences() {
        assert_eq!(
            parse_with("v=1&host=a&port=1&flag"),
            Err(UriError::MalformedParameter)
        );
        assert_eq!(
            parse_with("v=1&host=a&port=1&"),
            Err(UriError::MalformedParameter)
        );
        assert_eq!(
            parse_with("v=1&host=a&port=1&name=%zz"),
            Err(UriError::Invalid("name"))
        );
        assert_eq!(
            parse_with("v=1&host=a&port=1&name=a%2"),
            Err(UriError::Invalid("name"))
        );
        assert_eq!(parse_with("v=%31&host=a&port=1").map(|uri| uri.port), Ok(1));
        assert_eq!(
            parse_with("v=01&host=a&port=1"),
            Err(UriError::Invalid("v"))
        );
        // An unknown parameter is ignored whatever its value is.
        assert!(parse_with("v=1&host=a&port=1&x=%zz").is_ok());
    }

    #[test]
    fn no_query_is_refused() {
        assert_eq!(
            PairingUri::parse("swayconnect://pair"),
            Err(UriError::NotPairingUri)
        );
        assert_eq!(
            PairingUri::parse("swayconnect:pair?v=1"),
            Err(UriError::NotPairingUri)
        );
    }

    #[test]
    fn written_uri_parses_back_with_reserved_characters() {
        let uri = PairingUri {
            host: "fd00::1".to_owned(),
            port: 28414,
            fp: [0xAA; 32],
            secret: [7; 32],
            name: Some("my desk+1 & caf\u{e9}=#?".to_owned()),
        };
        assert_eq!(PairingUri::parse(&uri.to_uri_string()), Ok(uri));
    }

    #[test]
    fn debug_output_has_no_secret() {
        let uri = parse_with("v=1&host=a&port=1&name=desk").expect("valid");
        let debug = format!("{uri:?}");
        assert!(
            !debug.contains("secret") && !debug.contains("0, 1, 2"),
            "{debug}"
        );
    }
}
