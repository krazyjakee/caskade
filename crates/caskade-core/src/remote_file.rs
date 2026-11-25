//! A file fetched again only when it changed (ETag), gzip-decoded and
//! bounded: Ludusavi's save manifest, the ports catalogue.

use crate::{
    Error, Result,
    error::net,
    http::{self, ReadGzipError},
};
use reqwest::header::{ACCEPT_ENCODING, ETAG, IF_NONE_MATCH};

const MAX_RESPONSE: usize = 64 << 20;

pub(crate) enum Response {
    Unchanged,
    Changed {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

/// Fetches `url`, `what` naming it in errors. With `etag`, an unchanged
/// file answers [`Response::Unchanged`].
pub(crate) async fn fetch(url: &str, etag: Option<&str>, what: &str) -> Result<Response> {
    let client = http::client().map_err(net)?;
    let mut request = client.get(url).header(ACCEPT_ENCODING, "gzip");
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let mut response = request.send().await.map_err(net)?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Response::Unchanged);
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(invalid(format!(
            "{what} server answered {}",
            response.status()
        )));
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|etag| etag.to_str().ok())
        .map(str::to_owned);
    let bytes = http::read_bounded_gzip(&mut response, MAX_RESPONSE)
        .await
        .map_err(|error| match error {
            ReadGzipError::Net(error) => net(error),
            ReadGzipError::TooLarge => invalid(format!("{what} exceeds 64 MiB")),
            ReadGzipError::Gzip(error) => Error::Io(error),
        })?;
    Ok(Response::Changed { bytes, etag })
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}
