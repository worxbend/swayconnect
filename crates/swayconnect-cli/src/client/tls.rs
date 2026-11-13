//! TLS 1.3 with the server certificate pinned by fingerprint (`docs/PROTOCOL.md` 3.1, 3.2).

use std::net::Ipv4Addr;
use std::sync::Arc;

use rustls::client::Resumption;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme};
use swayconnect_protocol::fingerprint::fingerprint;
use swayconnect_protocol::limits::ALPN_ID;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::error::ClientError;

/// Accepts exactly one leaf certificate. Chain, names and dates carry no meaning (3.2 rule 5).
#[derive(Debug)]
struct PinVerifier {
    pinned: [u8; 32],
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity.as_ref()) == self.pinned {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(pin_mismatch())
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not allowed".to_owned()))
    }

    // The certificate is public, so only this signature shows that the peer holds its key
    // (3.2 rule 4). It must stay a real check.
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn pin_mismatch() -> rustls::Error {
    rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

fn client_config(pinned: [u8; 32]) -> Result<ClientConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinVerifier {
        pinned,
        provider: Arc::clone(&provider),
    });
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_ID.to_vec()];
    // A resumed handshake carries no certificate, so the pin check could not run (3.1 rule 10).
    config.resumption = Resumption::disabled();
    config.enable_sni = false;
    Ok(config)
}

/// A verified TLS stream and the fingerprint of the certificate presented on it.
pub(super) async fn connect(
    host: &str,
    port: u16,
    pinned: [u8; 32],
) -> Result<(TlsStream<TcpStream>, [u8; 32]), ClientError> {
    let config = client_config(pinned).map_err(ClientError::Tls)?;
    let tcp = TcpStream::connect((host, port)).await?;
    // SNI is off and the verifier ignores names, so the name is only a placeholder.
    let unused_name = ServerName::IpAddress(Ipv4Addr::UNSPECIFIED.into());
    let stream = TlsConnector::from(Arc::new(config))
        .connect(unused_name, tcp)
        .await
        .map_err(handshake_error)?;
    let session = stream.get_ref().1;
    // Common TLS stacks continue when the peer selects nothing (3.1 rule 9).
    if session.alpn_protocol() != Some(ALPN_ID) {
        return Err(ClientError::NoAlpn);
    }
    // The proofs use the fingerprint computed from this connection, not the pinned copy
    // (3.2 rule 6).
    let leaf = session
        .peer_certificates()
        .and_then(<[CertificateDer<'_>]>::first)
        .ok_or(ClientError::PinMismatch)?;
    let presented = fingerprint(leaf.as_ref());
    Ok((stream, presented))
}

fn handshake_error(error: std::io::Error) -> ClientError {
    let tls_error = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>());
    match tls_error {
        Some(tls_error) if *tls_error == pin_mismatch() => ClientError::PinMismatch,
        Some(tls_error) => ClientError::Tls(tls_error.clone()),
        None => ClientError::Io(error),
    }
}
