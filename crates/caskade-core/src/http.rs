//! Shared HTTP client, user agent and bounded response readers.

use std::{io::Read, sync::OnceLock, time::Duration};

/// The `User-Agent` sent by Caskade's HTTP requests.
pub const USER_AGENT: &str = concat!("Caskade/", env!("CARGO_PKG_VERSION"));

/// One shared client for standard API and file requests: 30 s connect timeout,
/// 120 s total timeout, and [`USER_AGENT`].
pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .user_agent(USER_AGENT)
        .build()?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

/// Why [`read_bounded`] stopped before returning the body.
#[derive(Debug)]
pub enum ReadBodyError {
    Net(reqwest::Error),
    TooLarge,
}

/// Why [`read_bounded_gzip`] stopped before returning the body.
#[derive(Debug)]
pub enum ReadGzipError {
    Net(reqwest::Error),
    TooLarge,
    Gzip(std::io::Error),
}

/// Reads `response`'s body up to `max` bytes.
pub async fn read_bounded(
    response: &mut reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, ReadBodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > max as u64)
    {
        return Err(ReadBodyError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(ReadBodyError::Net)? {
        if chunk.len() > max - bytes.len() {
            return Err(ReadBodyError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Reads `response`'s body up to `max` bytes, gzip-decoding it off-thread when
/// `Content-Encoding` is `gzip` (also bounded by `max`).
pub async fn read_bounded_gzip(
    response: &mut reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, ReadGzipError> {
    let gzip = response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|encoding| encoding.as_bytes().eq_ignore_ascii_case(b"gzip"));
    let bytes = read_bounded(response, max)
        .await
        .map_err(|error| match error {
            ReadBodyError::Net(error) => ReadGzipError::Net(error),
            ReadBodyError::TooLarge => ReadGzipError::TooLarge,
        })?;
    if !gzip {
        return Ok(bytes);
    }
    let decoded = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take((max as u64).saturating_add(1))
            .read_to_end(&mut decoded)?;
        Ok(decoded)
    })
    .await
    .map_err(|error| ReadGzipError::Gzip(std::io::Error::other(error)))?
    .map_err(ReadGzipError::Gzip)?;
    if decoded.len() > max {
        return Err(ReadGzipError::TooLarge);
    }
    Ok(decoded)
}
