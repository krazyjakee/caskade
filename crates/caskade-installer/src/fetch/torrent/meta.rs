//! Torrent metadata on disk: `.torrent` files, and a magnet link's metadata
//! kept beside its data for later jobs.

use std::{io::Read, path::Path};

use cap_std::fs::Dir;

use crate::fetch::{FetchError, Result, storage};

const METADATA: &str = "meta.torrent";

/// Whether the downloaded file at `path` is torrent metadata: named
/// `.torrent`, or a small bencoded file that parses as one.
pub fn is_metadata(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent"))
    {
        return true;
    }
    let limit = super::TorrentConfig::default().max_metadata_bytes;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    let mut first = [0; 1];
    if !metadata.is_file()
        || metadata.len() > limit
        || file.read_exact(&mut first).is_err()
        || first[0] != b'd'
    {
        return false;
    }
    std::fs::read(path).is_ok_and(|bytes| librqbit::torrent_from_bytes(&bytes).is_ok())
}

pub(super) fn hash_of(bytes: &[u8]) -> Result<String> {
    librqbit::torrent_from_bytes(bytes)
        .map(|torrent| torrent.info_hash.as_string())
        .map_err(|error| FetchError::Torrent {
            message: format!("not a BitTorrent v1 or hybrid torrent: {error}"),
        })
}

pub(super) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(FetchError::Torrent {
            message: format!("the .torrent file is larger than {limit} bytes"),
        });
    }
    Ok(bytes)
}

/// A magnet link's metadata from an earlier job, if it still matches.
pub(super) fn cached(folder: &Dir, hash: &str, limit: u64) -> Option<Vec<u8>> {
    let metadata = folder.symlink_metadata(METADATA).ok()?;
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > limit {
        return None;
    }
    let bytes = folder.read(METADATA).ok()?;
    (hash_of(&bytes).ok()? == hash).then_some(bytes)
}

pub(super) fn write_cached(folder: &Dir, bytes: &[u8]) -> Result<()> {
    storage::replace_file(folder, METADATA, bytes)
}
