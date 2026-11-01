//! Seekable, bounded view of an embedded archive without copying its payload.
use std::io::{self, Read, Seek, SeekFrom};

pub struct PayloadReader<R> {
    reader: R,
    start: u64,
    length: u64,
    position: u64,
}
impl<R: Read + Seek> PayloadReader<R> {
    pub fn new(mut reader: R, start: u64, length: u64) -> io::Result<Self> {
        let size = reader.seek(SeekFrom::End(0))?;
        if start.checked_add(length).is_none_or(|end| end > size) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "payload outside source",
            ));
        }
        reader.seek(SeekFrom::Start(start))?;
        Ok(Self {
            reader,
            start,
            length,
            position: 0,
        })
    }
    pub fn size(&self) -> u64 {
        self.length
    }
}
impl<R: Read + Seek> Read for PayloadReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = (self.length.saturating_sub(self.position)).min(buffer.len() as u64) as usize;
        if length == 0 {
            return Ok(0);
        }
        let count = self.reader.read(&mut buffer[..length])?;
        self.position += count as u64;
        Ok(count)
    }
}
impl<R: Read + Seek> Seek for PayloadReader<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.length) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        let target = u64::try_from(target)
            .ok()
            .filter(|n| *n <= self.length)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek outside payload"))?;
        self.reader.seek(SeekFrom::Start(self.start + target))?;
        self.position = target;
        Ok(target)
    }
}
