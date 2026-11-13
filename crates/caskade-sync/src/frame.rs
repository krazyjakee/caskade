//! One JSON object per line, read with a bound this build sets.
//!
//! Nothing here trusts a length a peer wrote: there is no length prefix, and a
//! line that grows past the reader's bound fails as [`Error::Malformed`]
//! before more of it is buffered. The bound is per call, so each message in
//! a protocol states how large it may be.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};

use crate::error::{Error, Result};

/// How much is read from the stream at a time.
const CHUNK: usize = 1024;

/// The sending half of a framed stream.
#[derive(Debug)]
pub struct FrameWriter {
    send: SendStream,
}

impl FrameWriter {
    pub(crate) fn new(send: SendStream) -> Self {
        Self { send }
    }

    /// Writes `message` as one line.
    pub async fn write<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let mut line = serde_json::to_vec(message).map_err(|_| Error::Malformed)?;
        line.push(b'\n');
        self.write_raw(&line).await
    }

    /// Writes bytes already framed as a line.
    pub(crate) async fn write_raw(&mut self, line: &[u8]) -> Result<()> {
        self.send.write_all(line).await.map_err(Error::interrupted)
    }
}

/// How much of what a peer still sends is read, and dropped, while waiting
/// for it to end its side.
const DRAIN_LIMIT: usize = 1024 * 1024;

/// Ends a conversation so the close that follows can't discard anything the
/// peer still means to read: ends this side's stream, then waits until the
/// peer ends its own (or closes, or goes away), dropping whatever it still
/// sends. A peer ends its side only once its protocol has read what it
/// needed. The caller bounds the wait.
pub(crate) async fn end_together(writer: FrameWriter, mut reader: FrameReader) {
    let FrameWriter { mut send } = writer;
    if send.finish().is_err() {
        return;
    }
    let mut drained = reader.buffer.len();
    while drained <= DRAIN_LIMIT {
        match reader.recv.read_chunk(CHUNK).await {
            Ok(Some(chunk)) => drained += chunk.len(),
            // The peer ended its side, closed or went away.
            Ok(None) | Err(_) => return,
        }
    }
}

/// The receiving half of a framed stream.
#[derive(Debug)]
pub struct FrameReader {
    recv: RecvStream,
    buffer: Vec<u8>,
}

impl FrameReader {
    pub(crate) fn new(recv: RecvStream) -> Self {
        Self {
            recv,
            buffer: Vec::new(),
        }
    }

    /// Reads one line of at most `max` bytes (newline excluded) and parses it.
    ///
    /// A longer line, bytes that aren't the expected JSON, or a stream that
    /// ends mid-line are all [`Error::Malformed`], with no detail: the bytes
    /// came from a peer.
    pub async fn read<T: DeserializeOwned>(&mut self, max: usize) -> Result<T> {
        let line = self.read_line(max).await?;
        serde_json::from_slice(&line).map_err(|_| Error::Malformed)
    }

    pub(crate) async fn read_line(&mut self, max: usize) -> Result<Vec<u8>> {
        loop {
            if let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
                if end > max {
                    return Err(Error::Malformed);
                }
                let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                line.pop();
                return Ok(line);
            }
            if self.buffer.len() > max {
                return Err(Error::Malformed);
            }
            match self
                .recv
                .read_chunk(CHUNK)
                .await
                .map_err(Error::interrupted)?
            {
                Some(chunk) => self.buffer.extend_from_slice(&chunk),
                None => return Err(Error::Malformed),
            }
        }
    }
}
