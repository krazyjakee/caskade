use super::{PkgError, Result, invalid};
use sha2::Digest;
use std::io::{self, Read};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub style: String,
    pub bytes: Vec<u8>,
}
impl Checksum {
    pub fn parse(style: &str, text: &str) -> Result<Self> {
        let hash = Hasher::new(style)?;
        let text = text.trim();
        if text.len() != hash.size() * 2 {
            return Err(invalid("checksum hex length"));
        }
        let mut bytes = Vec::with_capacity(hash.size());
        for pair in text.as_bytes().as_chunks::<2>().0 {
            let pair = std::str::from_utf8(pair).map_err(|_| invalid("checksum encoding"))?;
            bytes.push(u8::from_str_radix(pair, 16).map_err(|_| invalid("checksum hex"))?);
        }
        Ok(Self {
            style: style.into(),
            bytes,
        })
    }
}
pub enum Hasher {
    None,
    Sha1(sha1::Sha1),
    Md5(md5::Md5),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}
impl Hasher {
    pub fn new(style: &str) -> Result<Self> {
        Ok(match style {
            "none" => Self::None,
            "sha1" => Self::Sha1(sha1::Sha1::new()),
            "md5" => Self::Md5(md5::Md5::new()),
            "sha256" => Self::Sha256(sha2::Sha256::new()),
            "sha512" => Self::Sha512(sha2::Sha512::new()),
            _ => return Err(PkgError::Unsupported(format!("checksum algorithm {style}"))),
        })
    }
    pub fn size(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Sha1(_) => 20,
            Self::Md5(_) => 16,
            Self::Sha256(_) => 32,
            Self::Sha512(_) => 64,
        }
    }
    pub fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::None => (),
            Self::Sha1(hash) => hash.update(bytes),
            Self::Md5(hash) => hash.update(bytes),
            Self::Sha256(hash) => hash.update(bytes),
            Self::Sha512(hash) => hash.update(bytes),
        }
    }
    pub fn finish(self) -> Vec<u8> {
        match self {
            Self::None => vec![],
            Self::Sha1(hash) => hash.finalize().to_vec(),
            Self::Md5(hash) => hash.finalize().to_vec(),
            Self::Sha256(hash) => hash.finalize().to_vec(),
            Self::Sha512(hash) => hash.finalize().to_vec(),
        }
    }
}
pub struct Verified<R> {
    inner: R,
    expected: u64,
    actual: u64,
    checksum: Option<(Hasher, Vec<u8>)>,
    checked: bool,
}
impl<R> Verified<R> {
    pub fn new(inner: R, expected: u64, checksum: Option<&Checksum>) -> Result<Self> {
        Ok(Self {
            inner,
            expected,
            actual: 0,
            checksum: checksum
                .map(|checksum| -> Result<_> {
                    Ok((Hasher::new(&checksum.style)?, checksum.bytes.clone()))
                })
                .transpose()?,
            checked: false,
        })
    }
    pub fn expected(&self) -> u64 {
        self.expected
    }
    pub fn complete(&self) -> bool {
        self.actual == self.expected && self.checked
    }
    fn verify(&mut self) -> io::Result<()> {
        if !self.checked {
            if let Some((hash, expected)) = self.checksum.take()
                && hash.finish() != expected
            {
                return Err(io::Error::other(PkgError::Checksum));
            }
            self.checked = true;
        }
        Ok(())
    }
}
impl<R: Read> Read for Verified<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let size = buffer.len().min(64 * 1024);
        let count = self.inner.read(&mut buffer[..size])?;
        self.actual = self
            .actual
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other(PkgError::Limit))?;
        if self.actual > self.expected || (count == 0 && self.actual != self.expected) {
            return Err(io::Error::other(invalid("XAR member size mismatch")));
        }
        if let Some((hasher, _)) = &mut self.checksum {
            hasher.update(&buffer[..count]);
        }
        if self.actual == self.expected {
            self.verify()?;
        }
        Ok(count)
    }
}
