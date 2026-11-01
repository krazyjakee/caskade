use super::{
    ArtworkCache, ArtworkError,
    cache::{MAX_ASSET_BYTES, bounded},
};

/// Adapter-independent HTTP result; Tauri never receives an ambient file path.
pub struct AssetResponse {
    pub status: u16,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub cache_control: &'static str,
}
impl AssetResponse {
    fn error(status: u16) -> Self {
        Self {
            status,
            mime: "text/plain; charset=utf-8",
            bytes: Vec::new(),
            cache_control: "no-store",
        }
    }
}

/// Resolve a raw scheme URI, Windows WebView URI, or raw URI path. Reject raw
/// traversal/encoding before a URL library could normalize dot segments away.
pub async fn serve(cache: &ArtworkCache, uri: &str) -> AssetResponse {
    if uri.len() > 1024 {
        return AssetResponse::error(400);
    }
    let path = if uri.starts_with('/') {
        uri
    } else if let Some(path) = uri.strip_prefix("caskade-asset://localhost") {
        path
    } else if let Some(path) = uri.strip_prefix("http://caskade-asset.localhost") {
        path
    } else {
        return AssetResponse::error(400);
    };
    // Tauri convertFileSrc percent-encodes the entire cache key, including
    // its separator. Decode exactly once, then validate the resulting grammar.
    let Some(path) = decode_path(path) else {
        return AssetResponse::error(400);
    };
    if !path.starts_with('/')
        || path.contains(['%', '\\', '?', '#'])
        || path.chars().any(char::is_control)
    {
        return AssetResponse::error(400);
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 3 || !parts[0].is_empty() || !super::valid_hash(parts[1]) {
        return AssetResponse::error(400);
    }
    if !matches!(
        parts[2],
        "original.webp" | "thumb-96.webp" | "thumb-320.webp" | "thumb-960.webp"
    ) {
        return AssetResponse::error(404);
    }
    let relative = format!("{}/{}", parts[1], parts[2]);
    let cache = cache.clone();
    match tokio::task::spawn_blocking(move || -> super::Result<Vec<u8>> {
        let file = cache.dir.open(relative)?;
        if !file.metadata()?.is_file() {
            return Err(ArtworkError::Invalid("asset is not a regular file"));
        }
        bounded(file, MAX_ASSET_BYTES)
    })
    .await
    {
        Ok(Ok(bytes)) => AssetResponse {
            status: 200,
            mime: "image/webp",
            bytes,
            cache_control: "public, max-age=31536000, immutable",
        },
        Ok(Err(ArtworkError::Io(e)))
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            AssetResponse::error(404)
        }
        Ok(Err(ArtworkError::Invalid(_) | ArtworkError::Limit(_))) => AssetResponse::error(404),
        _ => AssetResponse::error(500),
    }
}

fn decode_path(path: &str) -> Option<String> {
    let mut bytes = path.bytes();
    let mut decoded = Vec::with_capacity(path.len());
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()?
        } else {
            byte
        });
    }
    String::from_utf8(decoded).ok()
}
