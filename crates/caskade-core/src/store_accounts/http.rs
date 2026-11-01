//! HTTP for store providers: one client, bounded answers, and failures as
//! [`ProviderError`]s. URLs can carry keys and tokens: they are never
//! logged or put in an error.

use super::ProviderError;
use reqwest::{RequestBuilder, Response, StatusCode, header::RETRY_AFTER};
use serde::de::DeserializeOwned;
use std::{sync::OnceLock, time::Duration};

/// At most this much of an answer is read.
const MAX_BODY: usize = 64 * 1024 * 1024;

/// One client for every store request.
pub(crate) fn client() -> std::result::Result<reqwest::Client, ProviderError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| ProviderError::Other(crate::error::describe_net(error)))?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

/// Sends `request` and reads its JSON answer. `store` names the store in
/// errors.
pub(crate) async fn json<T: DeserializeOwned>(
    request: RequestBuilder,
    store: &str,
) -> std::result::Result<T, ProviderError> {
    let bytes = body(request, store).await?;
    serde_json::from_slice(&bytes).map_err(|_| unreadable(store))
}

/// [`json`], or `None` when the store has no such thing (404).
pub(crate) async fn json_if_found<T: DeserializeOwned>(
    request: RequestBuilder,
    store: &str,
) -> std::result::Result<Option<T>, ProviderError> {
    let Some(bytes) = read(request, store, true).await? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| unreadable(store))
}

/// Sends `request` and reads its body: 429 waits as the store asks, 401 and
/// 403 mean the sign-in is no good, other failures say the status.
pub(crate) async fn body(
    request: RequestBuilder,
    store: &str,
) -> std::result::Result<Vec<u8>, ProviderError> {
    Ok(read(request, store, false).await?.unwrap_or_default())
}

/// [`body`], or `None` for a 404 when `missing_ok`.
async fn read(
    request: RequestBuilder,
    store: &str,
    missing_ok: bool,
) -> std::result::Result<Option<Vec<u8>>, ProviderError> {
    let response = send(request).await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::Expired(crate::tr!(
            "{store} refused the sign-in (HTTP {status})",
            store = store,
            status = status.as_u16()
        )));
    }
    if missing_ok && status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(if status.is_server_error() {
            ProviderError::Offline(answered(store, status))
        } else {
            unexpected(store, status)
        });
    }
    read_body(response, store, MAX_BODY).await.map(Some)
}

/// Sends `request`: a failed connection or a timeout is `Offline`, a 429
/// `RateLimited` for as long as the store asks (up to six hours).
pub(crate) async fn send(request: RequestBuilder) -> std::result::Result<Response, ProviderError> {
    let response = request.send().await.map_err(|error| {
        let error = error.without_url();
        if error.is_connect() || error.is_timeout() {
            ProviderError::Offline(crate::error::describe_net(error))
        } else {
            ProviderError::Other(crate::error::describe_net(error))
        }
    })?;
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()?.trim().parse::<u64>().ok())
            .map(|seconds| Duration::from_secs(seconds.min(6 * 60 * 60)));
        return Err(ProviderError::RateLimited { retry_after });
    }
    Ok(response)
}

/// `response`'s body, refused past `max` bytes.
pub(crate) async fn read_body(
    mut response: Response,
    store: &str,
    max: usize,
) -> std::result::Result<Vec<u8>, ProviderError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ProviderError::Offline(crate::error::describe_net(error.without_url())))?
    {
        if bytes.len() + chunk.len() > max {
            return Err(ProviderError::Other(crate::tr!(
                "{store} sent too large an answer",
                store = store
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// "<store> answered HTTP <status>".
pub(crate) fn answered(store: &str, status: StatusCode) -> String {
    crate::tr!(
        "{store} answered HTTP {status}",
        store = store,
        status = status.as_u16()
    )
}

/// "<store> answered HTTP <status>", for an answer a provider can't use.
pub(crate) fn unexpected(store: &str, status: StatusCode) -> ProviderError {
    ProviderError::Other(answered(store, status))
}

/// "<store> sent an answer Caskade can't read".
pub(crate) fn unreadable(store: &str) -> ProviderError {
    ProviderError::Other(crate::tr!(
        "{store} sent an answer Caskade can't read",
        store = store
    ))
}
