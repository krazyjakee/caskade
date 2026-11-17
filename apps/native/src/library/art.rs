//! Game artwork for the grid and list: the cached thumbnails (320 px for a
//! card, 96 px for a list row), never the full-size original, served from
//! the artwork cache through `caskade-asset://` as the webview's are.

use blitz_traits::net::{NetHandler, Request};
use caskade_core::artwork::ArtworkCache;
use caskade_core::model::Artwork;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The scheme, as the Tauri app registers it (`asset.rs`).
pub const SCHEME: &str = "caskade-asset";

/// Which thumbnail a place shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// A card's cover: 320 px.
    Card,
    /// A list row's icon: 96 px.
    Icon,
    /// A game page's hero: 960 px.
    Hero,
}

impl Variant {
    fn size(self) -> u32 {
        match self {
            Self::Card => 320,
            Self::Icon => 96,
            Self::Hero => 960,
        }
    }
}

/// The URL of `art`'s thumbnail for `variant`; `None` without one (the
/// original is never shown in a grid or list).
pub fn url(art: Option<&Artwork>, variant: Variant) -> Option<String> {
    let thumb = art?
        .thumbnails
        .iter()
        .find(|thumb| thumb.size == variant.size())?;
    Some(format!(
        "{SCHEME}://localhost/{}",
        thumb.path.trim_start_matches('/')
    ))
}

/// Serves a `caskade-asset://` request from the artwork cache; whether the
/// request was one. The theme's network provider (`theme::Net`) asks this
/// first.
pub fn fetch(request: &Request, handler: Box<dyn NetHandler>) -> Result<(), Box<dyn NetHandler>> {
    if request.url.scheme() != SCHEME {
        return Err(handler);
    }
    let url = request.url.to_string();
    // The platforms' renders, built in.
    if request.url.host_str() == Some(super::platforms::ART_HOST) {
        match super::platforms::art(request.url.path()) {
            Some(bytes) => handler.bytes(url, bytes.to_vec().into()),
            None => handler.error(url, "no such platform render".into()),
        }
        return Ok(());
    }
    caskade_app::runtime::handle().spawn(async move {
        let Some(cache) = cache() else {
            handler.error(url, "no artwork cache".into());
            return;
        };
        let response = caskade_core::artwork::serve(cache, &url).await;
        if response.status == 200 {
            handler.bytes(url, response.bytes.into());
        } else {
            handler.error(url, format!("artwork: {}", response.status));
        }
    });
    Ok(())
}

/// The image hosts the webview's CSP lets it show (`tauri.conf.json`'s
/// `img-src`): IGDB's covers and screenshots, and YouTube's thumbnails.
pub const REMOTE_IMAGE_HOSTS: [&str; 2] = ["images.igdb.com", "i.ytimg.com"];

/// Serves an image from [`REMOTE_IMAGE_HOSTS`] through the artwork cache
/// (downloaded once, kept for offline use, as achievement pictures are);
/// whether the request was one. Other remote URLs aren't fetched.
pub fn fetch_remote(
    request: &Request,
    handler: Box<dyn NetHandler>,
) -> Result<(), Box<dyn NetHandler>> {
    let remote = request.url.scheme() == "https"
        && request
            .url
            .host_str()
            .is_some_and(|host| REMOTE_IMAGE_HOSTS.contains(&host));
    if !remote {
        return Err(handler);
    }
    let url = request.url.to_string();
    caskade_app::runtime::handle().spawn(async move {
        let Some(cache) = cache() else {
            remote_failed(&url, "no artwork cache");
            handler.error(url, "no artwork cache".into());
            return;
        };
        let cached = match cache.download_metadata(&url).await {
            Ok(cached) => cached,
            Err(error) => {
                let reason = format!("remote image: {error}");
                remote_failed(&url, &reason);
                handler.error(url, reason);
                return;
            }
        };
        let local = format!(
            "{SCHEME}://localhost/{}",
            cached.cached_path.trim_start_matches('/')
        );
        let response = caskade_core::artwork::serve(cache, &local).await;
        if response.status == 200 {
            handler.bytes(url, response.bytes.into());
        } else {
            let reason = format!("remote image: {}", response.status);
            remote_failed(&url, &reason);
            handler.error(url, reason);
        }
    });
    Ok(())
}

/// How often a failing image is logged: a Store page of covers failing
/// alike logs one line, with how many more failed since, not one each.
const LOG_EVERY: Duration = Duration::from_secs(30);

/// Lets one line through every [`LOG_EVERY`], counting the ones it holds
/// back.
#[derive(Debug, Default)]
struct Throttle {
    last: Option<Instant>,
    held: u32,
}

impl Throttle {
    /// Whether to log now, and if so how many were held back before it.
    fn allow(&mut self, now: Instant) -> Option<u32> {
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < LOG_EVERY)
        {
            self.held = self.held.saturating_add(1);
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.held))
    }
}

/// Logs why a remote image didn't load (#163), at debug level and
/// throttled: the image handler alone says nothing anyone can read.
fn remote_failed(url: &str, reason: &str) {
    static THROTTLE: Mutex<Throttle> = Mutex::new(Throttle {
        last: None,
        held: 0,
    });
    if let Some(held) = caskade_core::lock(&THROTTLE).allow(Instant::now()) {
        tracing::debug!(url, reason, held, "remote image failed");
    }
}

/// The artwork cache images are served from.
static CACHE: OnceLock<ArtworkCache> = OnceLock::new();

/// Serves images from `cache` where no services run: the website's
/// captures (`crate::shots`, #150). The first cache set stays.
#[cfg(feature = "shots")]
pub fn use_cache(cache: ArtworkCache) {
    let _ = CACHE.set(cache);
}

/// The artwork cache, as the shell opened it (`paths.cache/artwork`,
/// created when missing), once the services are up. A cache that can't
/// open is logged with its path and the OS error, throttled, and tried
/// again on the next image.
fn cache() -> Option<&'static ArtworkCache> {
    static THROTTLE: Mutex<Throttle> = Mutex::new(Throttle {
        last: None,
        held: 0,
    });
    if let Some(cache) = CACHE.get() {
        return Some(cache);
    }
    let services = crate::shell::services()?;
    let root = services.paths.cache.join("artwork");
    match ArtworkCache::new(&root) {
        Ok(cache) => Some(CACHE.get_or_init(|| cache)),
        Err(error) => {
            if let Some(held) = caskade_core::lock(&THROTTLE).allow(Instant::now()) {
                tracing::warn!(path = %root.display(), %error, held, "artwork cache unavailable: images can't load");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caskade_core::model::{ArtworkKind, ArtworkThumbnail};

    #[test]
    fn failing_images_log_once_per_interval_with_the_count_held_back() {
        let mut throttle = Throttle::default();
        let start = Instant::now();
        assert_eq!(throttle.allow(start), Some(0));
        assert_eq!(throttle.allow(start + Duration::from_secs(1)), None);
        assert_eq!(throttle.allow(start + Duration::from_secs(2)), None);
        assert_eq!(throttle.allow(start + LOG_EVERY), Some(2));
        assert_eq!(throttle.allow(start + LOG_EVERY), None);
    }

    #[test]
    fn cards_and_rows_show_thumbnails_never_the_original() {
        let mut cover = Artwork::new(
            "g1",
            ArtworkKind::Cover,
            "ab12",
            "igdb",
            "ab12/original.webp",
            1200,
            1800,
        );
        let thumb = |size: u32| ArtworkThumbnail {
            size,
            path: format!("ab12/thumb-{size}.webp"),
            width: size,
            height: size * 3 / 2,
        };
        cover.thumbnails = vec![thumb(96), thumb(320), thumb(960)];
        assert_eq!(
            url(Some(&cover), Variant::Card).as_deref(),
            Some("caskade-asset://localhost/ab12/thumb-320.webp")
        );
        assert_eq!(
            url(Some(&cover), Variant::Icon).as_deref(),
            Some("caskade-asset://localhost/ab12/thumb-96.webp")
        );
        // Without the thumbnail, no picture rather than the original.
        cover.thumbnails = vec![thumb(960)];
        assert_eq!(url(Some(&cover), Variant::Card), None);
        assert_eq!(url(None, Variant::Card), None);
    }
}
