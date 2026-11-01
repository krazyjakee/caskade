//! The archive: a zip of the files, then `manifest.json` with each file's
//! size and SHA-256. [`verify`] reads every byte back against the manifest,
//! and refuses unsafe paths, unlisted entries and oversized claims, so it
//! can open archives from anywhere (#192).

use super::{
    BackupError,
    manifest::{self, Component, FileEntry, Manifest},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// At most this many files in an archive.
const MAX_FILES: usize = 1_000_000;
/// At most this many bytes of manifest.
const MAX_MANIFEST: u64 = 64 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

/// A file to put in the archive.
pub(super) struct Source {
    pub path: String,
    pub component: Component,
    pub file: PathBuf,
}

/// Where the bytes are, for progress.
pub(super) type Report<'a> = &'a (dyn Fn(u64) + Send + Sync);

/// Writes `sources` and then the manifest (its `files` filled in here) to
/// `out`. A source that vanished since it was listed (a plugin removed
/// meanwhile) is left out; the library snapshot never is.
pub(super) fn write<W: Write + Seek>(
    out: W,
    sources: &[Source],
    mut manifest: Manifest,
    cancel: &CancellationToken,
    report: Report<'_>,
) -> Result<(W, Manifest), BackupError> {
    let mut zip = ZipWriter::new(out);
    let mut done = 0u64;
    let mut buffer = vec![0u8; CHUNK];
    for source in sources {
        let mut file = match File::open(&source.file) {
            Ok(file) => file,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && source.component != Component::Library =>
            {
                tracing::debug!(path = %source.path, "left out of the backup: it's gone");
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let size = file.metadata()?.len();
        let options = SimpleFileOptions::default()
            .compression_method(method(&source.path))
            .large_file(size >= u64::from(u32::MAX))
            .unix_permissions(0o644);
        zip.start_file(source.path.as_str(), options)
            .map_err(zip_error)?;
        let mut hash = Sha256::new();
        let mut written = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(BackupError::Cancelled);
            }
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
            zip.write_all(&buffer[..read])?;
            written += read as u64;
            done += read as u64;
            report(done);
        }
        manifest.files.push(FileEntry {
            path: source.path.clone(),
            component: source.component,
            size: written,
            sha256: format!("{:x}", hash.finalize()),
        });
    }
    for summary in &mut manifest.components {
        let files = manifest.files.iter().filter(|f| f.component == summary.id);
        summary.files = u32::try_from(files.clone().count()).unwrap_or(u32::MAX);
        summary.bytes = files.map(|f| f.size).sum();
    }
    zip.start_file(
        manifest::MANIFEST,
        SimpleFileOptions::default().unix_permissions(0o644),
    )
    .map_err(zip_error)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    let out = zip.finish().map_err(zip_error)?;
    Ok((out, manifest))
}

/// Images are compressed already.
fn method(path: &str) -> CompressionMethod {
    let lower = path.to_ascii_lowercase();
    if [".webp", ".png", ".jpg", ".jpeg", ".zip", ".gz"]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    }
}

/// Reads the archive at `path` back: the manifest must be readable and of a
/// format this Caskade knows, every entry listed and safe, every listed file
/// there with its size and SHA-256, and the library snapshot among them.
pub fn verify(
    path: &Path,
    cancel: &CancellationToken,
    report: Report<'_>,
) -> Result<Manifest, BackupError> {
    let mut archive = ZipArchive::new(File::open(path)?).map_err(damaged)?;
    let manifest = read_manifest(&mut archive)?;
    let mut listed = BTreeSet::new();
    for file in &manifest.files {
        safe_path(&file.path, file.component)?;
        if !listed.insert(file.path.as_str()) {
            return Err(BackupError::Damaged(crate::tr!(
                "“{path}” is listed twice",
                path = file.path
            )));
        }
    }
    if !manifest
        .files
        .iter()
        .any(|f| f.path == manifest::DATABASE && f.component == Component::Library)
    {
        return Err(BackupError::Damaged(crate::tr!(
            "the backup has no library database"
        )));
    }
    // Every entry is the manifest or a listed file, and each only once.
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let name = archive
            .name_for_index(index)
            .and_then(Result::ok)
            .ok_or_else(|| BackupError::Damaged(crate::tr!("the archive's index is damaged")))?
            .into_owned();
        if !seen.insert(name.clone())
            || (name != manifest::MANIFEST && !listed.contains(name.as_str()))
        {
            return Err(BackupError::Damaged(crate::tr!(
                "the archive holds “{path}”, which its manifest doesn't list",
                path = name
            )));
        }
    }
    let mut done = 0u64;
    let mut buffer = vec![0u8; CHUNK];
    for file in &manifest.files {
        let mut entry = archive.by_name(&file.path).map_err(|_| {
            BackupError::Damaged(crate::tr!("“{path}” is missing", path = file.path))
        })?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        // One byte more than claimed is enough to tell it lied.
        let mut limited = (&mut entry).take(file.size.saturating_add(1));
        loop {
            if cancel.is_cancelled() {
                return Err(BackupError::Cancelled);
            }
            let read = limited.read(&mut buffer).map_err(damaged_io)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
            size += read as u64;
            done += read as u64;
            report(done);
        }
        if size != file.size || format!("{:x}", hash.finalize()) != file.sha256 {
            return Err(BackupError::Damaged(crate::tr!(
                "“{path}” doesn't match its checksum",
                path = file.path
            )));
        }
    }
    Ok(manifest)
}

/// The manifest alone, checked for its format but not against the files.
pub(super) fn read_manifest<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Manifest, BackupError> {
    if archive.len() > MAX_FILES + 1 {
        return Err(BackupError::Damaged(crate::tr!(
            "the archive holds too many files"
        )));
    }
    let entry = archive
        .by_name(manifest::MANIFEST)
        .map_err(|_| BackupError::Damaged(crate::tr!("this isn't a Caskade backup")))?;
    if entry.size() > MAX_MANIFEST {
        return Err(BackupError::Damaged(crate::tr!(
            "the backup's manifest is too large"
        )));
    }
    let mut text = Vec::new();
    entry
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut text)
        .map_err(damaged_io)?;
    // The format first, so a newer backup says so rather than failing to
    // parse.
    #[derive(serde::Deserialize)]
    struct Format {
        format: u32,
    }
    let format = serde_json::from_slice::<Format>(&text)
        .map_err(|_| BackupError::Damaged(crate::tr!("the backup's manifest is damaged")))?
        .format;
    if format > manifest::FORMAT {
        return Err(BackupError::Newer);
    }
    let manifest: Manifest = serde_json::from_slice(&text)
        .map_err(|_| BackupError::Damaged(crate::tr!("the backup's manifest is damaged")))?;
    if manifest.files.len() > MAX_FILES {
        return Err(BackupError::Damaged(crate::tr!(
            "the archive holds too many files"
        )));
    }
    Ok(manifest)
}

/// A relative `/`-separated path inside the component's folder, with no
/// `.`, `..`, empty, drive or backslash parts.
pub(super) fn safe_path(path: &str, component: Component) -> Result<(), BackupError> {
    let parts: Vec<&str> = path.split('/').collect();
    let safe = path.len() <= 1024
        && parts.len() >= 2
        && parts[0] == component.folder()
        && parts.iter().all(|part| {
            !part.is_empty() && *part != "." && *part != ".." && !part.contains(['\\', ':', '\0'])
        });
    if safe {
        Ok(())
    } else {
        Err(BackupError::Damaged(crate::tr!(
            "the backup holds an unsafe path: “{path}”",
            path = path
        )))
    }
}

fn zip_error(error: zip::result::ZipError) -> BackupError {
    match error {
        zip::result::ZipError::Io(error) => error.into(),
        error => BackupError::Io(std::io::Error::other(error)),
    }
}

fn damaged(error: zip::result::ZipError) -> BackupError {
    match error {
        zip::result::ZipError::Io(error) if error.kind() != std::io::ErrorKind::InvalidData => {
            error.into()
        }
        _ => BackupError::Damaged(crate::tr!("the file isn't a readable backup archive")),
    }
}

/// A read inside an entry failing is the archive's fault (a bad deflate
/// stream, a CRC mismatch), unless the drive failed.
fn damaged_io(error: std::io::Error) -> BackupError {
    match error.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
            BackupError::Damaged(crate::tr!("the backup archive is damaged"))
        }
        _ => error.into(),
    }
}
