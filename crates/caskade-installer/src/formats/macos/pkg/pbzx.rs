use super::{PkgError, Result, invalid};
use std::io::{self, Cursor, Read};
const MAX_CHUNK: u64 = 16 * 1024 * 1024;
/// One bounded chunk at a time. Modern pbzx stores the maximum block size in
/// its header and the inflated/stored lengths per chunk; EOF ends the stream.
pub struct Pbzx<R> {
    inner: R,
    current: Cursor<Vec<u8>>,
    block: u64,
    done: bool,
    total: u64,
    max: u64,
}
impl<R: Read> Pbzx<R> {
    pub fn new(mut inner: R, max: u64) -> Result<Self> {
        let mut header = [0; 12];
        inner.read_exact(&mut header)?;
        let block = u64::from_be_bytes(header[4..12].try_into().unwrap());
        if &header[..4] != b"pbzx" || block == 0 || block > MAX_CHUNK {
            return Err(invalid("pbzx block header"));
        }
        Ok(Self {
            inner,
            current: Cursor::new(vec![]),
            block,
            done: false,
            total: 0,
            max,
        })
    }
    fn chunk(&mut self) -> Result<bool> {
        let mut header = [0; 16];
        if self.inner.read(&mut header[..1])? == 0 {
            self.done = true;
            return Ok(false);
        }
        self.inner.read_exact(&mut header[1..])?;
        let inflated = u64::from_be_bytes(header[..8].try_into().unwrap());
        let stored = u64::from_be_bytes(header[8..].try_into().unwrap());
        if inflated == 0 || inflated > self.block || stored == 0 || stored > MAX_CHUNK + 1024 * 1024
        {
            return Err(PkgError::Limit);
        }
        self.total = self.total.checked_add(inflated).ok_or(PkgError::Limit)?;
        if self.total > self.max {
            return Err(PkgError::Limit);
        }
        let mut raw = vec![0; stored as usize];
        self.inner.read_exact(&mut raw)?;
        let bytes = if stored == inflated {
            raw
        } else {
            if !raw.starts_with(b"\xfd7zXZ\0") {
                return Err(PkgError::Unsupported("pbzx non-XZ chunk".into()));
            }
            let mut decoded = Vec::new();
            lzma_rust2::XzReader::new_mem_limit(Cursor::new(raw), true, 64 * 1024)
                .take(inflated + 1)
                .read_to_end(&mut decoded)?;
            if decoded.len() as u64 != inflated {
                return Err(invalid("pbzx inflated length"));
            }
            decoded
        };
        self.current = Cursor::new(bytes);
        Ok(true)
    }
}
impl<R: Read> Read for Pbzx<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let size = buffer.len().min(64 * 1024);
            let n = self.current.read(&mut buffer[..size])?;
            if n != 0 || self.done {
                return Ok(n);
            }
            self.chunk().map_err(io::Error::other)?;
        }
    }
}
