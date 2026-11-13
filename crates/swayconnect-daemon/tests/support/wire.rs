//! A client that speaks frames by hand, for what the client library refuses to do: a proof
//! over the wrong fingerprint, a replayed frame, silence, bytes that are no frame.

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use swayconnect_protocol::frame::{FrameDecoder, encode_frame};
use swayconnect_protocol::limits::{ALPN_ID, MAX_FRAME_S2C_BYTES, PROTOCOL_VERSION};
use swayconnect_protocol::proof::{Context, client_proof, pair_proof};
use swayconnect_protocol::{Auth, DeviceId, JsonObject, Message, Pair};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::{TcpSocket, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

const CLIENT_NONCE: [u8; 32] = [7; 32];
const READ_CHUNK_BYTES: usize = 4096;

pub struct Wire<S> {
    stream: S,
    decoder: FrameDecoder,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    pub fn new(stream: S) -> Wire<S> {
        Wire {
            stream,
            decoder: FrameDecoder::new(MAX_FRAME_S2C_BYTES),
        }
    }

    /// For reads that must see what the frame decoder would hide: a close with no frame.
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    pub async fn send(&mut self, message: &Value) {
        let object = message.as_object().expect("a JSON object");
        self.send_bytes(&encode_frame(object, u32::MAX).expect("frame"))
            .await;
    }

    /// Sends `payload` as one frame, byte for byte as a vector file holds it.
    pub async fn send_payload(&mut self, payload: &str) {
        let length = u32::try_from(payload.len()).expect("a payload that fits a frame");
        let frame = [&length.to_be_bytes(), payload.as_bytes()].concat();
        self.send_bytes(&frame).await;
    }

    pub async fn send_bytes(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.expect("client write");
        self.stream.flush().await.expect("client flush");
    }

    /// Reads once into the decoder. 0: the server closed the connection.
    pub async fn fill(&mut self) -> usize {
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        // A reset, or TLS without `close_notify`, is a close too.
        let length = self.stream.read(&mut chunk).await.unwrap_or(0);
        self.decoder.push(&chunk[..length]);
        length
    }

    /// The next frame, or `None` when the server closed the connection.
    pub async fn next(&mut self) -> Option<JsonObject> {
        loop {
            if let Some(frame) = self.decoder.next_frame().expect("a valid server frame") {
                return Some(frame);
            }
            if self.fill().await == 0 {
                return None;
            }
        }
    }

    pub async fn receive(&mut self) -> JsonObject {
        self.next().await.expect("a frame, not a close")
    }

    /// Reads `hello` and builds the proof context over `fp`.
    pub async fn hello(&mut self, fp: [u8; 32]) -> Context {
        let Ok(Message::Hello(hello)) = Message::decode(&self.receive().await) else {
            panic!("the first frame is not `hello`");
        };
        Context {
            fp,
            server_nonce: hello.nonce,
            client_nonce: CLIENT_NONCE,
        }
    }
}

/// A correct `auth` frame of the device for `context`.
pub fn auth(device_id: &DeviceId, key: &[u8; 32], context: &Context) -> Value {
    let auth = Auth {
        protocol: PROTOCOL_VERSION,
        device_id: device_id.clone(),
        nonce: context.client_nonce,
        proof: client_proof(key, context, device_id),
        client: None,
    };
    Value::Object(Message::Auth(auth).encode())
}

/// A correct `pair` frame for the pairing secret and `context`.
pub fn pair(secret: &[u8; 32], context: &Context) -> Value {
    let pair = Pair {
        protocol: PROTOCOL_VERSION,
        nonce: context.client_nonce,
        proof: pair_proof(secret, context),
        device_name: "raw client".to_owned(),
        platform: None,
        client: None,
    };
    Value::Object(Message::Pair(pair).encode())
}

/// Takes any certificate: these tests are about what the server does, not about the pin.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not allowed".to_owned()))
    }

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
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A TCP connection to `server`, from the local address `from` when one is given.
pub async fn tcp(server: SocketAddr, from: Option<SocketAddr>) -> std::io::Result<TcpStream> {
    let socket = TcpSocket::new_v4()?;
    if let Some(from) = from {
        socket.bind(from)?;
    }
    socket.connect(server).await
}

/// TLS 1.3 with the protocol's ALPN id over `stream`. `Err`: the server closed instead.
pub async fn tls(stream: TcpStream) -> std::io::Result<Wire<TlsStream<TcpStream>>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 is supported")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_ID.to_vec()];
    config.enable_sni = false;
    let name = ServerName::IpAddress(stream.peer_addr()?.ip().into());
    let stream = TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await?;
    Ok(Wire::new(stream))
}

pub async fn connect(server: SocketAddr) -> Wire<TlsStream<TcpStream>> {
    let stream = tcp(server, None).await.expect("TCP connection");
    tls(stream).await.expect("TLS handshake")
}
