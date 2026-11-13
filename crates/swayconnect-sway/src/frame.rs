//! The 14-byte sway IPC frame: `"i3-ipc"`, `u32` length, `u32` type, native byte order.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAGIC: &[u8; 6] = b"i3-ipc";
const HEADER_LEN: usize = 14;
const EVENT_BIT: u32 = 1 << 31;
/// A `GET_TREE` reply of a busy desktop is a few hundred kilobytes. A larger length means the
/// stream is out of step, and it must not become an allocation.
const MAX_PAYLOAD_LEN: u32 = 16 * 1024 * 1024;

/// The message types this project sends. Sway never answers an unknown type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageType {
    RunCommand,
    GetWorkspaces,
    Subscribe,
    GetTree,
    GetVersion,
}

impl MessageType {
    pub(crate) fn code(self) -> u32 {
        match self {
            MessageType::RunCommand => 0,
            MessageType::GetWorkspaces => 1,
            MessageType::Subscribe => 2,
            MessageType::GetTree => 4,
            MessageType::GetVersion => 7,
        }
    }
}

/// A frame read from sway: a reply or an event.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) code: u32,
    pub(crate) payload: Vec<u8>,
}

impl Frame {
    pub(crate) fn is_event(&self) -> bool {
        self.code & EVENT_BIT != 0
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum FrameError {
    #[error("sway IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("sway IPC frame does not start with \"i3-ipc\"")]
    BadMagic,
    #[error("sway IPC payload of {0} bytes is over the limit")]
    TooLarge(u64),
}

fn encode(kind: MessageType, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    let len = u32::try_from(payload.len())
        .ok()
        .filter(|len| *len <= MAX_PAYLOAD_LEN)
        .ok_or(FrameError::TooLarge(payload.len() as u64))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&len.to_ne_bytes());
    bytes.extend_from_slice(&kind.code().to_ne_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

pub(crate) async fn write_frame<W>(
    writer: &mut W,
    kind: MessageType,
    payload: &[u8],
) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&encode(kind, payload)?).await?;
    Ok(())
}

fn u32_at(header: &[u8; HEADER_LEN], offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&header[offset..offset + 4]);
    u32::from_ne_bytes(bytes)
}

/// Not cancel safe: only the reader task calls it, and that task is never cancelled half way
/// while the connection is in use.
pub(crate) async fn read_frame<R>(reader: &mut R) -> Result<Frame, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    if &header[..MAGIC.len()] != MAGIC {
        return Err(FrameError::BadMagic);
    }
    let len = u32_at(&header, 6);
    if len > MAX_PAYLOAD_LEN {
        return Err(FrameError::TooLarge(u64::from(len)));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;
    Ok(Frame {
        code: u32_at(&header, 10),
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new variant does not compile here until it is listed, and then the test below sees it.
    const ALL_TYPES: [MessageType; 5] = {
        const fn listed(kind: MessageType) -> MessageType {
            match kind {
                MessageType::RunCommand
                | MessageType::GetWorkspaces
                | MessageType::Subscribe
                | MessageType::GetTree
                | MessageType::GetVersion => kind,
            }
        }
        [
            listed(MessageType::RunCommand),
            listed(MessageType::GetWorkspaces),
            listed(MessageType::Subscribe),
            listed(MessageType::GetTree),
            listed(MessageType::GetVersion),
        ]
    };

    #[tokio::test]
    async fn the_writer_can_only_write_types_0_1_2_4_7() {
        let mut written_codes = Vec::new();
        for kind in ALL_TYPES {
            let mut bytes = Vec::new();
            write_frame(&mut bytes, kind, b"")
                .await
                .expect("write to a Vec");
            assert_eq!(bytes.len(), HEADER_LEN);
            written_codes.push(u32_at(&bytes.try_into().expect("14 bytes"), 10));
        }
        assert_eq!(written_codes, [0, 1, 2, 4, 7]);
    }

    #[tokio::test]
    async fn a_written_frame_has_magic_length_type_and_payload() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, MessageType::GetTree, b"abc")
            .await
            .expect("write to a Vec");
        let mut expected = b"i3-ipc".to_vec();
        expected.extend_from_slice(&3u32.to_ne_bytes());
        expected.extend_from_slice(&4u32.to_ne_bytes());
        expected.extend_from_slice(b"abc");
        assert_eq!(bytes, expected);
    }

    #[tokio::test]
    async fn a_written_frame_reads_back() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, MessageType::GetVersion, b"{}")
            .await
            .expect("write to a Vec");
        let frame = read_frame(&mut bytes.as_slice()).await.expect("read back");
        assert_eq!(
            frame,
            Frame {
                code: 7,
                payload: b"{}".to_vec()
            }
        );
        assert!(!frame.is_event());
    }

    #[tokio::test]
    async fn the_high_bit_marks_an_event() {
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&0u32.to_ne_bytes());
        bytes.extend_from_slice(&0x8000_0003u32.to_ne_bytes());
        let frame = read_frame(&mut bytes.as_slice()).await.expect("read");
        assert!(frame.is_event());
    }

    #[tokio::test]
    async fn bad_magic_is_refused() {
        let bytes = [b'x'; HEADER_LEN];
        let error = read_frame(&mut bytes.as_slice())
            .await
            .expect_err("bad magic");
        assert!(matches!(error, FrameError::BadMagic), "{error}");
    }

    #[tokio::test]
    async fn an_oversize_length_is_refused_before_any_allocation() {
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&u32::MAX.to_ne_bytes());
        bytes.extend_from_slice(&4u32.to_ne_bytes());
        let error = read_frame(&mut bytes.as_slice())
            .await
            .expect_err("too large");
        assert!(
            matches!(error, FrameError::TooLarge(0xFFFF_FFFF)),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_short_frame_is_an_io_error() {
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&5u32.to_ne_bytes());
        bytes.extend_from_slice(&4u32.to_ne_bytes());
        bytes.extend_from_slice(b"ab");
        let error = read_frame(&mut bytes.as_slice()).await.expect_err("short");
        assert!(matches!(error, FrameError::Io(_)), "{error}");
    }
}
