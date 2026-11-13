//! `POST /games`, `POST /match`, `POST /search` and the catalog's
//! `/catalog/*` against the metadata server.

use super::{IgdbCatalogFilters, MatchResponse, RawPage, Response, SearchRequest, StoreId};
use crate::{Error, error::net};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE, RETRY_AFTER};
use std::{io::Read, sync::OnceLock, time::Duration};

/// 500 games with every artwork and screenshot come to a few MiB.
const MAX_RESPONSE: usize = 64 << 20;

/// A failed request, and how long the server asked to be left alone.
pub(super) struct Failure {
    pub(super) error: Error,
    pub(super) retry_after: Option<Duration>,
    /// The HTTP status the server answered with, when it answered.
    pub(super) status: Option<u16>,
}

impl<E: Into<Error>> From<E> for Failure {
    fn from(error: E) -> Self {
        Self {
            error: error.into(),
            retry_after: None,
            status: None,
        }
    }
}

/// One client for every request, so batches reuse the connection.
fn client() -> Result<reqwest::Client, Failure> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // The server queries IGDB for every id it has no copy of.
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(net)?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

fn too_large() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        crate::tr!("metadata server response exceeds 64 MiB"),
    ))
}

async fn post(url: String, body: Vec<u8>) -> Result<Vec<u8>, Failure> {
    read(
        client()?
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body),
    )
    .await
}

async fn read(request: reqwest::RequestBuilder) -> Result<Vec<u8>, Failure> {
    let mut response = request
        // Decoded here rather than by reqwest's `gzip` feature, which
        // would turn on decoding for the installer's downloads too.
        .header(ACCEPT_ENCODING, "gzip")
        .send()
        .await
        .map_err(net)?;
    let status = response.status();
    if !status.is_success() {
        // Seconds only; an HTTP date gets the default pause.
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs);
        return Err(Failure {
            error: Error::Io(std::io::Error::other(crate::tr!(
                "metadata server answered {status}",
                status = status
            ))),
            retry_after,
            status: Some(status.as_u16()),
        });
    }
    let gzip = response
        .headers()
        .get(CONTENT_ENCODING)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"gzip"));
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        if chunk.len() > MAX_RESPONSE - bytes.len() {
            return Err(too_large().into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !gzip {
        return Ok(bytes);
    }
    let decoded = tokio::task::spawn_blocking(move || {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut decoded)
            .map(|_| decoded)
    })
    .await??;
    if decoded.len() > MAX_RESPONSE {
        return Err(too_large().into());
    }
    Ok(decoded)
}

pub(super) async fn games(base: &str, ids: &[u64]) -> Result<Response, Failure> {
    let bytes = post(format!("{base}/games"), serde_json::to_vec(ids)?).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) async fn matches(base: &str, ids: &[StoreId]) -> Result<MatchResponse, Failure> {
    let bytes = post(format!("{base}/match"), serde_json::to_vec(ids)?).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) async fn search(base: &str, request: &SearchRequest<'_>) -> Result<Response, Failure> {
    let bytes = post(format!("{base}/search"), serde_json::to_vec(request)?).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// `POST {path}` for a page of the catalog.
pub(super) async fn catalog(
    base: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<RawPage, Failure> {
    let bytes = post(format!("{base}{path}"), serde_json::to_vec(body)?).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// `GET /catalog/filters`.
pub(super) async fn filters(base: &str) -> Result<IgdbCatalogFilters, Failure> {
    let bytes = read(client()?.get(format!("{base}/catalog/filters"))).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// `POST /popularity`, with a complete, validated answer for every asked id.
pub(super) async fn popularity(
    base: &str,
    ids: &[u64],
) -> Result<super::popularity::PopularityResponse, Failure> {
    let bytes = post(format!("{base}/popularity"), serde_json::to_vec(ids)?).await?;
    let response: super::popularity::PopularityResponse = serde_json::from_slice(&bytes)?;
    if !response.valid(ids) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            crate::tr!("invalid popularity response"),
        )
        .into());
    }
    Ok(response)
}
