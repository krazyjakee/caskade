//! The catalogue client: the last catalogue that verified and validated,
//! kept on disk for offline browsing, and a refresh that replaces it only
//! with one that does too. A refresh is one GET of the catalogue and one of
//! its signature; it sends nothing about the user or their library.

use super::{Catalogue, parse};
#[cfg(feature = "net")]
use super::{MAX_CATALOGUE_BYTES, MAX_SIGNATURE_BYTES};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use url::Url;

const CATALOGUE_FILE: &str = "catalogue.json";
const SIGNATURE_FILE: &str = "catalogue.json.sig";
const META_FILE: &str = "catalogue.meta.json";

/// Where the catalogue is and how it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueStatus {
    pub origin: String,
    /// When the catalogue shown was fetched; none before the first fetch.
    pub fetched_at_ms: Option<i64>,
    pub refreshing: bool,
    /// Why the last refresh failed; the catalogue shown is the last good one.
    pub error: Option<String>,
    /// The last refresh couldn't reach the origin.
    pub offline: bool,
    /// Its signature was checked against Caskade's key.
    pub verified: bool,
    /// Records left out of the catalogue shown (details in the log).
    pub left_out: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    origin: String,
    fetched_at_ms: i64,
}

struct State {
    catalogue: Catalogue,
    status: CatalogueStatus,
}

struct Inner {
    dir: PathBuf,
    origin: Url,
    key: Option<minisign_verify::PublicKey>,
    state: RwLock<State>,
    /// One refresh at a time.
    #[cfg_attr(not(feature = "net"), allow(dead_code))]
    refresh: tokio::sync::Mutex<()>,
}

/// The add-on catalogue. Cheap to clone.
#[derive(Clone)]
pub struct CatalogueService {
    inner: Arc<Inner>,
}

/// minisign text, as is or base64-wrapped as Tauri's keys and signatures
/// are.
fn minisign_text(text: &str) -> String {
    use base64_decode::decode;
    let text = text.trim();
    if text.starts_with("untrusted comment:") {
        return text.to_owned();
    }
    decode(text).unwrap_or_else(|| text.to_owned())
}

/// A tiny standard-base64 decoder: the keys and signatures are short.
mod base64_decode {
    pub fn decode(text: &str) -> Option<String> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let (mut buffer, mut bits) = (0u32, 0);
        for byte in text
            .bytes()
            .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        {
            let value = ALPHABET.iter().position(|c| *c == byte)? as u32;
            buffer = (buffer << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        String::from_utf8(out).ok()
    }
}

fn verify(
    key: Option<&minisign_verify::PublicKey>,
    catalogue: &[u8],
    signature: Option<&[u8]>,
) -> Result<bool> {
    let Some(key) = key else {
        return Ok(false);
    };
    let refused = || {
        Error::Invalid(crate::tr!(
            "the catalogue's signature doesn't match Caskade's key"
        ))
    };
    let signature = signature
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(refused)?;
    let signature =
        minisign_verify::Signature::decode(&minisign_text(signature)).map_err(|_| refused())?;
    key.verify(catalogue, &signature, false)
        .map_err(|_| refused())?;
    Ok(true)
}

/// Writes `bytes` to `path` whole or not at all.
async fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("partial");
    tokio::fs::write(&temp, bytes).await?;
    tokio::fs::rename(&temp, path).await?;
    Ok(())
}

impl CatalogueService {
    /// The catalogue at `origin`, cached in `dir`; checked against
    /// `public_key` (minisign, as is or base64) when there is one. Reads the
    /// cached catalogue, if one is there and still verifies.
    pub fn new(dir: impl Into<PathBuf>, origin: &str, public_key: Option<&str>) -> Result<Self> {
        let dir = dir.into();
        let origin = Url::parse(origin)
            .map_err(|_| Error::Invalid(crate::tr!("the catalogue address isn't valid")))?;
        let key = public_key
            .map(|text| {
                minisign_verify::PublicKey::decode(&minisign_text(text))
                    .map_err(|_| Error::Invalid(crate::tr!("the catalogue key can't be read")))
            })
            .transpose()?;
        let status = CatalogueStatus {
            origin: origin.to_string(),
            fetched_at_ms: None,
            refreshing: false,
            error: None,
            offline: false,
            verified: false,
            left_out: 0,
        };
        let service = Self {
            inner: Arc::new(Inner {
                dir,
                origin,
                key,
                state: RwLock::new(State {
                    catalogue: Catalogue::default(),
                    status,
                }),
                refresh: tokio::sync::Mutex::new(()),
            }),
        };
        service.load_cached();
        Ok(service)
    }

    /// The cached catalogue, when it is this origin's and verifies.
    fn load_cached(&self) {
        let inner = &self.inner;
        let read = |name: &str| std::fs::read(inner.dir.join(name)).ok();
        let (Some(bytes), Some(meta)) = (read(CATALOGUE_FILE), read(META_FILE)) else {
            return;
        };
        let Ok(meta) = serde_json::from_slice::<Meta>(&meta) else {
            return;
        };
        if meta.origin != inner.origin.as_str() {
            return;
        }
        let signature = read(SIGNATURE_FILE);
        let outcome = verify(inner.key.as_ref(), &bytes, signature.as_deref())
            .and_then(|verified| Ok((verified, parse(&bytes, &inner.origin)?)));
        match outcome {
            Ok((verified, catalogue)) => {
                let mut state = crate::write(&inner.state);
                state.status.fetched_at_ms = Some(meta.fetched_at_ms);
                state.status.verified = verified;
                state.status.left_out = catalogue.problems.len() as u32;
                state.catalogue = catalogue;
            }
            Err(error) => tracing::warn!(%error, "the cached add-on catalogue was set aside"),
        }
    }

    pub fn catalogue(&self) -> Catalogue {
        crate::read(&self.inner.state).catalogue.clone()
    }

    pub fn status(&self) -> CatalogueStatus {
        crate::read(&self.inner.state).status.clone()
    }

    pub fn origin(&self) -> &Url {
        &self.inner.origin
    }

    /// Takes `bytes` (and their signature) as the new catalogue when they
    /// verify and parse; the last good one stays otherwise.
    pub async fn accept(
        &self,
        bytes: &[u8],
        signature: Option<&[u8]>,
        fetched_at_ms: i64,
    ) -> Result<CatalogueStatus> {
        let inner = &self.inner;
        let verified = verify(inner.key.as_ref(), bytes, signature)?;
        let catalogue = parse(bytes, &inner.origin)?;
        for problem in &catalogue.problems {
            tracing::warn!(id = ?problem.id, problem = %problem.message, "catalogue record left out");
        }
        tokio::fs::create_dir_all(&inner.dir).await?;
        match signature {
            Some(signature) => write_atomically(&inner.dir.join(SIGNATURE_FILE), signature).await?,
            None => {
                let _ = tokio::fs::remove_file(inner.dir.join(SIGNATURE_FILE)).await;
            }
        }
        write_atomically(&inner.dir.join(CATALOGUE_FILE), bytes).await?;
        let meta = Meta {
            origin: inner.origin.to_string(),
            fetched_at_ms,
        };
        write_atomically(&inner.dir.join(META_FILE), &serde_json::to_vec(&meta)?).await?;
        let mut state = crate::write(&inner.state);
        state.status.fetched_at_ms = Some(fetched_at_ms);
        state.status.verified = verified;
        state.status.error = None;
        state.status.offline = false;
        state.status.left_out = catalogue.problems.len() as u32;
        state.catalogue = catalogue;
        Ok(state.status.clone())
    }

    #[cfg(feature = "net")]
    fn failed(&self, error: &Error, offline: bool) -> CatalogueStatus {
        let mut state = crate::write(&self.inner.state);
        state.status.error = Some(error.to_string());
        state.status.offline = offline;
        state.status.refreshing = false;
        state.status.clone()
    }

    /// Fetches the catalogue again. On any failure the last good catalogue
    /// stays and the status says why.
    #[cfg(feature = "net")]
    pub async fn refresh(&self) -> CatalogueStatus {
        let _one = self.inner.refresh.lock().await;
        crate::write(&self.inner.state).status.refreshing = true;
        let outcome = self.fetch().await;
        crate::write(&self.inner.state).status.refreshing = false;
        match outcome {
            Ok((bytes, signature)) => match self
                .accept(&bytes, signature.as_deref(), crate::now_ms())
                .await
            {
                Ok(status) => status,
                Err(error) => {
                    tracing::warn!(%error, "the add-on catalogue was refused");
                    self.failed(&error, false)
                }
            },
            Err((error, offline)) => {
                tracing::warn!(%error, "the add-on catalogue wasn't fetched");
                self.failed(&error, offline)
            }
        }
    }

    #[cfg(feature = "net")]
    async fn fetch(&self) -> std::result::Result<(Vec<u8>, Option<Vec<u8>>), (Error, bool)> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|error| (Error::Invalid(error.to_string()), false))?;
        let origin = &self.inner.origin;
        let bytes = get(&client, origin.clone(), MAX_CATALOGUE_BYTES).await?;
        let signature = if self.inner.key.is_some() {
            let mut url = origin.clone();
            url.set_path(&format!("{}.sig", origin.path()));
            Some(get(&client, url, MAX_SIGNATURE_BYTES).await?)
        } else {
            None
        };
        Ok((bytes, signature))
    }
}

/// `url`'s body, up to `max` bytes; whether a failure is the network's.
#[cfg(feature = "net")]
async fn get(
    client: &reqwest::Client,
    url: Url,
    max: usize,
) -> std::result::Result<Vec<u8>, (Error, bool)> {
    let fail = |error: reqwest::Error| {
        let offline = error.is_connect() || error.is_timeout();
        (
            Error::Invalid(crate::tr!(
                "the catalogue couldn't be fetched: {error}",
                error = error.without_url()
            )),
            offline,
        )
    };
    let mut response = client.get(url).send().await.map_err(fail)?;
    if !response.status().is_success() {
        return Err((
            Error::Invalid(crate::tr!(
                "the catalogue server answered {status}",
                status = response.status().as_u16()
            )),
            false,
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(fail)? {
        if body.len() + chunk.len() > max {
            return Err((
                Error::Invalid(crate::tr!(
                    "the catalogue is larger than {mib} MiB",
                    mib = MAX_CATALOGUE_BYTES >> 20
                )),
                false,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
