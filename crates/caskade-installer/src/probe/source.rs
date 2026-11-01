use std::io::{self, Read, Seek, SeekFrom};

use super::{CountingReader, ProbeError, ReadStats};

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

/// Bounded random-access view supplied to format probes. The underlying reader
/// cannot be accessed directly, so plugin reads cannot bypass counting/budgets.
pub struct ProbeSource<'a> {
    reader: CountingReader<&'a mut dyn ReadSeek>,
    size: u64,
    budget: u64,
}

// Archive parsers can consume this view directly as Read + Seek without gaining
// access to the raw file. Streaming and random-access reads share one budget.
impl Read for ProbeSource<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let position = self.reader.stream_position()?;
        let available = self.size.saturating_sub(position);
        if available == 0 {
            return Ok(0);
        }
        let remaining = self.budget.saturating_sub(self.stats().bytes_read);
        if remaining == 0 {
            return Err(io::Error::other(ProbeError::ReadBudgetExceeded {
                limit: self.budget,
            }));
        }
        let length = (buffer.len() as u64).min(available).min(remaining) as usize;
        self.reader.read(&mut buffer[..length])
    }
}

impl Seek for ProbeSource<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.reader.seek(position)
    }
}

impl<'a> ProbeSource<'a> {
    pub fn new(reader: &'a mut dyn ReadSeek, budget: u64) -> Result<Self, ProbeError> {
        let mut reader = CountingReader::new(reader);
        let size = reader.seek(SeekFrom::End(0))?;
        Ok(Self {
            reader,
            size,
            budget,
        })
    }

    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn stats(&self) -> ReadStats {
        self.reader.stats()
    }

    /// Read at most `length` bytes, clipped at EOF. A request beyond EOF is an
    /// error, and allocation is checked against the remaining budget first.
    pub fn read_at(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, ProbeError> {
        let available = self
            .size
            .checked_sub(offset)
            .ok_or(ProbeError::InvalidOffset {
                offset,
                size: self.size,
            })?;
        let length = available.min(length as u64);
        let attempted = self
            .stats()
            .bytes_read
            .checked_add(length)
            .ok_or(ProbeError::ReadBudgetExceeded { limit: self.budget })?;
        if attempted > self.budget {
            return Err(ProbeError::ReadBudgetExceeded { limit: self.budget });
        }
        self.reader.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0; length as usize];
        let mut filled = 0;
        while filled < data.len() {
            match self.reader.read(&mut data[filled..]) {
                Ok(0) => {
                    data.truncate(filled);
                    break;
                }
                Ok(count) => filled += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(data)
    }
}
