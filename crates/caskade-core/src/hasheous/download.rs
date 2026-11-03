//! Hasheous's listing and dump downloads. A dump is streamed to a `.part`
//! file next to its destination and renamed into place only when its size
//! and MD5 match the listing.

use super::{Listed, hex};
use crate::{Error, Result, error::net};
use md5::Digest;
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

/// The listing is ~100 KiB for ~650 dumps.
const MAX_LISTING: usize = 8 << 20;
/// The most the listing may take in all: a server trickling it out must
/// not hold the sync (and its lock) forever.
const LISTING_TIMEOUT: Duration = Duration::from_secs(60);

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // Dumps reach hundreds of MiB: no total timeout, only a stall one.
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(net)
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

pub(super) async fn listing(base: &str) -> Result<Vec<Listed>> {
    let mut response = client()?
        .get(format!("{base}/api/v1/Dumps/platforms?includeDetails=true"))
        .timeout(LISTING_TIMEOUT)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        if chunk.len() > MAX_LISTING - bytes.len() {
            return Err(invalid(crate::tr!("Hasheous listing exceeds 8 MiB")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Where the dump file `name` (with `.zip`) downloads from.
pub(super) fn dump_url(base: &str, name: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(&format!("{base}/api/v1/Dumps/platforms/"))
        .map_err(|error| invalid(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| invalid("Hasheous URL cannot be a base"))?
        .pop_if_empty()
        .push(name);
    Ok(url)
}

pub(super) async fn fetch(base: &str, listed: &Listed, to: &Path) -> Result<()> {
    let url = dump_url(base, &listed.name)?;
    let mut response = client()?
        .get(url)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?;
    let expected = listed.size_bytes;
    if response
        .content_length()
        .is_some_and(|length| length != expected)
    {
        return Err(invalid(crate::tr!(
            "{dump} is not the listed size",
            dump = listed.name
        )));
    }
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut part = to.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let written = async {
        let mut file = tokio::fs::File::create(&part).await?;
        let mut hasher = md5::Md5::new();
        let mut size = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(net)? {
            size += chunk.len() as u64;
            if size > expected {
                return Err(invalid(crate::tr!(
                    "{dump} is larger than listed",
                    dump = listed.name
                )));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        if size != expected {
            return Err(invalid(crate::tr!(
                "{dump} is shorter than listed",
                dump = listed.name
            )));
        }
        if !hex(&hasher.finalize()).eq_ignore_ascii_case(&listed.md5hash) {
            return Err(invalid(crate::tr!(
                "{dump} does not match its MD5",
                dump = listed.name
            )));
        }
        file.sync_all().await?;
        Ok(())
    }
    .await;
    match written {
        Ok(()) => Ok(tokio::fs::rename(&part, to).await?),
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(error)
        }
    }
}
