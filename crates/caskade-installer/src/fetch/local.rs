//! Files that are already near (#236): a source the caller provides, asked
//! before the download's own, such as another of the user's devices on the
//! local network that downloaded the same file. Whatever it hands over is
//! checked as a download from the source would be (its size and SHA-256
//! where the request names them; the pipeline reads it like any download),
//! and anything that fails is discarded so the source is used instead.

use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{DownloadRequest, DownloadedFile};

pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// Bytes so far and the total, as a file arrives.
pub type Progress<'a> = dyn Fn(u64, u64) + Send + Sync + 'a;

/// A file a [`LocalSource`] found, and what its source says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Near {
    pub path: PathBuf,
    /// Its SHA-256 (lowercase hex), as the source downloaded it.
    pub sha256: String,
    pub size: u64,
    /// The validator the source downloaded it with, if any.
    pub validator: Option<String>,
}

/// Somewhere a download may already be.
pub trait LocalSource: Send + Sync {
    /// A file with `request`'s content, written into `dir`; `None` to
    /// download it from its source. `progress` hears the bytes so far and
    /// the total while it arrives.
    fn find<'a>(
        &'a self,
        request: &'a DownloadRequest,
        dir: &'a Path,
        cancel: &'a CancellationToken,
        progress: &'a Progress<'a>,
    ) -> LocalFuture<'a, Option<Near>>;
    /// A download from the source finished and checked out: keep it for
    /// others, if this source does that.
    fn keep<'a>(
        &'a self,
        request: &'a DownloadRequest,
        file: &'a DownloadedFile,
    ) -> LocalFuture<'a, ()>;
}

/// `near` as the download `request` asks for, when its size and SHA-256
/// match both what the request names and what the source said it was;
/// `None` (with the file removed) otherwise.
pub fn accept(request: &DownloadRequest, near: &Near) -> Option<DownloadedFile> {
    let checked = (|| {
        let expected = super::engine::expected_checksum(request).ok()?;
        let (sha256, size) = digest(&near.path).ok()?;
        if size != near.size || request.size.is_some_and(|wanted| wanted != size) {
            return None;
        }
        if sha256 != near.sha256 || expected.is_some_and(|wanted| wanted != sha256) {
            return None;
        }
        Some(DownloadedFile {
            path: near.path.clone(),
            source_url: request.url.clone(),
            final_url: request.url.clone(),
            size,
            sha256,
            insecure_transport: false,
            validator: near.validator.clone(),
        })
    })();
    if checked.is_none() {
        let _ = std::fs::remove_file(&near.path);
    }
    checked
}

/// A file's SHA-256 (lowercase hex) and size.
pub fn digest(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hash.update(&buffer[..read]);
    }
    Ok((hex(&hash.finalize()), size))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(size: Option<u64>, sha256: Option<String>) -> DownloadRequest {
        DownloadRequest {
            size,
            sha256,
            ..DownloadRequest::new("https://example.org/game.zip".parse().unwrap())
        }
    }

    #[test]
    fn a_file_is_taken_only_when_its_size_and_digest_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.zip");
        std::fs::write(&path, b"the game").unwrap();
        let (sha256, size) = digest(&path).unwrap();
        let near = |sha256: &str, size| Near {
            path: path.clone(),
            sha256: sha256.into(),
            size,
            validator: Some("\"v1\"".into()),
        };
        let taken = accept(
            &request(Some(size), Some(sha256.clone())),
            &near(&sha256, size),
        )
        .unwrap();
        assert_eq!((taken.size, taken.sha256.as_str()), (8, sha256.as_str()));
        assert_eq!(taken.validator.as_deref(), Some("\"v1\""));
        // The request names nothing: what the source said it was decides.
        assert!(accept(&request(None, None), &near(&sha256, size)).is_some());
        // The wrong size or digest, the request's or the source's: discarded.
        assert!(accept(&request(Some(9), None), &near(&sha256, size)).is_none());
        assert!(!path.exists());
        for (request, near) in [
            (request(None, Some("0".repeat(64))), near(&sha256, size)),
            (request(None, None), near(&"0".repeat(64), size)),
            (request(None, None), near(&sha256, 9)),
        ] {
            std::fs::write(&path, b"the game").unwrap();
            assert!(accept(&request, &near).is_none());
            assert!(!path.exists());
        }
    }
}
