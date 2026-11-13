//! Bounded archive listing and streaming extraction through the safety layer.

mod common;
mod handler;
mod payload;
mod probe;
mod seven;
mod seven_header;
pub mod sevenzip;
pub mod sfx;
mod tar;
mod volume;
mod zip;

pub use common::{
    ArchiveError, ArchiveOptions, Entry, EntryKind, EventProgress, ExtractionControl, NoopControl,
    Result,
};
pub use handler::{ArchiveExtractor, ArchiveHandler};
pub use payload::PayloadReader;
pub use probe::ArchiveProbe;
#[cfg(any(feature = "linux-formats", feature = "macos-formats"))]
pub(crate) use tar::decoded;

use crate::{Format, probe::ProbeEngine, safety::SafeDir};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub fn supported(format: Format) -> bool {
    matches!(
        format,
        Format::Zip
            | Format::SevenZip
            | Format::Tar
            | Format::Gzip
            | Format::Xz
            | Format::Zstd
            | Format::Bzip2
    )
}

pub fn list(
    mut reader: impl Read + Seek,
    format: Format,
    options: &ArchiveOptions,
) -> Result<Vec<Entry>> {
    let packed = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut bounded = options.clone();
    bounded.max_unpacked_bytes = bounded
        .max_unpacked_bytes
        .min(packed.saturating_mul(options.max_expansion_ratio));
    let entries = match format {
        Format::Zip => zip::list(reader, &bounded),
        Format::SevenZip => seven::list(reader, &bounded),
        Format::Tar | Format::Gzip | Format::Xz | Format::Zstd | Format::Bzip2 => {
            tar::list(reader, format, &bounded)
        }
        _ => Err(ArchiveError::Unsupported(format!("{format:?}"))),
    }?;
    let total = entries.iter().try_fold(0_u64, |sum, entry| {
        sum.checked_add(entry.size)
            .ok_or(crate::safety::SafetyError::ByteLimit)
    })?;
    if total > bounded.max_unpacked_bytes {
        return Err(crate::safety::SafetyError::ByteLimit.into());
    }
    Ok(entries)
}

/// Stream one resolved archive into staging. This never runs extracted programs.
pub fn extract(
    mut reader: impl Read + Seek,
    format: Format,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    control.checkpoint(0)?;
    reader.seek(SeekFrom::Start(0))?;
    match format {
        Format::Zip => zip::extract(reader, dir, options, control)?,
        Format::SevenZip => seven::extract(reader, dir, options, control)?,
        Format::Tar | Format::Gzip | Format::Xz | Format::Zstd | Format::Bzip2 => {
            tar::extract(reader, format, dir, options, control)?
        }
        _ => return Err(ArchiveError::Unsupported(format!("{format:?}"))),
    }
    Ok(())
}

/// File names of tarballs a release can wrap in another archive.
const TARBALLS: [&str; 9] = [
    ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.zst", ".tar.bz2", ".tbz2", ".tbz",
];

/// Unpacks a tarball that was the only thing in the archive just extracted
/// into `dir`, in its place: releases are often zipped around a tarball so
/// their programs keep their executable bits through GitHub's artifacts.
/// Only a top-level file named and identified as a tarball counts, never
/// another kind of archive (a zip inside a zip may be a ROM set). Shares
/// `dir`'s budget.
pub fn unwrap_tarball(
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let only = {
        let mut files = dir.extracted_files();
        match (files.next(), files.next()) {
            (Some(only), None) => only.to_path_buf(),
            _ => return Ok(()),
        }
    };
    let Some(name) = only.to_str().map(str::to_owned) else {
        return Ok(());
    };
    let lower = name.to_lowercase();
    if only.components().count() != 1 || !TARBALLS.iter().any(|ending| lower.ends_with(ending)) {
        return Ok(());
    }
    let format = ProbeEngine::default()
        .probe_reader(&mut dir.open_read(&name)?, &only)
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?
        .root
        .format;
    if !matches!(
        format,
        Format::Tar | Format::Gzip | Format::Xz | Format::Zstd | Format::Bzip2
    ) {
        return Ok(());
    }
    {
        let file = dir.open_read(&name)?;
        tar::extract(file, format, dir, options, control)?;
    }
    dir.remove_extracted(&name)?;
    Ok(())
}

/// Resolve multipart siblings before opening the parser. `SafeDir` must use the
/// complete source byte length as its expansion-ratio denominator.
pub fn extract_path(
    path: &Path,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let (reader, format) = handler::source(path)?;
    extract(reader, format, dir, options, control)
}

pub fn list_path(path: &Path, options: &ArchiveOptions) -> Result<Vec<Entry>> {
    let (reader, format) = handler::source(path)?;
    list(reader, format, options)
}
