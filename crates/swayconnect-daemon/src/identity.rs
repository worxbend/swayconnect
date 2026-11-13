//! Certificate and key: create, load, fingerprint, rustls `ServerConfig`. Written by slice S6a.

use std::sync::Arc;

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SerialNumber};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::NoServerSessionStorage;
use swayconnect_protocol::fingerprint::fingerprint;
use swayconnect_protocol::limits::ALPN_ID;
use swayconnect_protocol::proof::random_bytes;

use crate::config::StartError;
use crate::paths::{Paths, read_private, write_atomic};

/// The certificate goes to every unauthenticated peer, so it names nothing about the host.
const COMMON_NAME: &str = "swayconnectd";

/// The server certificate and the TLS configuration built from it and its key.
pub struct Identity {
    fingerprint: [u8; 32],
    tls: Arc<ServerConfig>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Identity").finish_non_exhaustive()
    }
}

impl Identity {
    /// Loads the identity from the data directory, or creates it at first run.
    /// The data directory must exist and be checked already.
    pub fn open(paths: &Paths, uid: u32) -> Result<Identity, StartError> {
        let certificate = read_private(&paths.identity_cert(), uid)?;
        let key = read_private(&paths.identity_key(), uid)?;
        let (certificate, key) = match (certificate, key) {
            (Some(certificate), Some(key)) => (certificate, key),
            (None, None) => {
                let (certificate, key) = create()?;
                write_atomic(&paths.identity_key(), &key)?;
                write_atomic(&paths.identity_cert(), &certificate)?;
                (certificate, key)
            }
            // A new identity would silently unpair every device; the owner decides.
            _ => {
                return Err(StartError::Identity(
                    "only one of the certificate and key files exists; delete it to create a new identity"
                        .to_owned(),
                ));
            }
        };
        Ok(Identity {
            fingerprint: fingerprint(&certificate),
            tls: Arc::new(tls_config(certificate, key)?),
        })
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub fn tls_config(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.tls)
    }
}

/// A self-signed ECDSA P-256 certificate (DER) and its private key (PKCS#8 DER).
fn create() -> Result<(Vec<u8>, Vec<u8>), StartError> {
    let failed = |error: rcgen::Error| StartError::Identity(error.to_string());
    let serial: [u8; 16] =
        random_bytes().map_err(|error| StartError::Identity(error.to_string()))?;
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(failed)?;
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, COMMON_NAME);
    // Fixed dates: the creation time is not in the certificate.
    params.not_before = rcgen::date_time_ymd(2000, 1, 1);
    params.not_after = rcgen::date_time_ymd(9999, 12, 31);
    params.serial_number = Some(SerialNumber::from_slice(&serial));
    let certificate = params.self_signed(&key).map_err(failed)?;
    Ok((certificate.der().to_vec(), key.serialize_der()))
}

/// TLS 1.3 only, one ALPN id, and nothing that lets a handshake skip the certificate.
fn tls_config(certificate: Vec<u8>, key: Vec<u8>) -> Result<ServerConfig, StartError> {
    let failed = |error: rustls::Error| StartError::Identity(error.to_string());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(failed)?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .map_err(failed)?;
    config.alpn_protocols = vec![ALPN_ID.to_vec()];
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;

    use x509_parser::prelude::{FromDer as _, X509Certificate};

    use super::*;

    fn paths_in(root: &Path) -> (Paths, u32) {
        let paths = Paths {
            config_file: root.join("config.toml"),
            data_dir: root.to_owned(),
            runtime_dir: root.join("runtime"),
        };
        (paths, fs::metadata(root).expect("metadata").uid())
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn the_second_run_loads_the_identity_of_the_first() {
        let root = tempfile::tempdir().expect("temp dir");
        let (paths, uid) = paths_in(root.path());

        let first = Identity::open(&paths, uid).expect("create");
        let second = Identity::open(&paths, uid).expect("load");

        assert_eq!(first.fingerprint(), second.fingerprint());
        let certificate = fs::read(paths.identity_cert()).expect("certificate file");
        assert_eq!(first.fingerprint(), fingerprint(&certificate));
        for file in [paths.identity_cert(), paths.identity_key()] {
            assert_eq!(fs::metadata(file).expect("metadata").mode() & 0o777, 0o600);
        }
        assert_eq!(format!("{first:?}"), "Identity { .. }");
    }

    #[test]
    fn the_certificate_says_nothing_about_the_host() {
        let root = tempfile::tempdir().expect("temp dir");
        let (paths, uid) = paths_in(root.path());
        Identity::open(&paths, uid).expect("create");
        let der = fs::read(paths.identity_cert()).expect("certificate file");

        let (rest, certificate) = X509Certificate::from_der(&der).expect("valid DER");
        assert!(rest.is_empty());
        for name in [certificate.subject(), certificate.issuer()] {
            let common_names: Vec<&str> = name
                .iter_common_name()
                .map(|attribute| attribute.as_str().expect("text"))
                .collect();
            assert_eq!(common_names, [COMMON_NAME]);
            assert_eq!(name.iter_attributes().count(), 1);
        }
        assert!(
            certificate
                .subject_alternative_name()
                .expect("extensions parse")
                .is_none()
        );
        // 2000-01-01T00:00:00Z
        assert_eq!(certificate.validity().not_before.timestamp(), 946_684_800);

        let hostname = fs::read_to_string("/proc/sys/kernel/hostname").expect("hostname");
        let hostname = hostname.trim();
        assert!(!hostname.is_empty());
        assert!(!contains(&der, hostname.as_bytes()));
    }

    #[test]
    fn the_tls_config_never_resumes_a_session() {
        let root = tempfile::tempdir().expect("temp dir");
        let (paths, uid) = paths_in(root.path());
        let tls = Identity::open(&paths, uid).expect("create").tls_config();

        assert!(!tls.session_storage.can_cache());
        assert_eq!(tls.send_tls13_tickets, 0);
        assert_eq!(tls.max_early_data_size, 0);
        assert_eq!(tls.alpn_protocols, [ALPN_ID.to_vec()]);
    }

    #[test]
    fn a_lone_key_file_or_a_loose_mode_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let (paths, uid) = paths_in(root.path());
        Identity::open(&paths, uid).expect("create");

        fs::remove_file(paths.identity_cert()).expect("remove certificate");
        assert!(matches!(
            Identity::open(&paths, uid),
            Err(StartError::Identity(_))
        ));

        let other = tempfile::tempdir().expect("temp dir");
        let (paths, uid) = paths_in(other.path());
        Identity::open(&paths, uid).expect("create");
        let loose = std::os::unix::fs::PermissionsExt::from_mode(0o644);
        fs::set_permissions(paths.identity_key(), loose).expect("chmod");
        assert!(matches!(
            Identity::open(&paths, uid),
            Err(StartError::Path(_))
        ));
    }
}
