//! An in-test sway socket in a temp directory, with its own copy of the framing.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

pub(crate) struct TestSocket {
    path: PathBuf,
    listener: UnixListener,
    _dir: TempDir,
}

impl TestSocket {
    pub(crate) fn bind() -> TestSocket {
        let dir = tempfile::tempdir().expect("temp directory");
        let path = dir.path().join("sway.sock");
        let listener = UnixListener::bind(&path).expect("bind in the temp directory");
        TestSocket {
            path,
            listener,
            _dir: dir,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) async fn accept(&self) -> Peer {
        let (stream, _) = self.listener.accept().await.expect("accept");
        Peer(stream)
    }
}

/// The sway side of one connection.
pub(crate) struct Peer(UnixStream);

impl Peer {
    /// Returns the message type and the payload.
    pub(crate) async fn read_request(&mut self) -> (u32, Vec<u8>) {
        let mut header = [0u8; 14];
        self.0
            .read_exact(&mut header)
            .await
            .expect("request header");
        assert_eq!(&header[..6], b"i3-ipc");
        let len = u32::from_ne_bytes(header[6..10].try_into().expect("4 bytes"));
        let code = u32::from_ne_bytes(header[10..14].try_into().expect("4 bytes"));
        let mut payload = vec![0u8; len as usize];
        self.0
            .read_exact(&mut payload)
            .await
            .expect("request payload");
        (code, payload)
    }

    pub(crate) async fn send(&mut self, code: u32, payload: &[u8]) {
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
        bytes.extend_from_slice(&code.to_ne_bytes());
        bytes.extend_from_slice(payload);
        self.send_raw(&bytes).await;
    }

    pub(crate) async fn send_raw(&mut self, bytes: &[u8]) {
        self.0.write_all(bytes).await.expect("write to the client");
    }
}
