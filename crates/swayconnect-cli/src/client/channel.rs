//! Frames over the pinned TLS stream (`docs/PROTOCOL.md` 3.3, 3.4).

use std::io::ErrorKind;

use swayconnect_protocol::frame::{EncodeError, FrameDecoder, encode_frame};
use swayconnect_protocol::limits::{
    MAX_FRAME_C2S_BEFORE_WELCOME_BYTES, MAX_FRAME_S2C_HANDSHAKE_BYTES,
};
use swayconnect_protocol::{JsonObject, Limits, Message};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use super::error::ClientError;
use super::tls;

const READ_BUFFER_BYTES: usize = 8192;

pub(super) struct Channel {
    stream: TlsStream<TcpStream>,
    decoder: FrameDecoder,
    send_limit: u32,
}

impl Channel {
    /// Connects and returns the channel with the fingerprint of the presented certificate.
    pub(super) async fn open(
        host: &str,
        port: u16,
        pinned: [u8; 32],
    ) -> Result<(Channel, [u8; 32]), ClientError> {
        let (stream, presented) = tls::connect(host, port, pinned).await?;
        let channel = Channel {
            stream,
            decoder: FrameDecoder::new(MAX_FRAME_S2C_HANDSHAKE_BYTES),
            send_limit: MAX_FRAME_C2S_BEFORE_WELCOME_BYTES,
        };
        Ok((channel, presented))
    }

    /// The limits of both directions change with `welcome` (spec 14).
    pub(super) fn apply_limits(&mut self, limits: &Limits) {
        self.decoder.set_limit(limits.max_frame_s2c);
        self.send_limit = limits.max_frame_c2s;
    }

    /// Cancel safe: bytes already read stay in the decoder.
    pub(super) async fn read_frame(&mut self) -> Result<JsonObject, ClientError> {
        let mut buffer = [0u8; READ_BUFFER_BYTES];
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                return Ok(frame);
            }
            let count = match self.stream.read(&mut buffer).await {
                Ok(count) => count,
                // A peer that drops TCP without close_notify. Clients must handle that (6.4).
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => 0,
                Err(error) => return Err(error.into()),
            };
            if count == 0 {
                return Err(ClientError::Closed);
            }
            self.decoder.push(&buffer[..count]);
        }
    }

    pub(super) async fn write_message(&mut self, message: &Message) -> Result<(), ClientError> {
        let frame = encode_frame(&message.encode(), self.send_limit)?;
        self.write_bytes(&frame).await
    }

    /// Sends `payload` as one frame without any check of its content or of the frame limit.
    pub(super) async fn write_raw_frame(&mut self, payload: &[u8]) -> Result<(), ClientError> {
        let length = u32::try_from(payload.len()).map_err(|_| EncodeError::TooLarge {
            length: payload.len(),
            limit: u32::MAX,
        })?;
        let mut frame = length.to_be_bytes().to_vec();
        frame.extend_from_slice(payload);
        self.write_bytes(&frame).await
    }

    async fn write_bytes(&mut self, frame: &[u8]) -> Result<(), ClientError> {
        self.stream.write_all(frame).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// TLS `close_notify`, then TCP close (spec 11 rule 6).
    pub(super) async fn close(mut self) -> Result<(), ClientError> {
        self.stream.shutdown().await?;
        Ok(())
    }
}
