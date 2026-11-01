//! Content-addressed artwork, bounded off-thread decoding and cache-only URIs.
mod asset;
mod cache;
#[cfg(feature = "net")]
mod download;
#[cfg(test)]
mod tests;
pub use asset::{AssetResponse, serve};
pub use cache::{ArtworkCache, MAX_INPUT_BYTES};
use serde::{Deserialize, Serialize};

pub const THUMBNAIL_SIZES: [u32; 3] = [96, 320, 960];
pub type Result<T> = std::result::Result<T, ArtworkError>;

#[derive(Debug, thiserror::Error)]
pub enum ArtworkError {
    #[error("artwork I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("artwork decode: {0}")]
    Image(#[from] image::ImageError),
    #[error("artwork metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid artwork: {0}")]
    Invalid(&'static str),
    #[error("artwork limit: {0}")]
    Limit(&'static str),
    #[error("artwork worker failed")]
    Worker,
    #[error("artwork download: {message}")]
    Download {
        failure: DownloadFailure,
        message: String,
    },
}

/// Why an artwork download failed, for deciding when to try it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadFailure {
    /// The host couldn't be reached or didn't answer in time: the network
    /// may be down.
    Unreachable,
    /// The server answered with this HTTP status.
    Status(u16),
    /// The connection broke, too many redirects, …
    Other,
}

impl ArtworkError {
    #[cfg(feature = "net")]
    fn download(error: reqwest::Error) -> Self {
        let failure = if error.is_timeout() || error.is_connect() {
            DownloadFailure::Unreachable
        } else if let Some(status) = error.status() {
            DownloadFailure::Status(status.as_u16())
        } else {
            DownloadFailure::Other
        };
        Self::Download {
            failure,
            message: crate::error::describe_net(error),
        }
    }

    /// Whether the download failed because its host couldn't be reached:
    /// other downloads would likely fail too.
    pub fn unreachable(&self) -> bool {
        matches!(
            self,
            Self::Download {
                failure: DownloadFailure::Unreachable,
                ..
            }
        )
    }

    /// Whether trying again can't help: the server refused the image (a
    /// 4xx other than 408 and 429), or what came isn't usable artwork.
    pub fn permanent(&self) -> bool {
        match self {
            Self::Download {
                failure: DownloadFailure::Status(status),
                ..
            } => (400..500).contains(status) && !matches!(status, 408 | 429),
            Self::Image(_) | Self::Invalid(_) | Self::Limit(_) => true,
            _ => false,
        }
    }
}

/// Cache metadata has no game association: multiple Artwork entities may reuse
/// these exact bytes with distinct opaque IDs, kinds, sources and game IDs.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedArtwork {
    pub content_hash: String,
    pub cached_path: String,
    pub width: u32,
    pub height: u32,
    pub thumbnails: Vec<CachedThumbnail>,
}
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedThumbnail {
    pub size: u32,
    pub path: String,
    pub width: u32,
    pub height: u32,
}
impl CachedArtwork {
    /// Create a distinct domain association while sharing immutable cache
    /// content. Reusing bytes never reuses an entity ID across games or kinds.
    pub fn record(
        &self,
        game_id: impl Into<String>,
        kind: crate::model::ArtworkKind,
        source: impl Into<String>,
    ) -> crate::model::Artwork {
        let mut artwork = crate::model::Artwork::new(
            game_id,
            kind,
            self.content_hash.clone(),
            source,
            self.cached_path.clone(),
            self.width,
            self.height,
        );
        artwork.thumbnails = self
            .thumbnails
            .iter()
            .map(|thumbnail| crate::model::ArtworkThumbnail {
                size: thumbnail.size,
                path: thumbnail.path.clone(),
                width: thumbnail.width,
                height: thumbnail.height,
            })
            .collect();
        artwork
    }

    /// Grids/lists/details explicitly select bounded thumbnails, never a
    /// fallback to the original full-size image.
    pub fn thumbnail_uri(&self, size: u32) -> Option<String> {
        self.thumbnails
            .iter()
            .find(|t| t.size == size)
            .map(|t| format!("caskade-asset://localhost/{}", t.path))
    }
}
pub(super) fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
