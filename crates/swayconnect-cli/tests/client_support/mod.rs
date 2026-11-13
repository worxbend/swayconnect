//! In-test rustls server and vector access, shared by the `client_*` test files.

use std::io::ErrorKind;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::sign::{CertifiedKey, SingleCertAndKey};
use serde_json::Value;
use swayconnect_cli::client::{ClientError, Connector, Profile, Session};
use swayconnect_protocol::DeviceId;
use swayconnect_protocol::frame::{FrameDecoder, encode_frame};
use swayconnect_protocol::limits::ALPN_ID;
use swayconnect_protocol::proof::RandomError;
use swayconnect_protocol::uri::PairingUri;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../protocol/vectors/v1/");

pub fn vector(file: &str) -> Value {
    let text = std::fs::read_to_string(format!("{VECTORS}{file}")).expect("vector file");
    serde_json::from_str(&text).expect("vector JSON")
}

/// The frame payloads of one transcript, in order.
pub fn transcript(name: &str) -> Vec<String> {
    let frames = &vector(&format!("transcripts/{name}.json"))["frames"];
    let frames = frames.as_array().expect("frames");
    frames
        .iter()
        .map(|frame| frame["payload"].as_str().expect("payload").to_owned())
        .collect()
}

pub fn parse(payload: &str) -> Value {
    serde_json::from_str(payload).expect("payload JSON")
}

pub fn with_member(payload: &str, name: &str, value: Value) -> String {
    let mut message = parse(payload);
    message[name] = value;
    message.to_string()
}

pub fn hex32(value: &Value) -> [u8; 32] {
    let text = value.as_str().expect("hex string");
    let bytes: Vec<u8> = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex digits"))
        .collect();
    bytes.try_into().expect("32 bytes")
}

pub fn pairing_nonce() -> Result<[u8; 32], RandomError> {
    Ok(hex32(
        &vector("proofs.json")["inputs"]["pairing"]["client_nonce"],
    ))
}

pub fn later_nonce() -> Result<[u8; 32], RandomError> {
    Ok(hex32(
        &vector("proofs.json")["inputs"]["later"]["client_nonce"],
    ))
}

fn identity(member: &str) -> Vec<u8> {
    let encoded = vector("fingerprint.json")[0][member]
        .as_str()
        .expect("base64")
        .to_owned();
    STANDARD.decode(encoded).expect("standard base64")
}

pub fn vector_fp() -> [u8; 32] {
    hex32(&vector("fingerprint.json")[0]["fp_hex"])
}

/// Presents the vector certificate and signs with the named key of `fingerprint.json`.
/// A resolver is used because `with_single_cert` refuses a key that does not fit.
pub fn server_config(key_member: &str, alpn: &[&[u8]]) -> ServerConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity(key_member)));
    let signing_key = rustls::crypto::ring::sign::any_ecdsa_type(&key).expect("P-256 key");
    let certificate = CertificateDer::from(identity("cert_der_b64"));
    let certified = CertifiedKey::new(vec![certificate], signing_key);
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(SingleCertAndKey::from(certified)));
    config.alpn_protocols = alpn.iter().map(|id| id.to_vec()).collect();
    config
}

pub fn good_config() -> ServerConfig {
    server_config("key_pkcs8_b64", &[ALPN_ID])
}

pub enum Step {
    Send(String),
    /// Waits for one more frame from the client.
    Receive,
    WaitFor(oneshot::Receiver<()>),
}

/// What the server got from the client inside TLS.
pub struct Received {
    pub frames: Vec<Value>,
    pub bytes: usize,
}

pub struct Server {
    pub port: u16,
    /// `Err` when the TLS handshake failed.
    task: JoinHandle<std::io::Result<Received>>,
}

impl Server {
    pub async fn start(config: ServerConfig, script: Vec<Step>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local address").port();
        let task = tokio::spawn(serve(listener, config, script));
        Server { port, task }
    }

    pub async fn received(self) -> std::io::Result<Received> {
        self.task.await.expect("server task")
    }
}

struct Peer {
    stream: TlsStream<TcpStream>,
    decoder: FrameDecoder,
    received: Received,
}

impl Peer {
    /// Reads once and decodes every complete frame. `false` at the end of the stream.
    async fn read_more(&mut self) -> std::io::Result<bool> {
        let mut buffer = [0u8; 4096];
        let count = match self.stream.read(&mut buffer).await {
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => 0,
            Err(error) => return Err(error),
        };
        self.received.bytes += count;
        self.decoder.push(&buffer[..count]);
        while let Some(frame) = self.decoder.next_frame().expect("client frame") {
            self.received.frames.push(Value::Object(frame));
        }
        Ok(count > 0)
    }
}

async fn serve(
    listener: TcpListener,
    config: ServerConfig,
    script: Vec<Step>,
) -> std::io::Result<Received> {
    let (tcp, _) = listener.accept().await?;
    let stream = TlsAcceptor::from(Arc::new(config)).accept(tcp).await?;
    let mut peer = Peer {
        stream,
        decoder: FrameDecoder::new(u32::MAX),
        received: Received {
            frames: Vec::new(),
            bytes: 0,
        },
    };
    let mut expected_frames = 0;
    for step in script {
        match step {
            Step::Send(payload) => {
                let Value::Object(message) = parse(&payload) else {
                    panic!("script payload is not an object");
                };
                let frame = encode_frame(&message, u32::MAX).expect("frame");
                peer.stream.write_all(&frame).await?;
                peer.stream.flush().await?;
            }
            Step::Receive => {
                expected_frames += 1;
                while peer.received.frames.len() < expected_frames {
                    assert!(peer.read_more().await?, "client closed before its frame");
                }
            }
            Step::WaitFor(signal) => signal.await.expect("signal"),
        }
    }
    while peer.read_more().await? {}
    Ok(peer.received)
}

pub fn profile(port: u16, fp: [u8; 32]) -> Profile {
    let proofs = vector("proofs.json");
    let device_id = proofs["inputs"]["device_id"].as_str().expect("device id");
    Profile {
        host: "127.0.0.1".to_owned(),
        port,
        fp,
        device_id: DeviceId::parse(device_id).expect("device id"),
        key: hex32(&proofs["expect"]["k"]),
        server_name: "desk".to_owned(),
    }
}

pub fn pairing_uri(port: u16) -> PairingUri {
    PairingUri {
        host: "127.0.0.1".to_owned(),
        port,
        fp: vector_fp(),
        secret: hex32(&vector("proofs.json")["inputs"]["s"]),
        name: None,
    }
}

pub fn pairing_connector() -> Connector {
    Connector::new(Some("swayconnect-android/0.1.0".to_owned())).with_nonce_source(pairing_nonce)
}

pub fn auth_connector() -> Connector {
    Connector::new(Some("swayconnect-cli/0.1.0".to_owned())).with_nonce_source(later_nonce)
}

pub async fn connect(server: &Server) -> Result<Session, ClientError> {
    auth_connector()
        .connect(&profile(server.port, vector_fp()))
        .await
}
