//! The framed stream of one connection: frame limits, write timeout, the one frame before
//! `welcome` (`docs/PROTOCOL.md` 5.1).

use std::io;
use std::time::Duration;

use swayconnect_protocol::frame::{FrameDecoder, encode_frame};
use swayconnect_protocol::limits::{
    MAX_FRAME_C2S_BEFORE_WELCOME_BYTES, MAX_FRAME_S2C_HANDSHAKE_BYTES,
};
use swayconnect_protocol::{JsonObject, Message};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::time::timeout;

const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const LENGTH_PREFIX_BYTES: usize = 4;
pub(super) const READ_CHUNK_BYTES: usize = 4096;

/// The connection cannot be written to any more: close without another frame.
pub(super) struct Unwritable;

pub(super) enum FirstFrameError {
    /// The client closed, or the stream failed.
    Gone,
    /// Bad or too large, or more bytes than one frame (spec 5.1 rule 6).
    Refused,
}

pub(super) struct Wire<S> {
    pub(super) stream: S,
    pub(super) decoder: FrameDecoder,
    pub(super) send_limit: u32,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    pub(super) fn new(stream: S) -> Wire<S> {
        Wire {
            stream,
            decoder: FrameDecoder::new(MAX_FRAME_C2S_BEFORE_WELCOME_BYTES),
            send_limit: MAX_FRAME_S2C_HANDSHAKE_BYTES,
        }
    }

    pub(super) async fn send(&mut self, message: Message) -> Result<(), Unwritable> {
        let frame = encode_frame(&message.encode(), self.send_limit).map_err(|error| {
            tracing::error!(%error, "frame not sent");
            Unwritable
        })?;
        let write = async {
            self.stream.write_all(&frame).await?;
            self.stream.flush().await
        };
        match timeout(WRITE_TIMEOUT, write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(_) => Err(Unwritable),
        }
    }

    /// Sends the last frame of the connection and closes it.
    pub(super) async fn send_last(&mut self, message: Message) {
        if self.send(message).await.is_ok() {
            // The peer may be gone already; there is nobody left to tell.
            let _ = timeout(WRITE_TIMEOUT, self.stream.shutdown()).await;
        }
    }

    /// Reads once into the decoder. `Ok(0)`: the client closed. Cancel safe.
    pub(super) async fn fill(&mut self) -> io::Result<usize> {
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        let length = self.stream.read(&mut chunk).await?;
        self.decoder.push(&chunk[..length]);
        Ok(length)
    }

    /// The one frame a client may send before `welcome`.
    pub(super) async fn first_frame(&mut self) -> Result<JsonObject, FirstFrameError> {
        let mut prefix = Vec::with_capacity(LENGTH_PREFIX_BYTES);
        let mut received = 0usize;
        loop {
            let mut chunk = [0u8; READ_CHUNK_BYTES];
            let length = match self.stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return Err(FirstFrameError::Gone),
                Ok(length) => length,
            };
            let missing = LENGTH_PREFIX_BYTES - prefix.len();
            prefix.extend(chunk[..length].iter().take(missing));
            received += length;
            self.decoder.push(&chunk[..length]);
            match self.decoder.next_frame() {
                Ok(None) => {}
                Ok(Some(frame)) if received == frame_bytes(&prefix) => return Ok(frame),
                Ok(Some(_)) | Err(_) => return Err(FirstFrameError::Refused),
            }
        }
    }
}

/// Length on the wire of the frame that starts with `prefix`. 0 without a whole prefix.
fn frame_bytes(prefix: &[u8]) -> usize {
    let Ok(prefix) = <[u8; LENGTH_PREFIX_BYTES]>::try_from(prefix) else {
        return 0;
    };
    usize::try_from(u32::from_be_bytes(prefix)).map_or(0, |payload| LENGTH_PREFIX_BYTES + payload)
}
