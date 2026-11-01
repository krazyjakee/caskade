use super::{
    PkgError, Result, Toc, XarEntry,
    digest::{Hasher, Verified},
    invalid, parse_toc,
};
use crate::formats::archive::PayloadReader;
use std::io::{Cursor, Read, Seek, SeekFrom};
#[derive(Debug, Clone, Copy)]
pub struct XarLimits {
    pub max_toc_bytes: usize,
    pub max_compressed_toc_bytes: usize,
    pub max_member_bytes: u64,
    pub max_expansion_ratio: u64,
    pub decoder_bytes: u64,
}
impl Default for XarLimits {
    fn default() -> Self {
        Self {
            max_toc_bytes: 16 * 1024 * 1024,
            max_compressed_toc_bytes: 2 * 1024 * 1024,
            max_member_bytes: 100 * 1024 * 1024 * 1024,
            max_expansion_ratio: 1000,
            decoder_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Xar {
    pub toc: Toc,
    pub heap_offset: u64,
    pub size: u64,
    pub limits: XarLimits,
}
fn be(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 256 + u64::from(*byte))
}
impl Xar {
    pub fn inspect(reader: &mut (impl Read + Seek), limits: XarLimits) -> Result<Self> {
        let size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let mut header = [0; 28];
        reader.read_exact(&mut header)?;
        if &header[..4] != b"xar!" || be(&header[6..8]) != 1 {
            return Err(invalid("XAR header magic/version"));
        }
        let header_size = be(&header[4..6]);
        let compressed = be(&header[8..16]);
        let expanded = be(&header[16..24]);
        let algorithm = be(&header[24..28]);
        if !(28..=4096).contains(&header_size)
            || compressed == 0
            || compressed > limits.max_compressed_toc_bytes as u64
            || expanded > limits.max_toc_bytes as u64
        {
            return Err(PkgError::Limit);
        }
        let heap = header_size.checked_add(compressed).ok_or(PkgError::Limit)?;
        if heap > size {
            return Err(invalid("truncated XAR TOC"));
        }
        reader.seek(SeekFrom::Start(header_size))?;
        let mut raw = vec![0; compressed as usize];
        reader.read_exact(&mut raw)?;
        let mut decoded = Vec::new();
        let mut decoder = flate2::read::ZlibDecoder::new(Cursor::new(&raw));
        decoder
            .by_ref()
            .take(limits.max_toc_bytes as u64 + 1)
            .read_to_end(&mut decoded)?;
        if decoded.len() as u64 != expanded
            || decoded.len() > limits.max_toc_bytes
            || decoder.total_in() != compressed
        {
            return Err(invalid("XAR TOC size mismatch"));
        }
        let toc = parse_toc(&decoded, size - heap)?;
        let style = match algorithm {
            0 => "none",
            1 => "sha1",
            2 => "md5",
            3 => "sha256",
            4 => "sha512",
            _ => return Err(PkgError::Unsupported("XAR checksum algorithm".into())),
        };
        if algorithm != 0 {
            let (recorded_style, offset, length) = toc
                .checksum
                .as_ref()
                .ok_or_else(|| invalid("missing TOC checksum"))?;
            let mut digest = Hasher::new(style)?;
            if recorded_style != style || *length != digest.size() as u64 {
                return Err(invalid("inconsistent TOC checksum algorithm"));
            }
            reader.seek(SeekFrom::Start(heap + offset))?;
            let mut expected = vec![0; *length as usize];
            reader.read_exact(&mut expected)?;
            digest.update(&raw);
            if digest.finish() != expected {
                return Err(PkgError::Checksum);
            }
        } else if toc
            .checksum
            .as_ref()
            .is_some_and(|(style, _, length)| style != "none" || *length != 0)
        {
            return Err(invalid("unexpected TOC checksum"));
        }
        Ok(Self {
            toc,
            heap_offset: heap,
            size,
            limits,
        })
    }
    pub fn entry(&self, path: &std::path::Path) -> Option<&XarEntry> {
        self.toc
            .entries
            .iter()
            .find(|entry| entry.path == path && !entry.directory)
    }
    pub fn open_entry<'a, R: Read + Seek + 'a>(
        &self,
        source: &'a mut R,
        entry: &XarEntry,
    ) -> Result<Box<dyn Read + 'a>> {
        if entry.directory {
            return Err(invalid("cannot read XAR directory"));
        }
        if entry.size > self.limits.max_member_bytes
            || u128::from(entry.size)
                > u128::from(entry.length) * u128::from(self.limits.max_expansion_ratio)
        {
            return Err(PkgError::Limit);
        }
        let raw = PayloadReader::new(source, self.heap_offset + entry.offset, entry.length)?;
        let raw = Verified::new(raw, entry.length, entry.archived.as_ref())?;
        let decoded = match entry.encoding.as_str() {
            "application/octet-stream" => Decode::Plain(raw),
            // XAR calls its zlib stream x-gzip despite the absent gzip header.
            "application/x-gzip" => Decode::Zlib(flate2::read::ZlibDecoder::new(raw)),
            "application/x-bzip2" => Decode::Bzip2(bzip2::read::BzDecoder::new(raw)),
            "application/x-xz" => Decode::Xz(Box::new(lzma_rust2::XzReader::new_mem_limit(
                raw,
                true,
                (self.limits.decoder_bytes / 1024).min(u64::from(u32::MAX)) as u32,
            ))),
            _ => {
                return Err(PkgError::Unsupported(format!(
                    "XAR member encoding {}",
                    entry.encoding
                )));
            }
        };
        Ok(Box::new(Verified::new(
            decoded,
            entry.size,
            entry.extracted.as_ref(),
        )?))
    }
    pub fn read_metadata(
        &self,
        source: &mut (impl Read + Seek),
        entry: &XarEntry,
        max: usize,
    ) -> Result<Vec<u8>> {
        if entry.size > max as u64 {
            return Err(PkgError::Limit);
        }
        let mut bytes = Vec::new();
        self.open_entry(source, entry)?
            .take(max as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err(PkgError::Limit);
        }
        Ok(bytes)
    }
}

enum Decode<R: Read> {
    Plain(R),
    Zlib(flate2::read::ZlibDecoder<R>),
    Bzip2(bzip2::read::BzDecoder<R>),
    Xz(Box<lzma_rust2::XzReader<R>>),
}
impl<R: Read> Read for Decode<Verified<R>> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = match self {
            Self::Plain(reader) => reader.read(buffer)?,
            Self::Zlib(reader) => reader.read(buffer)?,
            Self::Bzip2(reader) => reader.read(buffer)?,
            Self::Xz(reader) => reader.read(buffer)?,
        };
        if count == 0 {
            let complete = match self {
                Self::Plain(reader) => reader.complete(),
                Self::Zlib(reader) => {
                    reader.get_ref().complete() && reader.total_in() == reader.get_ref().expected()
                }
                Self::Bzip2(reader) => {
                    reader.get_ref().complete() && reader.total_in() == reader.get_ref().expected()
                }
                Self::Xz(reader) => reader.inner().complete(),
            };
            if !complete {
                return Err(std::io::Error::other(invalid(
                    "XAR compressed member was not completely consumed",
                )));
            }
        }
        Ok(count)
    }
}
