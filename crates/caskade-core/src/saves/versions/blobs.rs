//! Save files by content (#239): `<data>/saves/blobs/<2 hex>/<64 hex>`, each
//! named by its SHA-256, so a file that is the same in many versions is kept
//! once.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{Error, Result};

const SCHEME: &str = "sha256:";

/// A content-addressed folder of save files. Cheap to clone.
#[derive(Debug, Clone)]
pub struct BlobStore {
    dir: PathBuf,
}

/// `sha256:<hex>` of `bytes`.
pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{SCHEME}{}", hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The hex digest of a well-formed hash; nothing else names a file.
fn digest(hash: &str) -> Option<&str> {
    let digest = hash.strip_prefix(SCHEME)?;
    (digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(digest)
}

/// `sha256:<hex>` and size of a file. Blocking.
pub fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut input = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((format!("{SCHEME}{}", hex(&hasher.finalize())), size))
}

impl BlobStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where `hash` is kept; `None` for a malformed hash.
    pub fn path(&self, hash: &str) -> Option<PathBuf> {
        let digest = digest(hash)?;
        Some(self.dir.join(&digest[..2]).join(digest))
    }

    pub fn has(&self, hash: &str) -> bool {
        self.path(hash).is_some_and(|path| path.is_file())
    }

    fn partial(&self) -> Result<(PathBuf, fs::File)> {
        fs::create_dir_all(&self.dir)?;
        let path = self
            .dir
            .join(format!(".partial-{}", uuid::Uuid::new_v4().simple()));
        let file = fs::File::create(&path)?;
        Ok((path, file))
    }

    /// Moves a finished partial file into place as `hash`.
    fn keep(&self, partial: &Path, hash: &str) -> Result<()> {
        let path = self
            .path(hash)
            .ok_or_else(|| Error::Invalid("invalid save blob hash".into()))?;
        if path.is_file() {
            fs::remove_file(partial)?;
            return Ok(());
        }
        fs::create_dir_all(path.parent().unwrap_or(&self.dir))?;
        fs::rename(partial, &path)?;
        Ok(())
    }

    /// Copies `source` in and returns its hash and size. Blocking.
    pub fn put_file(&self, source: &Path) -> Result<(String, u64)> {
        let mut input = fs::File::open(source)?;
        let (partial, mut output) = self.partial()?;
        let result = (|| {
            let mut hasher = Sha256::new();
            let mut buffer = vec![0; 64 * 1024];
            let mut size = 0u64;
            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                output.write_all(&buffer[..read])?;
                size += read as u64;
            }
            output.sync_all()?;
            Ok::<_, Error>((format!("{SCHEME}{}", hex(&hasher.finalize())), size))
        })();
        match result {
            Ok((hash, size)) => {
                self.keep(&partial, &hash)?;
                Ok((hash, size))
            }
            Err(error) => {
                let _ = fs::remove_file(&partial);
                Err(error)
            }
        }
    }

    /// Keeps `bytes` and returns their hash. Blocking.
    pub fn put_bytes(&self, bytes: &[u8]) -> Result<String> {
        let hash = hash_bytes(bytes);
        if self.has(&hash) {
            return Ok(hash);
        }
        let (partial, mut file) = self.partial()?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.keep(&partial, &hash)?;
        Ok(hash)
    }

    pub fn get_bytes(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self
            .path(hash)
            .ok_or_else(|| Error::Invalid("invalid save blob hash".into()))?;
        Ok(fs::read(path)?)
    }

    /// Up to `len` bytes of `hash` from `offset`.
    pub fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let path = self
            .path(hash)
            .ok_or_else(|| Error::Invalid("invalid save blob hash".into()))?;
        let mut file = fs::File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = Vec::with_capacity(len);
        file.take(len as u64).read_to_end(&mut buffer)?;
        Ok(buffer)
    }

    /// Writes `hash` to `target`, replacing what's there all at once. Blocking.
    pub fn restore(&self, hash: &str, target: &Path) -> Result<()> {
        let source = self
            .path(hash)
            .filter(|path| path.is_file())
            .ok_or_else(|| Error::Invalid(crate::tr!("A save file hasn't arrived yet.")))?;
        let parent = target
            .parent()
            .ok_or_else(|| Error::Invalid("a save file has no folder".into()))?;
        fs::create_dir_all(parent)?;
        let partial = parent.join(format!(
            ".caskade-restore-{}",
            uuid::Uuid::new_v4().simple()
        ));
        if let Err(error) = fs::copy(&source, &partial).and_then(|_| fs::rename(&partial, target)) {
            let _ = fs::remove_file(&partial);
            return Err(error.into());
        }
        Ok(())
    }

    /// Starts receiving `hash` from another device.
    pub fn receive(&self, hash: &str) -> Result<Incoming> {
        if digest(hash).is_none() {
            return Err(Error::Invalid("invalid save blob hash".into()));
        }
        let (partial, file) = self.partial()?;
        Ok(Incoming {
            store: self.clone(),
            hash: hash.to_owned(),
            partial,
            file,
            hasher: Sha256::new(),
        })
    }

    /// Deletes every blob not in `keep`, and partial files left by a crash.
    /// Blocking.
    pub fn collect_garbage(&self, keep: &HashSet<String>) -> Result<usize> {
        let keep: HashSet<&str> = keep.iter().filter_map(|hash| digest(hash)).collect();
        let mut removed = 0;
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Ok(0);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() && name.len() == 2 {
                for blob in fs::read_dir(&path)?.flatten() {
                    let blob_name = blob.file_name().to_string_lossy().into_owned();
                    if !keep.contains(blob_name.as_str()) && fs::remove_file(blob.path()).is_ok() {
                        removed += 1;
                    }
                }
            } else if name.starts_with(".partial-")
                && entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|modified| {
                        modified.elapsed().unwrap_or_default()
                            > std::time::Duration::from_secs(3600)
                    })
            {
                let _ = fs::remove_file(path);
            }
        }
        Ok(removed)
    }
}

/// A blob arriving from another device: kept only if it matches its hash.
pub struct Incoming {
    store: BlobStore,
    hash: String,
    partial: PathBuf,
    file: fs::File,
    hasher: Sha256,
}

impl Incoming {
    pub fn write(&mut self, chunk: &[u8]) -> Result<()> {
        self.hasher.update(chunk);
        self.file.write_all(chunk)?;
        Ok(())
    }

    /// `true` when it matched and was kept.
    pub fn finish(mut self) -> Result<bool> {
        self.file.sync_all()?;
        let actual = format!("{SCHEME}{}", hex(&self.hasher.finalize_reset()));
        if actual != self.hash {
            let _ = fs::remove_file(&self.partial);
            return Ok(false);
        }
        self.store.keep(&self.partial, &self.hash)?;
        Ok(true)
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        // Unfinished: never kept.
        if self.partial.is_file() {
            let _ = fs::remove_file(&self.partial);
        }
    }
}
