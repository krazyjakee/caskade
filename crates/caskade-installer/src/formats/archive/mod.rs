//! Bounded archive listing and streaming extraction through the safety layer.
//! Extracted regular files are re-probed statically; their artifact trees can be
//! sent back through the planner to find portable games or nested installers.

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
#[cfg(feature = "linux-formats")]
pub(crate) use tar::decoded;

use crate::{ArtifactId, ArtifactNode, Format, probe::ProbeEngine, safety::SafeDir};
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

/// Stream one resolved archive into staging, then identify every completed
/// regular file from its bytes. This never runs extracted programs.
pub fn extract(
    mut reader: impl Read + Seek,
    format: Format,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<Vec<ArtifactNode>> {
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
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(ArchiveProbe));
    let mut artifacts = Vec::new();
    for path in dir.extracted_files() {
        control.checkpoint(0)?;
        let name = path
            .to_str()
            .ok_or(crate::safety::SafetyError::InvalidEncoding)?;
        let mut file = dir.open_read(name)?;
        let mut node = engine
            .probe_reader(&mut file, path)
            .map_err(|e| ArchiveError::Invalid(e.to_string()))?
            .root;
        node.id = ArtifactId(format!("extracted:{}", artifacts.len()));
        artifacts.push(node);
    }
    Ok(artifacts)
}

/// Resolve multipart siblings before opening the parser. `SafeDir` must use the
/// complete source byte length as its expansion-ratio denominator.
pub fn extract_path(
    path: &Path,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<Vec<ArtifactNode>> {
    let (reader, format) = handler::source(path)?;
    extract(reader, format, dir, options, control)
}

pub fn list_path(path: &Path, options: &ArchiveOptions) -> Result<Vec<Entry>> {
    let (reader, format) = handler::source(path)?;
    list(reader, format, options)
}
