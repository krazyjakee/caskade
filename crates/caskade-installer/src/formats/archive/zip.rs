use super::common::*;
use crate::safety::{self, SafeDir};
use std::io::{Read, Seek, SeekFrom};
use zip::read::HasZipMetadata;

/// Bound central-directory allocation before zip constructs its entry table.
/// ZIP64 central directories are accepted when their metadata fits the same cap.
fn preflight(reader: &mut (impl Read + Seek), options: &ArchiveOptions) -> Result<()> {
    let length = reader.seek(SeekFrom::End(0))?;
    let tail_length = length.min(65_557) as usize;
    reader.seek(SeekFrom::End(-(tail_length as i64)))?;
    let mut tail = vec![0; tail_length];
    reader.read_exact(&mut tail)?;
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            tail.get(i..i + 4) == Some(b"PK\x05\x06")
                && i + 22 + usize::from(u16::from_le_bytes([tail[i + 20], tail[i + 21]]))
                    == tail.len()
        })
        .ok_or_else(|| ArchiveError::Invalid("missing ZIP end record".into()))?;
    let count = u16::from_le_bytes(tail[eocd + 10..eocd + 12].try_into().unwrap()) as u64;
    let size = u32::from_le_bytes(tail[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
    let (count, size) = if count == u16::MAX as u64 || size == u32::MAX as u64 {
        if eocd < 20 || tail.get(eocd - 20..eocd - 16) != Some(b"PK\x06\x07") {
            return Err(ArchiveError::Invalid("missing ZIP64 locator".into()));
        }
        let offset = u64::from_le_bytes(tail[eocd - 12..eocd - 4].try_into().unwrap());
        reader.seek(SeekFrom::Start(offset))?;
        let mut record = [0; 56];
        reader.read_exact(&mut record)?;
        if &record[..4] != b"PK\x06\x06" {
            return Err(ArchiveError::Invalid("invalid ZIP64 end record".into()));
        }
        (
            u64::from_le_bytes(record[32..40].try_into().unwrap()),
            u64::from_le_bytes(record[40..48].try_into().unwrap()),
        )
    } else {
        (count, size)
    };
    if count > options.max_entries as u64 {
        return Err(safety::SafetyError::EntryLimit.into());
    }
    if size > options.max_metadata_bytes as u64 {
        return Err(ArchiveError::MemoryLimit);
    }
    reader.seek(SeekFrom::Start(0))?;
    Ok(())
}
fn open<R: Read + Seek>(mut reader: R, options: &ArchiveOptions) -> Result<zip::ZipArchive<R>> {
    preflight(&mut reader, options)?;
    let archive = zip::ZipArchive::new(reader).map_err(|e| ArchiveError::Invalid(e.to_string()))?;
    if archive.len() > options.max_entries {
        return Err(safety::SafetyError::EntryLimit.into());
    }
    Ok(archive)
}
fn metadata<R: Read>(file: &zip::read::ZipFile<'_, R>, options: &ArchiveOptions) -> Result<Entry> {
    if file.encrypted() {
        return Err(ArchiveError::Unsupported(
            "encrypted ZIP requires a password".into(),
        ));
    }
    // EFS (APPNOTE 4.4.4): zip also sets it for a valid Unicode Path extra field.
    let encoding = if file.get_metadata().flags.as_u16() & (1 << 11) != 0 {
        safety::NameEncoding::Utf8
    } else {
        options.legacy_zip_encoding
    };
    let name = safety::decode_name(file.name_raw(), encoding)?;
    safety::sanitize_path(&name)?;
    let mode = file.unix_mode().unwrap_or(0o644);
    let kind = match mode & 0o170000 {
        0o010000 => return Err(safety::SafetyError::Fifo.into()),
        0o020000 | 0o060000 => return Err(safety::SafetyError::DeviceNode.into()),
        0o140000 => return Err(safety::SafetyError::SpecialFile.into()),
        _ if file.is_symlink() => EntryKind::Symlink,
        _ if file.is_dir() => EntryKind::Directory,
        _ => EntryKind::File,
    };
    Ok(Entry {
        name,
        kind,
        size: file.size(),
        mode: safety::safe_mode(mode),
        target: None,
    })
}

pub(super) fn list(reader: impl Read + Seek, options: &ArchiveOptions) -> Result<Vec<Entry>> {
    let mut archive = open(reader, options)?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| ArchiveError::Invalid(e.to_string()))?;
        let mut meta = metadata(&file, options)?;
        if meta.kind == EntryKind::Symlink {
            meta.target = Some(link_contents(&mut file, options)?);
        }
        entries.push(meta);
    }
    Ok(entries)
}
pub(super) fn extract(
    reader: impl Read + Seek,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let mut archive = open(reader, options)?;
    let selection = Selection::new(options)?;
    let mut links = Vec::new();
    for index in 0..archive.len() {
        control.checkpoint(0)?;
        let mut file = archive
            .by_index(index)
            .map_err(|e| ArchiveError::Invalid(e.to_string()))?;
        let meta = metadata(&file, options)?;
        let restore_exec = u8::from(file.get_metadata().system) != 3 || file.unix_mode().is_none();
        let Some(name) = selection.path(&meta.name)? else {
            continue;
        };
        match meta.kind {
            EntryKind::Directory => {
                dir.create_dir(&name, options.depth)?;
                control.progress(0, 1)?;
            }
            EntryKind::File => write_stream(
                dir,
                &name,
                &mut file,
                meta.size,
                FileMode {
                    bits: meta.mode,
                    restore_exec,
                },
                options,
                control,
            )?,
            EntryKind::Symlink => links.push((name, link_contents(&mut file, options)?)),
            EntryKind::Hardlink => unreachable!(),
        }
    }
    finish_links(dir, links, options, control)
}
