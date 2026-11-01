use super::{
    ArtworkCache, ArtworkError, CachedArtwork, Result,
    cache::{MAX_INPUT_BYTES, atomic_write, bounded, digest, lock},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak},
    time::Duration,
};
use tokio::sync::OwnedMutexGuard;

const MAX_REDIRECTS: usize = 5;

/// Why a metadata image was refused (CSK-11).
const LOCAL: &str = "metadata images can't come from local or private addresses";

#[derive(Debug, thiserror::Error)]
#[error("metadata images can't come from local or private addresses")]
struct LocalAddress;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl ArtworkCache {
    /// Persist the URL-to-content mapping too, so repeated imports (including
    /// across app restarts) do not refetch immutable artwork. Explicit refresh
    /// can import a new source URL; originals remain content-addressed.
    ///
    /// Only the cache check and decoding take a slot in the job queue, so
    /// downloads of different URLs overlap. Callers must not hold a slot
    /// themselves: a download of the same URL may be waiting for one.
    ///
    /// For URLs the user gave. Use [`Self::download_metadata`] for URLs a
    /// metadata server sent.
    pub async fn download(&self, source: &str) -> Result<CachedArtwork> {
        self.fetch(source, false).await
    }

    /// [`Self::download`] for an image URL that metadata (IGDB, RomM) sent:
    /// it must lead to a public address, after every redirect and DNS
    /// answer, so a compromised server can't make Caskade send requests to
    /// loopback or LAN services (CSK-11).
    pub async fn download_metadata(&self, source: &str) -> Result<CachedArtwork> {
        self.fetch(source, !self.local_metadata).await
    }

    async fn fetch(&self, source: &str, public: bool) -> Result<CachedArtwork> {
        let url =
            url::Url::parse(source).map_err(|_| ArtworkError::Invalid("invalid artwork URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ArtworkError::Invalid(
                "artwork requires an HTTP(S) URL without credentials or fragment",
            ));
        }
        if public && !public_host(&url) {
            return Err(ArtworkError::Invalid(LOCAL));
        }
        let key = digest(url.as_str().as_bytes());
        // Taken before the file lock, which then only ever waits on another
        // process: within this one, a duplicate URL waits here, holding no
        // slot, while its twin downloads and decodes.
        let _fetching = fetching(&key).await;
        let cache = self.clone();
        let (directory, _lock, cached) = caskade_jobs::blocking(
            crate::tr!("Checking artwork cache"),
            move || -> Result<_> {
                cache.dir.create_dir_all(format!("sources/{key}"))?;
                let directory = cache.dir.open_dir(format!("sources/{key}"))?;
                let guard = lock(&directory)?;
                let cached = match directory.open("content.json") {
                    Ok(file) => {
                        let bytes = bounded(file, 1024)?;
                        match serde_json::from_slice::<String>(&bytes) {
                            Ok(hash) => cache.lookup(&hash)?,
                            Err(_) => None,
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };
                Ok((directory, guard, cached))
            },
        )
        .await
        .map_err(|_| ArtworkError::Worker)??;
        if let Some(cached) = cached {
            return Ok(cached);
        }
        let mut response = client(public)?
            .get(url)
            .send()
            .await
            .map_err(download_error)?
            .error_for_status()
            .map_err(ArtworkError::download)?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_INPUT_BYTES as u64)
        {
            return Err(ArtworkError::Limit("artwork response exceeds 16 MiB"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(ArtworkError::download)? {
            if chunk.len() > MAX_INPUT_BYTES - bytes.len() {
                return Err(ArtworkError::Limit("artwork response exceeds 16 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let artwork = self.import_bytes(bytes).await?;
        let hash = artwork.content_hash.clone();
        caskade_jobs::blocking(crate::tr!("Saving artwork"), move || {
            atomic_write(&directory, "content.json", &serde_json::to_vec(&hash)?)
        })
        .await
        .map_err(|_| ArtworkError::Worker)??;
        Ok(artwork)
    }
}

/// One client for every download (and one for metadata images), so images
/// from the same host reuse the connection.
fn client(public: bool) -> Result<reqwest::Client> {
    static ANY: OnceLock<reqwest::Client> = OnceLock::new();
    static PUBLIC: OnceLock<reqwest::Client> = OnceLock::new();
    let cell = if public { &PUBLIC } else { &ANY };
    if let Some(client) = cell.get() {
        return Ok(client.clone());
    }
    let builder = reqwest::Client::builder().timeout(Duration::from_secs(30));
    let builder = if public {
        builder
            .dns_resolver(Arc::new(PublicResolver))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if !public_host(attempt.url()) {
                    attempt.error(LocalAddress)
                } else {
                    attempt.follow()
                }
            }))
    } else {
        builder.redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
    };
    let client = builder.build().map_err(ArtworkError::download)?;
    Ok(cell.get_or_init(|| client).clone())
}

/// A refused destination is permanent, not the network being down.
fn download_error(error: reqwest::Error) -> ArtworkError {
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        if inner.is::<LocalAddress>() {
            return ArtworkError::Invalid(LOCAL);
        }
        source = inner.source();
    }
    ArtworkError::download(error)
}

/// Whether `url` may lead to a public address: a name (checked once it
/// resolves, by [`PublicResolver`]) or a public IP address.
fn public_host(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(_)) => true,
        Some(url::Host::Ipv4(ip)) => public_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => public_ip(IpAddr::V6(ip)),
        None => false,
    }
}

/// Not loopback, private, link-local, shared (CGNAT), unspecified,
/// broadcast, multicast or documentation space.
pub(super) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => public_ip(IpAddr::V4(v4)),
            None => {
                let first = ip.segments()[0];
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || (first & 0xfe00) == 0xfc00
                    || (first & 0xffc0) == 0xfe80)
            }
        },
    }
}

/// Resolves names with the system resolver and keeps only public addresses,
/// so a name that points at loopback or the LAN (DNS rebinding included)
/// is never connected to.
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(public_addresses(name.as_str().to_owned()))
    }
}

async fn public_addresses(host: String) -> std::result::Result<reqwest::dns::Addrs, BoxError> {
    let found = tokio::task::spawn_blocking(move || {
        std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0))
            .map(|addresses| addresses.collect::<Vec<SocketAddr>>())
    })
    .await??;
    let public: Vec<SocketAddr> = found
        .into_iter()
        .filter(|address| public_ip(address.ip()))
        .collect();
    if public.is_empty() {
        return Err(Box::new(LocalAddress));
    }
    Ok(Box::new(public.into_iter()))
}

/// The URL `key`'s lock in this process, held while it downloads.
async fn fetching(key: &str) -> OwnedMutexGuard<()> {
    type Locks = Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>;
    static LOCKS: LazyLock<Locks> = LazyLock::new(Locks::default);
    let lock = {
        let mut locks = crate::lock(&LOCKS);
        locks.retain(|_, lock| lock.strong_count() > 0);
        match locks.get(key).and_then(Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(key.to_owned(), Arc::downgrade(&lock));
                lock
            }
        }
    };
    lock.lock_owned().await
}
