//! Downloads release files into the cache, with progress.

use crate::{Error, Result};
use std::{path::Path, sync::OnceLock, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// Releases are tens to hundreds of MiB: no total timeout, only a stall one.
pub(crate) fn client() -> Result<reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        // What this fetches is run unchecked: a redirect may not drop from
        // HTTPS to plain HTTP (#157).
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 10 {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() != "https"
                && attempt.previous().iter().any(|url| url.scheme() == "https")
            {
                attempt.error("a redirect left HTTPS")
            } else {
                attempt.follow()
            }
        }))
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(net)?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

// Without the URL: download hosts sign theirs.
pub(crate) use crate::error::net;

/// Streams `url` into `to` (through a `.part` file), reporting the bytes
/// so far and the total when known. A device nearby that has the file
/// (`near`, #277) is asked first; what comes from the internet is kept
/// for the other devices.
pub(crate) async fn fetch(
    url: &url::Url,
    to: &Path,
    cancel: &CancellationToken,
    near: Option<&crate::install::nearby::PeerDownloads>,
    progress: impl FnMut(u64, Option<u64>) + Send,
) -> Result<()> {
    let progress = std::sync::Mutex::new(progress);
    let report = |done: u64, total: Option<u64>| {
        (progress.lock().unwrap_or_else(|e| e.into_inner()))(done, total)
    };
    if let Some(near) = near {
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if near
            .fetch_to(url, to, cancel, &|done, total| report(done, Some(total)))
            .await
        {
            return Ok(());
        }
    }
    let validator = download(url, to, cancel, report).await?;
    if let Some(near) = near {
        near.keep_path(url, to, validator).await;
    }
    Ok(())
}

/// The download itself; the source's validator for what it sent.
async fn download(
    url: &url::Url,
    to: &Path,
    cancel: &CancellationToken,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Option<String>> {
    let cancelled = || Error::Invalid(crate::tr!("cancelled"));
    let mut response = tokio::select! {
        () = cancel.cancelled() => return Err(cancelled()),
        response = client()?.get(url.clone()).send() => response.map_err(net)?,
    }
    .error_for_status()
    .map_err(net)?;
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok())
            .map(str::to_owned)
    };
    let validator = match header(reqwest::header::ETAG) {
        Some(etag) => (!etag.starts_with("W/")).then_some(etag),
        None => header(reqwest::header::LAST_MODIFIED),
    };
    let total = response.content_length();
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut part = to.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let written = async {
        let mut file = tokio::fs::File::create(&part).await?;
        let mut size = 0u64;
        progress(0, total);
        loop {
            let chunk = tokio::select! {
                () = cancel.cancelled() => return Err(cancelled()),
                chunk = response.chunk() => chunk.map_err(net)?,
            };
            let Some(chunk) = chunk else { break };
            size += chunk.len() as u64;
            file.write_all(&chunk).await?;
            progress(size, total);
        }
        if total.is_some_and(|total| total != size) {
            return Err(Error::Invalid(crate::tr!("the download was cut short")));
        }
        file.sync_all().await?;
        Ok(())
    }
    .await;
    match written {
        Ok(()) => {
            tokio::fs::rename(&part, to).await?;
            Ok(validator)
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(error)
        }
    }
}
