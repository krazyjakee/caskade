use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
};

/// A seekable concatenation without buffering volumes or copying them to disk.
pub(super) struct Volumes {
    files: Vec<File>,
    offsets: Vec<u64>,
    size: u64,
    position: u64,
}
impl Volumes {
    pub fn open(paths: &[PathBuf]) -> io::Result<Self> {
        let mut files = Vec::new();
        let mut offsets = Vec::new();
        let mut size = 0_u64;
        for path in paths {
            let file = File::open(path)?;
            offsets.push(size);
            size = size
                .checked_add(file.metadata()?.len())
                .ok_or_else(|| io::Error::other("volume length overflow"))?;
            files.push(file);
        }
        Ok(Self {
            files,
            offsets,
            size,
            position: 0,
        })
    }
    pub fn size(&self) -> u64 {
        self.size
    }
}
impl Read for Volumes {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.size || buffer.is_empty() {
            return Ok(0);
        }
        let index = self
            .offsets
            .partition_point(|offset| *offset <= self.position)
            .saturating_sub(1);
        let end = self.offsets.get(index + 1).copied().unwrap_or(self.size);
        let count = buffer.len().min((end - self.position) as usize);
        self.files[index].seek(SeekFrom::Start(self.position - self.offsets[index]))?;
        let count = self.files[index].read(&mut buffer[..count])?;
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for Volumes {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.size) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        self.position = u64::try_from(target)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?;
        Ok(self.position)
    }
}
