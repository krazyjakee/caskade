use super::{common::*, seven_header::Prepared};
use crate::safety::{self, SafeDir};
use std::io::{Read, Seek};

fn metadata(file: &sevenz_rust2::ArchiveEntry) -> Result<Entry> {
    if file.is_anti_item {
        return Err(ArchiveError::Unsupported("7z anti-item".into()));
    }
    safety::sanitize_path(&file.name)?;
    let unix_mode = if file.has_windows_attributes && file.windows_attributes & 0x8000 != 0 {
        file.windows_attributes >> 16
    } else {
        0
    };
    let kind = match unix_mode & 0o170000 {
        0o010000 => return Err(safety::SafetyError::Fifo.into()),
        0o020000 | 0o060000 => return Err(safety::SafetyError::DeviceNode.into()),
        0o140000 => return Err(safety::SafetyError::SpecialFile.into()),
        0o120000 => EntryKind::Symlink,
        _ if file.is_directory => EntryKind::Directory,
        _ => EntryKind::File,
    };
    let mode = if unix_mode == 0 {
        if file.is_directory { 0o755 } else { 0o644 }
    } else {
        safety::safe_mode(unix_mode)
    };
    Ok(Entry {
        name: file.name.clone(),
        kind,
        size: file.size,
        mode,
        target: None,
    })
}
pub(super) fn list(reader: impl Read + Seek, options: &ArchiveOptions) -> Result<Vec<Entry>> {
    let mut reader = Prepared::new(reader, options)?;
    let archive = sevenz_rust2::Archive::read(&mut reader, &sevenz_rust2::Password::empty())
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?;
    archive.files.iter().map(metadata).collect()
}
pub(super) fn extract(
    reader: impl Read + Seek,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let reader = Prepared::new(reader, options)?;
    let mut archive = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?;
    archive.set_thread_count(1);
    // Validate the whole entry table before decompressing any payload.
    for entry in &archive.archive().files {
        metadata(entry)?;
    }
    let selection = Selection::new(options)?;
    let mut links = Vec::new();
    let mut failure = None;
    let result = archive.for_each_entries(|file, reader| {
        let operation = (|| {
            control.checkpoint(0)?;
            let meta = metadata(file)?;
            let Some(name) = selection.path(&meta.name)? else {
                // A solid archive still needs to consume skipped streams. Check
                // cancellation between bounded reads rather than letting the
                // upstream decoder discard an arbitrarily large stream at once.
                let mut checked = Checked { reader, control };
                std::io::copy(&mut checked, &mut std::io::sink())?;
                return Ok(());
            };
            match meta.kind {
                EntryKind::Directory => {
                    dir.create_dir(&name, options.depth)?;
                    control.progress(0, 1)?;
                }
                EntryKind::File => write_stream(
                    dir,
                    &name,
                    reader,
                    meta.size,
                    FileMode {
                        bits: meta.mode,
                        restore_exec: false,
                    },
                    options,
                    control,
                )?,
                EntryKind::Symlink => {
                    links.push((name, link_contents(Checked { reader, control }, options)?))
                }
                EntryKind::Hardlink => unreachable!(),
            }
            Ok(())
        })();
        if let Err(error) = operation {
            failure = Some(error);
            return Err(sevenz_rust2::Error::Other("extraction rejected".into()));
        }
        Ok(true)
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(|e| ArchiveError::Invalid(e.to_string()))?;
    finish_links(dir, links, options, control)
}
