//! Frame codec of the `tls-frames` binding (`docs/PROTOCOL.md` 3.3, 3.4). No I/O.

use crate::message::JsonObject;

const LENGTH_PREFIX_BYTES: usize = 4;
const MIN_PAYLOAD_BYTES: u32 = 2;

/// Why the stream cannot be read any further (spec 3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("bad frame: length under 2, or the payload is not one UTF-8 JSON object")]
    Bad,
    #[error("frame too large")]
    TooLarge,
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("payload of {length} bytes is over the limit of {limit} bytes")]
    TooLarge { length: usize, limit: u32 },
    #[error("JSON encoding failed")]
    Json(#[from] serde_json::Error),
}

/// Length prefix and payload of one message. Refuses a payload over `limit` bytes.
pub fn encode_frame(message: &JsonObject, limit: u32) -> Result<Vec<u8>, EncodeError> {
    let mut frame = vec![0u8; LENGTH_PREFIX_BYTES];
    serde_json::to_writer(&mut frame, message)?;
    let length = frame.len() - LENGTH_PREFIX_BYTES;
    let prefix = u32::try_from(length)
        .ok()
        .filter(|prefix| *prefix <= limit)
        .ok_or(EncodeError::TooLarge { length, limit })?;
    frame[..LENGTH_PREFIX_BYTES].copy_from_slice(&prefix.to_be_bytes());
    Ok(frame)
}

/// Turns the bytes of one direction into JSON objects.
///
/// Call [`FrameDecoder::next_frame`] until it returns `Ok(None)` after every
/// [`FrameDecoder::push`], so that at most one read and one unfinished frame are buffered.
/// After an error the stream position is unknown: the decoder keeps returning that error.
pub struct FrameDecoder {
    limit: u32,
    unread: Vec<u8>,
    failure: Option<FrameError>,
}

impl FrameDecoder {
    pub fn new(limit: u32) -> FrameDecoder {
        FrameDecoder {
            limit,
            unread: Vec::new(),
            failure: None,
        }
    }

    /// The limit changes with `welcome` (spec 14).
    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit;
    }

    /// Adds the bytes of one read.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.failure.is_none() {
            self.unread.extend_from_slice(bytes);
        }
    }

    /// The next complete frame, or `Ok(None)` when more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<JsonObject>, FrameError> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let frame = self.take_frame();
        if let Err(failure) = frame {
            self.failure = Some(failure);
            self.unread = Vec::new();
        }
        frame
    }

    fn take_frame(&mut self) -> Result<Option<JsonObject>, FrameError> {
        let Some((prefix, rest)) = self.unread.split_first_chunk::<LENGTH_PREFIX_BYTES>() else {
            return Ok(None);
        };
        // The length is judged before any payload byte is looked at (spec 3.3).
        let length = u32::from_be_bytes(*prefix);
        if length < MIN_PAYLOAD_BYTES {
            return Err(FrameError::Bad);
        }
        if length > self.limit {
            return Err(FrameError::TooLarge);
        }
        let length = usize::try_from(length).map_err(|_| FrameError::TooLarge)?;
        let Some(payload) = rest.get(..length) else {
            return Ok(None);
        };
        let message = serde_json::from_slice::<JsonObject>(payload).map_err(|_| FrameError::Bad)?;
        self.unread.drain(..LENGTH_PREFIX_BYTES + length);
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ping() -> JsonObject {
        serde_json::from_str(r#"{"type":"req","id":5,"op":"ping"}"#).expect("test JSON")
    }

    #[test]
    fn encode_writes_the_big_endian_payload_length() {
        let frame = encode_frame(&ping(), 4096).expect("fits");
        assert_eq!(frame[..4], [0, 0, 0, 0x21]);
        assert_eq!(frame.len(), 4 + 0x21);
    }

    #[test]
    fn encode_refuses_a_payload_over_the_limit() {
        let error = encode_frame(&ping(), 0x20).expect_err("one byte over");
        assert!(matches!(
            error,
            EncodeError::TooLarge {
                length: 0x21,
                limit: 0x20
            }
        ));
        assert!(encode_frame(&ping(), 0x21).is_ok());
    }

    #[test]
    fn decoder_handles_byte_by_byte_input() {
        let frame = encode_frame(&ping(), 4096).expect("fits");
        let mut decoder = FrameDecoder::new(4096);
        for byte in &frame[..frame.len() - 1] {
            decoder.push(std::slice::from_ref(byte));
            assert_eq!(decoder.next_frame(), Ok(None));
        }
        decoder.push(&frame[frame.len() - 1..]);
        assert_eq!(decoder.next_frame(), Ok(Some(ping())));
        assert_eq!(decoder.next_frame(), Ok(None));
    }

    #[test]
    fn decoder_stays_failed_after_an_error() {
        let mut decoder = FrameDecoder::new(4096);
        decoder.push(&[0, 0, 0, 2, b'[', b']']);
        assert_eq!(decoder.next_frame(), Err(FrameError::Bad));
        decoder.push(&encode_frame(&ping(), 4096).expect("fits"));
        assert_eq!(decoder.next_frame(), Err(FrameError::Bad));
    }

    #[test]
    fn a_frame_before_the_bad_one_is_still_delivered() {
        let mut decoder = FrameDecoder::new(4096);
        decoder.push(&encode_frame(&ping(), 4096).expect("fits"));
        decoder.push(&[0, 0, 0, 0]);
        assert_eq!(decoder.next_frame(), Ok(Some(ping())));
        assert_eq!(decoder.next_frame(), Err(FrameError::Bad));
    }

    #[test]
    fn the_limit_can_be_raised_after_welcome() {
        let frame = encode_frame(&ping(), 4096).expect("fits");
        let mut decoder = FrameDecoder::new(0x20);
        decoder.set_limit(0x21);
        decoder.push(&frame);
        assert_eq!(decoder.next_frame(), Ok(Some(ping())));

        let mut strict = FrameDecoder::new(0x20);
        strict.push(&frame[..4]);
        assert_eq!(strict.next_frame(), Err(FrameError::TooLarge));
    }
}
