//! Count actual bytes read and seek operations without changing I/O behavior.

use std::io::{self, Read, Seek, SeekFrom};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ReadStats {
    pub bytes_read: u64,
    pub read_calls: u64,
    pub seek_calls: u64,
}

#[derive(Debug)]
pub struct CountingReader<R> {
    inner: R,
    stats: ReadStats,
}

impl<R> CountingReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            stats: ReadStats::default(),
        }
    }
    pub fn stats(&self) -> ReadStats {
        self.stats
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stats.read_calls = self.stats.read_calls.saturating_add(1);
        let read = self.inner.read(buffer)?;
        self.stats.bytes_read = self.stats.bytes_read.saturating_add(read as u64);
        Ok(read)
    }
}

impl<R: Seek> Seek for CountingReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.stats.seek_calls = self.stats.seek_calls.saturating_add(1);
        self.inner.seek(position)
    }
}
