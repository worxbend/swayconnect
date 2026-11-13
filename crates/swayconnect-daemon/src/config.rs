//! TOML config and its validation (`docs/DESIGN.md` section 6). Written by slice S6a.

use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use swayconnect_protocol::limits::{DEFAULT_PORT, MAX_NAME_BYTES};
use swayconnect_protocol::text::{is_display_name, replace_unsafe};

use crate::paths::PathError;

const HOSTNAME_FILE: &str = "/proc/sys/kernel/hostname";

/// The validated daemon configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Label shown to clients after authentication. Never part of the certificate.
    pub server_name: String,
    pub listen: Vec<SocketAddr>,
    pub allow_from: Vec<Network>,
    pub read_only: bool,
    /// `None`: discover the sway socket.
    pub sway_socket: Option<PathBuf>,
}

/// Why the daemon cannot start (exit code 2, `docs/DESIGN.md` 4.7).
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("config file {}: {source}", path.display())]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("config: {0}")]
    Config(String),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("identity: {0}")]
    Identity(String),
    #[error("device store: {0}")]
    Devices(String),
    #[error("operating system: {0}")]
    Os(#[from] io::Error),
    #[error("swayconnectd is already running (it listens on {})", .0.display())]
    AlreadyRunning(PathBuf),
}

/// A source network in CIDR form, for `allow_from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Network {
    address: IpAddr,
    prefix_bits: u32,
}

impl Network {
    pub fn contains(&self, peer: IpAddr) -> bool {
        match (self.address, peer) {
            (IpAddr::V4(network), IpAddr::V4(peer)) => {
                let mask = high_bits(u128::from(u32::MAX), 32, self.prefix_bits);
                u128::from(network.to_bits()) & mask == u128::from(peer.to_bits()) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(peer)) => {
                let mask = high_bits(u128::MAX, 128, self.prefix_bits);
                network.to_bits() & mask == peer.to_bits() & mask
            }
            _ => false,
        }
    }
}

/// The top `prefix_bits` of an address that is `width` bits wide.
fn high_bits(all_ones: u128, width: u32, prefix_bits: u32) -> u128 {
    match width.checked_sub(prefix_bits) {
        Some(host_bits) if host_bits < 128 => all_ones >> host_bits << host_bits,
        _ => 0,
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(text: &str) -> Result<Network, String> {
        let invalid = || format!("allow_from entry {text:?} is not a network in CIDR form");
        let (address, prefix) = text.split_once('/').ok_or_else(invalid)?;
        let address: IpAddr = address.parse().map_err(|_| invalid())?;
        let is_plain_number = !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit());
        let prefix_bits: u32 = prefix.parse().map_err(|_| invalid())?;
        let width = if address.is_ipv4() { 32 } else { 128 };
        if !is_plain_number || prefix_bits > width {
            return Err(invalid());
        }
        Ok(Network {
            address,
            prefix_bits,
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawConfig {
    server_name: Option<String>,
    listen: Option<Vec<String>>,
    allow_from: Vec<String>,
    read_only: bool,
    sway: RawSway,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawSway {
    socket: String,
}

impl Config {
    /// Reads and validates the file. A missing file gives the defaults: every key is optional.
    pub fn load(path: &Path) -> Result<Config, StartError> {
        match fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Config::from_toml(""),
            Err(source) => Err(StartError::ConfigRead {
                path: path.to_owned(),
                source,
            }),
        }
    }

    pub fn from_toml(text: &str) -> Result<Config, StartError> {
        let raw: RawConfig =
            toml::from_str(text).map_err(|error| StartError::Config(error.to_string()))?;
        let server_name = match raw.server_name {
            Some(name) => name,
            None => system_hostname()?,
        };
        if !is_display_name(&server_name) {
            return Err(StartError::Config(format!(
                "server_name must be 1 to {MAX_NAME_BYTES} bytes without control or format characters"
            )));
        }
        let listen = match raw.listen {
            Some(addresses) => parse_listen(&addresses)?,
            None => vec![SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_PORT))],
        };
        let allow_from = raw
            .allow_from
            .iter()
            .map(|entry| entry.parse().map_err(StartError::Config))
            .collect::<Result<Vec<Network>, StartError>>()?;
        let has_lan_address = listen.iter().any(|address| !address.ip().is_loopback());
        if has_lan_address && allow_from.is_empty() {
            return Err(StartError::Config(
                "allow_from must list the allowed source networks when listen has a non-loopback address"
                    .to_owned(),
            ));
        }
        Ok(Config {
            server_name,
            listen,
            allow_from,
            read_only: raw.read_only,
            sway_socket: Some(raw.sway.socket)
                .filter(|socket| !socket.is_empty())
                .map(PathBuf::from),
        })
    }
}

fn parse_listen(addresses: &[String]) -> Result<Vec<SocketAddr>, StartError> {
    if addresses.is_empty() {
        return Err(StartError::Config("listen must not be empty".to_owned()));
    }
    addresses
        .iter()
        .map(|text| {
            let address: SocketAddr = text.parse().map_err(|_| {
                StartError::Config(format!("listen address {text:?} is not ip:port"))
            })?;
            if address.ip().to_canonical().is_unspecified() {
                return Err(StartError::Config(format!(
                    "listen address {text:?} is a wildcard; name one address"
                )));
            }
            Ok(address)
        })
        .collect()
}

/// The default server label: the kernel host name, made safe to display.
fn system_hostname() -> Result<String, StartError> {
    let raw = fs::read_to_string(HOSTNAME_FILE).map_err(|error| {
        StartError::Config(format!(
            "server_name is not set and {HOSTNAME_FILE} cannot be read: {error}"
        ))
    })?;
    let mut name = replace_unsafe(raw.trim());
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config.example.toml");

    fn error_text(toml: &str) -> String {
        match Config::from_toml(toml) {
            Ok(config) => panic!("accepted: {config:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn the_documented_example_loads() {
        let config = Config::from_toml(EXAMPLE).expect("example is valid");
        assert_eq!(config.server_name, "desk");
        let expected: Vec<SocketAddr> = vec![
            "127.0.0.1:28414".parse().expect("address"),
            "192.168.1.200:28414".parse().expect("address"),
        ];
        assert_eq!(config.listen, expected);
        assert_eq!(config.allow_from.len(), 1);
        assert!(!config.read_only);
        assert_eq!(config.sway_socket, None);
    }

    #[test]
    fn the_default_listen_address_is_loopback_port_28414() {
        let config = Config::from_toml("").expect("empty config is valid");
        let expected: SocketAddr = "127.0.0.1:28414".parse().expect("address");
        assert_eq!(config.listen, vec![expected]);
        assert!(config.allow_from.is_empty());
        assert!(is_display_name(&config.server_name));
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_a_directory_is_an_error() {
        let root = tempfile::tempdir().expect("temp dir");
        let config = Config::load(&root.path().join("config.toml")).expect("defaults");
        assert_eq!(config.listen.len(), 1);
        assert!(matches!(
            Config::load(root.path()),
            Err(StartError::ConfigRead { .. })
        ));
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        assert!(error_text("read_onli = true").contains("read_onli"));
        assert!(error_text("[sway]\nsoket = \"/x\"").contains("soket"));
    }

    #[test]
    fn a_wildcard_listen_address_is_an_error() {
        for address in ["0.0.0.0:28414", "[::]:28414", "[::ffff:0.0.0.0]:28414"] {
            let toml = format!("listen = [\"{address}\"]\nallow_from = [\"10.0.0.0/8\"]");
            assert!(error_text(&toml).contains("wildcard"), "{address}");
        }
        assert!(error_text("listen = [\"localhost:1\"]").contains("ip:port"));
        assert!(error_text("listen = []").contains("empty"));
    }

    #[test]
    fn a_non_loopback_listen_address_needs_allow_from() {
        assert!(error_text("listen = [\"192.168.1.200:28414\"]").contains("allow_from"));
        assert!(error_text("listen = [\"[fd00::1]:28414\"]").contains("allow_from"));
        Config::from_toml("listen = [\"[::1]:0\", \"127.0.0.2:0\"]").expect("loopback only");
    }

    #[test]
    fn a_server_name_outside_its_limits_is_an_error() {
        assert!(error_text("server_name = \"a\\u001bb\"").contains("server_name"));
        assert!(error_text("server_name = \"\"").contains("server_name"));
        let long = format!("server_name = \"{}\"", "a".repeat(65));
        assert!(error_text(&long).contains("server_name"));
    }

    #[test]
    fn allow_from_entries_must_be_cidr() {
        for entry in [
            "192.168.1.0",
            "192.168.1.0/33",
            "fd00::/129",
            "x/8",
            "10.0.0.0/+8",
        ] {
            let toml = format!("allow_from = [\"{entry}\"]");
            assert!(error_text(&toml).contains("CIDR"), "{entry}");
        }
    }

    #[test]
    fn a_network_contains_only_its_own_addresses() {
        let lan: Network = "192.168.1.0/24".parse().expect("network");
        assert!(lan.contains("192.168.1.77".parse().expect("address")));
        assert!(!lan.contains("192.168.2.77".parse().expect("address")));
        assert!(!lan.contains("fd00::1".parse().expect("address")));

        let ula: Network = "fd00:1:2:3::/64".parse().expect("network");
        assert!(ula.contains("fd00:1:2:3::9".parse().expect("address")));
        assert!(!ula.contains("fd00:1:2:4::9".parse().expect("address")));

        let everything: Network = "0.0.0.0/0".parse().expect("network");
        assert!(everything.contains("203.0.113.5".parse().expect("address")));
        let one_host: Network = "10.0.0.1/32".parse().expect("network");
        assert!(one_host.contains("10.0.0.1".parse().expect("address")));
        assert!(!one_host.contains("10.0.0.2".parse().expect("address")));
    }

    #[test]
    fn a_sway_socket_path_is_kept() {
        let config = Config::from_toml("[sway]\nsocket = \"/tmp/x/sway.sock\"").expect("valid");
        assert_eq!(config.sway_socket, Some(PathBuf::from("/tmp/x/sway.sock")));
    }
}
