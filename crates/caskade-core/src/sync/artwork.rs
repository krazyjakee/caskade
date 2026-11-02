//! The user's artwork files as `sync::blobs` (#273). An `artwork` change
//! waits here until its file is in the cache; the blob exchange then asks
//! the other device for it, by the artwork's content hash, and the change
//! applies once it has arrived.
//!
//! A device serves only files its own user artwork uses. What it sends is
//! its cache's full-size image, re-encoded when it was imported: the
//! receiver decodes it within the import limits and keeps it under the same
//! hash (`ArtworkCache::import_received`).

use async_trait::async_trait;

use super::blobs::{BlobKind, BlobWriter};
use super::policy::Category;
use crate::artwork::{ArtworkCache, MAX_INPUT_BYTES};
use crate::db::LibraryStore;
use crate::{Error, Result};

/// Artwork files, served from and received into the cache.
pub struct ArtworkBlobs {
    pub store: LibraryStore,
    pub cache: ArtworkCache,
}

fn artwork_error(error: crate::artwork::ArtworkError) -> Error {
    Error::Io(std::io::Error::other(error))
}

#[async_trait]
impl BlobKind for ArtworkBlobs {
    fn name(&self) -> &'static str {
        "artwork"
    }

    fn category(&self) -> Category {
        Category::Artwork
    }

    async fn wanted(&self) -> Result<Vec<String>> {
        let mut wanted = Vec::new();
        for hash in self.store.sync_wanted_artwork().await? {
            if !matches!(self.cache.cached(&hash), Ok(Some(_))) {
                wanted.push(hash);
            }
        }
        Ok(wanted)
    }

    async fn has(&self, hash: &str) -> bool {
        self.store
            .sync_has_user_artwork(hash)
            .await
            .unwrap_or(false)
            && matches!(self.cache.cached(hash), Ok(Some(_)))
    }

    async fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let cache = self.cache.clone();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || cache.read_original(&hash, offset, len))
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))?
            .map_err(artwork_error)
    }

    async fn receive(&self, hash: &str) -> Result<Box<dyn BlobWriter>> {
        Ok(Box::new(Receiving {
            cache: self.cache.clone(),
            hash: hash.to_owned(),
            bytes: Vec::new(),
            too_big: false,
        }))
    }
}

struct Receiving {
    cache: ArtworkCache,
    hash: String,
    bytes: Vec<u8>,
    too_big: bool,
}

#[async_trait]
impl BlobWriter for Receiving {
    async fn write(&mut self, chunk: &[u8]) -> Result<()> {
        if self.bytes.len() + chunk.len() > MAX_INPUT_BYTES {
            self.too_big = true;
            self.bytes = Vec::new();
        } else if !self.too_big {
            self.bytes.extend_from_slice(chunk);
        }
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<bool> {
        if self.too_big {
            return Ok(false);
        }
        match self.cache.import_received(self.hash, self.bytes).await {
            Ok(_) => Ok(true),
            Err(error) => {
                tracing::warn!(%error, "artwork from another device wasn't kept");
                Ok(false)
            }
        }
    }
}
