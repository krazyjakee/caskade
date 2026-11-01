use super::{DiscError, Result};
use crate::safety::sanitize_path;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataTrack {
    pub file: PathBuf,
    pub sector_bytes: u16,
    pub data_offset: u16,
    pub start_sector: u64,
}
/// Parse only data tracks; audio tracks remain members of the input hand-off.
/// No cue command, filename or metadata is ever executed.
pub fn parse_cue(text: &str) -> Result<DataTrack> {
    if text.len() > 1024 * 1024 || text.contains('\0') {
        return Err(DiscError::Limit("cue sheet"));
    }
    let mut file = None;
    let mut active = false;
    let mut result: Option<DataTrack> = None;
    let mut indexed = false;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            continue;
        };
        match command.to_ascii_uppercase().as_str() {
            "FILE" => {
                let tail = line.trim_start()[4..].trim_start();
                let (name, kind) = if let Some(tail) = tail.strip_prefix('"') {
                    tail.split_once('"')
                        .ok_or_else(|| DiscError::Invalid("unterminated cue filename".into()))?
                } else {
                    tail.split_once(char::is_whitespace)
                        .ok_or_else(|| DiscError::Invalid("missing cue file type".into()))?
                };
                if !kind.trim().eq_ignore_ascii_case("BINARY") {
                    return Err(DiscError::Unsupported("non-binary cue file".into()));
                }
                file = Some(sanitize_path(name)?);
                active = false;
            }
            "TRACK" => {
                let number: u8 = words
                    .next()
                    .unwrap_or("")
                    .parse()
                    .map_err(|_| DiscError::Invalid("cue track number".into()))?;
                if number == 0 || number > 99 {
                    return Err(DiscError::Invalid("cue track number".into()));
                }
                let mode = words.next().unwrap_or("").to_ascii_uppercase();
                active = mode != "AUDIO";
                if active {
                    if result.is_some() {
                        return Err(DiscError::Unsupported("multiple data tracks".into()));
                    }
                    let (sector_bytes, data_offset) = match mode.as_str() {
                        "MODE1/2048" | "MODE2/2048" => (2048, 0),
                        "MODE1/2352" => (2352, 16),
                        "MODE2/2352" => (2352, 24),
                        _ => return Err(DiscError::Unsupported(format!("cue mode {mode}"))),
                    };
                    result = Some(DataTrack {
                        file: file
                            .clone()
                            .ok_or_else(|| DiscError::Invalid("TRACK before FILE".into()))?,
                        sector_bytes,
                        data_offset,
                        start_sector: 0,
                    });
                }
            }
            "INDEX" if active => {
                if words.next() != Some("01") {
                    continue;
                }
                if indexed {
                    return Err(DiscError::Invalid("duplicate data INDEX 01".into()));
                }
                let fields = words
                    .next()
                    .unwrap_or("")
                    .split(':')
                    .map(str::parse::<u64>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|_| DiscError::Invalid("cue index".into()))?;
                if fields.len() != 3 || fields[1] >= 60 || fields[2] >= 75 || fields[0] > 999 {
                    return Err(DiscError::Invalid("cue index".into()));
                }
                result.as_mut().expect("active data track").start_sector =
                    (fields[0] * 60 + fields[1]) * 75 + fields[2];
                indexed = true;
            }
            _ => (),
        }
    }
    if !indexed {
        return Err(DiscError::Invalid("data track has no INDEX 01".into()));
    }
    result.ok_or_else(|| DiscError::Unsupported("cue contains only audio".into()))
}

/// Bounded logical 2048-byte filesystem view over cooked or raw CD sectors.
/// Audio and MODE2 Form2 sectors are not interpreted as filesystem bytes.
pub struct SectorReader<R> {
    inner: R,
    track: DataTrack,
    length: u64,
    position: u64,
}
impl<R: Read + Seek> SectorReader<R> {
    pub fn new(mut inner: R, track: DataTrack) -> io::Result<Self> {
        let physical = inner.seek(SeekFrom::End(0))?;
        let stride = u64::from(track.sector_bytes);
        if !matches!(
            (track.sector_bytes, track.data_offset),
            (2048, 0) | (2352, 16 | 24)
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid CD sector geometry",
            ));
        }
        let start = track
            .start_sector
            .checked_mul(stride)
            .ok_or_else(|| io::Error::other("CD offset overflow"))?;
        let remaining = physical
            .checked_sub(start)
            .ok_or_else(|| io::Error::other("CD track starts outside input"))?;
        if remaining % stride != 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated CD sector",
            ));
        }
        Ok(Self {
            inner,
            track,
            length: remaining / stride * 2048,
            position: 0,
        })
    }
}
impl<R: Read + Seek> Read for SectorReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.position == self.length {
            return Ok(0);
        }
        let sector = self.position / 2048;
        let within = (self.position % 2048) as usize;
        let base = (self.track.start_sector + sector)
            .checked_mul(u64::from(self.track.sector_bytes))
            .ok_or_else(|| io::Error::other("CD offset overflow"))?;
        if self.track.sector_bytes == 2352 {
            self.inner.seek(SeekFrom::Start(base))?;
            let mut header = [0; 24];
            self.inner.read_exact(&mut header)?;
            if header[..12] != [0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0]
                || (self.track.data_offset == 16 && header[15] != 1)
                || (self.track.data_offset == 24
                    && (header[15] != 2
                        || header[18] & 0x20 != 0
                        || header[16..20] != header[20..24]))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported raw CD sector",
                ));
            }
        }
        self.inner.seek(SeekFrom::Start(
            base + u64::from(self.track.data_offset) + within as u64,
        ))?;
        let count = output
            .len()
            .min(2048 - within)
            .min((self.length - self.position) as usize);
        self.inner.read_exact(&mut output[..count])?;
        self.position += count as u64;
        Ok(count)
    }
}
impl<R: Read + Seek> Seek for SectorReader<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.length) + i128::from(offset),
        };
        if target < 0 || target > i128::from(self.length) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CD seek outside track",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}
