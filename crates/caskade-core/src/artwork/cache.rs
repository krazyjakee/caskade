use super::{ArtworkError, CachedArtwork, CachedThumbnail, Result, THUMBNAIL_SIZES};
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Clones share the cache; all instances use the application resource queue.
#[derive(Clone)]
pub struct ArtworkCache {
    pub(super) dir: Arc<Dir>,
    /// Whether metadata images may come from local and private addresses;
    /// only tests that serve them from a local server allow it.
    pub(super) local_metadata: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    artwork: CachedArtwork,
}

impl ArtworkCache {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            dir: Arc::new(Dir::open_ambient_dir(root, cap_std::ambient_authority())?),
            local_metadata: false,
        })
    }

    /// Lets [`Self::download_metadata`] reach local and private addresses,
    /// for tests that serve metadata images from a local server.
    pub fn allowing_local_metadata(mut self) -> Self {
        self.local_metadata = true;
        self
    }

    /// Hashing, decoding, resizing and disk I/O all run off the async/UI thread.
    pub async fn import_bytes(&self, bytes: Vec<u8>) -> Result<CachedArtwork> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(ArtworkError::Limit("encoded artwork exceeds 16 MiB"));
        }
        let cache = self.clone();
        caskade_jobs::blocking(crate::tr!("Preparing artwork"), move || {
            cache.import_sync(&bytes)
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }

    pub(super) fn import_sync(&self, bytes: &[u8]) -> Result<CachedArtwork> {
        if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
            return Err(ArtworkError::Limit("empty or oversized encoded artwork"));
        }
        self.import_sync_as(&digest(bytes), bytes)
    }

    /// Keeps another device's copy of artwork `hash` (#273): the image its
    /// cache holds, which is re-encoded, so it can't be checked against
    /// `hash` (the original's). It's decoded within the same limits as an
    /// import, and arrives only from a paired device over its granted
    /// session.
    pub async fn import_received(&self, hash: String, bytes: Vec<u8>) -> Result<CachedArtwork> {
        if !super::valid_hash(&hash) {
            return Err(ArtworkError::Invalid("artwork hash"));
        }
        if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
            return Err(ArtworkError::Limit("empty or oversized encoded artwork"));
        }
        let cache = self.clone();
        caskade_jobs::blocking(crate::tr!("Preparing artwork"), move || {
            cache.import_sync_as(&hash, &bytes)
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }

    /// Artwork `hash` as the cache keeps it, if it does.
    pub fn cached(&self, hash: &str) -> Result<Option<CachedArtwork>> {
        self.lookup(hash)
    }

    /// Up to `len` bytes of artwork `hash`'s full-size image from `offset`,
    /// fewer only at its end: what another device receives (#273).
    pub fn read_original(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let Some(artwork) = self.lookup(hash)? else {
            return Err(ArtworkError::Invalid("artwork not cached"));
        };
        let mut file = self.dir.open(&artwork.cached_path)?.into_std();
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::with_capacity(len.min(MAX_INPUT_BYTES));
        file.take(len as u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn import_sync_as(&self, hash: &str, bytes: &[u8]) -> Result<CachedArtwork> {
        let hash = hash.to_owned();
        self.dir.create_dir_all(&hash)?;
        let directory = self.dir.open_dir(&hash)?;
        // OS locks serialize independent service instances and processes too.
        let _lock = lock(&directory)?;
        if let Some(cached) = self.lookup(&hash)? {
            return Ok(cached);
        }
        let image = decode(bytes)?;
        let mut thumbnails = Vec::with_capacity(THUMBNAIL_SIZES.len());
        write_image(&directory, "original.webp", &image)?;
        for size in THUMBNAIL_SIZES {
            // Never upscale a small icon to a cover-sized allocation.
            let thumbnail = if image.width().max(image.height()) <= size {
                image.clone()
            } else {
                image.thumbnail(size, size)
            };
            let file = format!("thumb-{size}.webp");
            write_image(&directory, &file, &thumbnail)?;
            thumbnails.push(CachedThumbnail {
                size,
                path: format!("{hash}/{file}"),
                width: thumbnail.width(),
                height: thumbnail.height(),
            });
        }
        let artwork = CachedArtwork {
            content_hash: hash.clone(),
            cached_path: format!("{hash}/original.webp"),
            width: image.width(),
            height: image.height(),
            thumbnails,
        };
        // Publish the manifest last. An interrupted generation is rebuilt,
        // while a completed generation leaves all thumbnail mtimes unchanged.
        let document = Document {
            schema_version: 1,
            artwork: artwork.clone(),
        };
        atomic_write(&directory, "entry.json", &serde_json::to_vec(&document)?)?;
        Ok(artwork)
    }

    pub(super) fn lookup(&self, hash: &str) -> Result<Option<CachedArtwork>> {
        if !super::valid_hash(hash) {
            return Ok(None);
        }
        let file = match self.dir.open(format!("{hash}/entry.json")) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let bytes = bounded(file, 16 * 1024)?;
        let Ok(document) = serde_json::from_slice::<Document>(&bytes) else {
            return Ok(None);
        };
        let art = document.artwork;
        if document.schema_version != 1
            || art.content_hash != hash
            || art.cached_path != format!("{hash}/original.webp")
            || art.width == 0
            || art.height == 0
            || art.width > 8192
            || art.height > 8192
            || u64::from(art.width) * u64::from(art.height) > MAX_PIXELS
            || art.thumbnails.len() != THUMBNAIL_SIZES.len()
        {
            return Ok(None);
        }
        for (thumbnail, size) in art.thumbnails.iter().zip(THUMBNAIL_SIZES) {
            if thumbnail.size != size
                || thumbnail.path != format!("{hash}/thumb-{size}.webp")
                || thumbnail.width == 0
                || thumbnail.height == 0
                || thumbnail.width > size
                || thumbnail.height > size
            {
                return Ok(None);
            }
        }
        for path in std::iter::once(&art.cached_path).chain(art.thumbnails.iter().map(|t| &t.path))
        {
            let file = match self.dir.open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ASSET_BYTES {
                return Ok(None);
            }
        }
        Ok(Some(art))
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ArtworkError::Limit("artwork byte limit"));
    }
    Ok(bytes)
}

pub(super) fn lock(directory: &Dir) -> Result<File> {
    let file = directory
        .open_with(
            ".lock",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )?
        .into_std();
    file.lock_exclusive()?;
    Ok(file)
}

pub(super) fn atomic_write(directory: &Dir, name: &str, bytes: &[u8]) -> Result<()> {
    // Only capability-relative names reach this function. create_new avoids
    // trusting any pre-existing temp file, including after a previous crash.
    let mut candidate = None;
    for _ in 0..100 {
        let name = format!(
            ".tmp-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        match directory.open_with(&name, OpenOptions::new().write(true).create_new(true)) {
            Ok(file) => {
                candidate = Some((name, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let (temporary, mut file) =
        candidate.ok_or(ArtworkError::Invalid("artwork temp namespace exhausted"))?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // A previous interrupted generation may have published this file.
        if directory.try_exists(name)? {
            directory.remove_file(name)?;
        }
        directory.rename(&temporary, directory, name)
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result.map_err(Into::into)
}

fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    let format = image::guess_format(bytes)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Ico
    ) {
        return Err(ArtworkError::Invalid(
            "supported artwork formats: PNG, JPEG, WebP, ICO",
        ));
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions()?;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(ArtworkError::Limit(
            "artwork dimensions exceed decode budget",
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(DynamicImage::ImageRgba8(reader.decode()?.to_rgba8()))
}

fn write_image(directory: &Dir, name: &str, image: &DynamicImage) -> Result<()> {
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, ImageFormat::WebP)?;
    if encoded.get_ref().len() as u64 > MAX_ASSET_BYTES {
        return Err(ArtworkError::Limit("encoded cache image exceeds budget"));
    }
    atomic_write(directory, name, encoded.get_ref())
}
