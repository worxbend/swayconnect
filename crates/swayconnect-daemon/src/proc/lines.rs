//! [`Lines`] of a child's stdout pipe, for [`Runner::spawn_lines`](super::Runner::spawn_lines).

use tokio::io::AsyncReadExt as _;
use tokio::net::unix::pipe;

use super::{BoxFuture, Lines, MAX_LINE_BYTES, ProcError, READ_CHUNK_BYTES};

/// [`Lines`] of a pipe. The buffer lives in the struct, so a cancelled `next_line` loses
/// nothing.
pub(super) struct PipeLines {
    stream: pipe::Receiver,
    line: Vec<u8>,
    pending: Vec<u8>,
    ended: bool,
}

impl PipeLines {
    pub(super) fn new(stream: pipe::Receiver) -> PipeLines {
        PipeLines {
            stream,
            line: Vec::new(),
            pending: Vec::new(),
            ended: false,
        }
    }

    /// Moves bytes of `pending` into `line` up to a newline; bytes past [`MAX_LINE_BYTES`]
    /// are dropped. Returns a finished line.
    fn take_line(&mut self) -> Option<String> {
        let newline = self.pending.iter().position(|byte| *byte == b'\n');
        let end = newline.unwrap_or(self.pending.len());
        let room = MAX_LINE_BYTES - self.line.len();
        self.line.extend_from_slice(&self.pending[..end.min(room)]);
        self.pending.drain(..newline.map_or(end, |at| at + 1));
        newline.map(|_| self.finish_line())
    }

    fn finish_line(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.line).into_owned();
        self.line.clear();
        line
    }
}

impl Lines for PipeLines {
    fn next_line(&mut self) -> BoxFuture<'_, Result<Option<String>, ProcError>> {
        Box::pin(async move {
            loop {
                if let Some(line) = self.take_line() {
                    return Ok(Some(line));
                }
                if self.ended {
                    let rest = (!self.line.is_empty()).then(|| self.finish_line());
                    return Ok(rest);
                }
                let mut chunk = [0_u8; READ_CHUNK_BYTES];
                let read = self.stream.read(&mut chunk).await?;
                self.ended = read == 0;
                self.pending.extend_from_slice(&chunk[..read]);
            }
        })
    }
}
