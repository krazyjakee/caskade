use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{FetchError, Result, safe_file_name};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct State {
    pub version: u32,
    pub request_key: String,
    pub file_name: String,
    pub final_url: Url,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub date: Option<String>,
    pub total: Option<u64>,
    pub complete: bool,
}

impl State {
    pub fn validator(&self) -> Option<&str> {
        if let Some(etag) = &self.etag {
            return (!etag.starts_with("W/") && etag.starts_with('"') && etag.ends_with('"'))
                .then_some(etag.as_str());
        }
        // RFC 9110: a Last-Modified date is a strong client validator only when
        // Date establishes at least sixty seconds of separation.
        let modified = httpdate::parse_http_date(self.last_modified.as_deref()?).ok()?;
        let date = httpdate::parse_http_date(self.date.as_deref()?).ok()?;
        (date.duration_since(modified).ok()?.as_secs() >= 60)
            .then_some(self.last_modified.as_deref()?)
    }
}

pub(super) struct Storage {
    pub job: Dir,
    pub states: Dir,
}
impl Storage {
    pub fn open(root: &Path, job_id: &str) -> Result<Self> {
        safe_file_name(job_id)?;
        let root = crate::safety::open_staging(root)?;
        let job = directory(&root, &Path::new(".caskade/downloads").join(job_id), true)?;
        let states = directory(&job, Path::new(".state"), true)?;
        Ok(Self { job, states })
    }
    pub fn read(&self, key: &str) -> Result<Option<State>> {
        let name = format!("{key}.json");
        match self.states.symlink_metadata(&name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(metadata)
                if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 64 * 1024 =>
            {
                return Err(FetchError::InvalidRequest {
                    message: "invalid download state file".into(),
                });
            }
            Ok(_) => (),
        }
        let state: State = serde_json::from_reader(self.states.open(name)?)?;
        safe_file_name(&state.file_name)?;
        if state.version != 1 || state.request_key != key {
            return Err(FetchError::InvalidRequest {
                message: "download state identity mismatch".into(),
            });
        }
        Ok(Some(state))
    }
    pub fn write(&self, state: &State) -> Result<()> {
        let mut bytes = serde_json::to_vec(state)?;
        bytes.push(b'\n');
        replace_file(&self.states, &format!("{}.json", state.request_key), &bytes)?;
        #[cfg(unix)]
        self.states.open(".")?.sync_all()?;
        Ok(())
    }
    pub fn purge(&self, key: &str) -> Result<()> {
        if let Some(state) = self.read(key)? {
            remove_file(&self.job, &format!("{}.part", state.file_name))?;
            remove_file(&self.job, &state.file_name)?;
        }
        remove_file(&self.states, &format!("{key}.json"))
    }
    pub fn part_len(&self, state: &State) -> Result<u64> {
        regular_len(&self.job, &format!("{}.part", state.file_name)).map(|value| value.unwrap_or(0))
    }
    pub fn choose_name(&self, name: &str) -> Result<String> {
        safe_file_name(name)?;
        let mut candidate = name.to_owned();
        for suffix in 1..=100_000 {
            if suffix > 1 {
                let path = Path::new(name);
                candidate = match (path.file_stem(), path.extension()) {
                    (Some(stem), Some(extension)) => format!(
                        "{} ({suffix}).{}",
                        stem.to_string_lossy(),
                        extension.to_string_lossy()
                    ),
                    _ => format!("{name} ({suffix})"),
                };
            }
            if self
                .job
                .symlink_metadata(&candidate)
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
                && self
                    .job
                    .symlink_metadata(format!("{candidate}.part"))
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
            {
                return Ok(candidate);
            }
        }
        Err(FetchError::InvalidRequest {
            message: "download name collision limit reached".into(),
        })
    }
    pub fn open_part(&self, state: &State, append: bool) -> Result<fs::File> {
        let name = format!("{}.part", state.file_name);
        regular_len(&self.job, &name)?;
        Ok(self
            .job
            .open_with(
                name,
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .append(append)
                    .truncate(!append),
            )?
            .into_std())
    }
}

pub(super) fn regular_len(dir: &Dir, name: &str) -> Result<Option<u64>> {
    match dir.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => Ok(Some(metadata.len())),
        Ok(_) => Err(FetchError::InvalidRequest {
            message: "download entry is not a regular file".into(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn remove_file(dir: &Dir, name: &str) -> Result<()> {
    match dir.remove_file(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result.map_err(Into::into),
    }
}

/// Writes `bytes` to `name` in `dir` through a synced temporary file, so a
/// crash leaves the old file or the new one, never part of it.
pub(super) fn replace_file(dir: &Dir, name: &str, bytes: &[u8]) -> Result<()> {
    let temporary = format!("{name}.tmp");
    remove_file(dir, &temporary)?;
    let mut file = dir.open_with(&temporary, OpenOptions::new().write(true).create_new(true))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    dir.rename(&temporary, dir, name)?;
    Ok(())
}

pub(super) fn directory(root: &Dir, path: &Path, create: bool) -> Result<Dir> {
    let mut dir = root.try_clone()?;
    for component in path.components() {
        let name = Path::new(component.as_os_str());
        match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => (),
            Ok(_) => {
                return Err(FetchError::InvalidRequest {
                    message: "download directory contains a symlink or non-directory".into(),
                });
            }
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                dir.create_dir(name)?
            }
            Err(error) => return Err(error.into()),
        }
        dir = dir.open_dir(name)?;
    }
    Ok(dir)
}

/// Torrent data, one folder per info hash (`<hex>/meta.torrent`, `<hex>/data/`):
/// a later job for the same torrent, after a restart too, verifies and
/// reuses its pieces.
pub(super) const TORRENTS: &str = ".caskade/torrents";
/// In a job's `.state`: the info hash whose data the job downloaded.
const TORRENT_MARKER: &str = "torrent";

/// Whether `value` is a lowercase hex v1 info hash.
pub(super) fn info_hash(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Storage {
    /// Records that this job's download is torrent `hash`'s data.
    #[cfg_attr(not(feature = "torrent"), allow(dead_code))]
    pub fn mark_torrent(&self, hash: &str) -> Result<()> {
        replace_file(&self.states, TORRENT_MARKER, hash.as_bytes())
    }
}

/// The info hash recorded in job folder `job`, if any.
pub(super) fn torrent_marker(job: &Dir) -> Option<String> {
    let states = job.open_dir(".state").ok()?;
    let metadata = states.symlink_metadata(TORRENT_MARKER).ok()?;
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 64 {
        return None;
    }
    let hash = states.read_to_string(TORRENT_MARKER).ok()?;
    let hash = hash.trim();
    info_hash(hash).then(|| hash.to_owned())
}

/// Deletes torrent `hash`'s data under `root` (a capability for the root).
pub(super) fn remove_torrent(root: &Dir, hash: &str) -> Result<()> {
    let path = Path::new(TORRENTS).join(hash);
    match root.symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(_) => (),
    }
    directory(root, &path, false)?;
    root.remove_dir_all(&path)?;
    if root
        .symlink_metadata(TORRENTS)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
    {
        let _ = root.remove_dir(TORRENTS);
    }
    Ok(())
}

pub(super) fn prune(root: &Path, job_id: &str) {
    if let Ok(root) = crate::safety::open_staging(root) {
        for path in [
            Path::new(".caskade/downloads").join(job_id).join(".state"),
            Path::new(".caskade/downloads").join(job_id),
            PathBuf::from(".caskade/downloads"),
            PathBuf::from(".caskade"),
        ] {
            if root
                .symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
            {
                let _ = root.remove_dir(path);
            }
        }
    }
}
