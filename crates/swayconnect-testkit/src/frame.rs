//! The 14-byte `i3-ipc` framing, written here a second time on purpose: the fake must
//! not share code with `swayconnect-sway`, or a framing bug would cancel itself out.

use std::io::{self, ErrorKind};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const MAGIC: &[u8; 6] = b"i3-ipc";
const HEADER_LEN: usize = 14;
/// Larger than any request a client of sway sends; stops a garbage length from allocating.
const MAX_REQUEST_PAYLOAD: u32 = 1 << 20;

pub(crate) struct Frame {
    pub message_type: u32,
    pub payload: Vec<u8>,
}

pub(crate) async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Frame> {
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let (magic, rest) = header.split_at(MAGIC.len());
    if magic != MAGIC {
        return Err(io::Error::new(ErrorKind::InvalidData, "bad magic"));
    }
    let (length, message_type) = rest.split_at(4);
    let length = u32::from_ne_bytes(length.try_into().map_err(io::Error::other)?);
    let message_type = u32::from_ne_bytes(message_type.try_into().map_err(io::Error::other)?);
    if length > MAX_REQUEST_PAYLOAD {
        return Err(io::Error::new(ErrorKind::InvalidData, "payload too long"));
    }
    let mut payload = vec![0u8; length as usize];
    reader.read_exact(&mut payload).await?;
    Ok(Frame {
        message_type,
        payload,
    })
}

pub(crate) async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    message_type: u32,
    payload: &[u8],
) -> io::Result<()> {
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "payload too long"))?;
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&length.to_ne_bytes());
    frame.extend_from_slice(&message_type.to_ne_bytes());
    frame.extend_from_slice(payload);
    // One write, so a reply and an event never interleave inside a frame.
    writer.write_all(&frame).await
}
