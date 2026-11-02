use crate::{Result, error::net};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, ETAG, IF_NONE_MATCH};
use std::{io::Read, sync::OnceLock, time::Duration};

const MAX_RESPONSE: usize = 64 << 20;

pub(super) enum Response {
    Unchanged,
    Changed {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

pub(super) async fn fetch(url: &str, etag: Option<&str>) -> Result<Response> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = match CLIENT.get() {
        Some(client) => client,
        None => {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(120))
                .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(net)?;
            CLIENT.get_or_init(|| client)
        }
    };
    let mut request = client.get(url).header(ACCEPT_ENCODING, "gzip");
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let mut response = request.send().await.map_err(net)?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Response::Unchanged);
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(super::invalid(format!(
            "Ludusavi manifest server answered {}",
            response.status()
        )));
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|etag| etag.to_str().ok())
        .map(str::to_owned);
    let gzip = response
        .headers()
        .get(CONTENT_ENCODING)
        .is_some_and(|encoding| encoding.as_bytes().eq_ignore_ascii_case(b"gzip"));
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        if chunk.len() > MAX_RESPONSE - bytes.len() {
            return Err(super::invalid("Ludusavi manifest exceeds 64 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if gzip {
        bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(bytes.as_slice())
                .take(MAX_RESPONSE as u64 + 1)
                .read_to_end(&mut decoded)?;
            if decoded.len() > MAX_RESPONSE {
                return Err(super::invalid("Ludusavi manifest exceeds 64 MiB"));
            }
            Ok(decoded)
        })
        .await
        .map_err(|error| super::invalid(error.to_string()))??;
    }
    Ok(Response::Changed { bytes, etag })
}
