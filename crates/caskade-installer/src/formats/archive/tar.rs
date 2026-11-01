use super::common::*;
use crate::{
    Format,
    safety::{self, SafeDir},
};
use std::io::Read;

struct DecodedBudget<R> {
    reader: R,
    remaining: u64,
}
impl<R: Read> Read for DecodedBudget<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut extra = [0; 1];
            return if self.reader.read(&mut extra)? == 0 {
                Ok(0)
            } else {
                Err(std::io::Error::other(safety::SafetyError::ByteLimit))
            };
        }
        let length = buffer.len().min(self.remaining.min(65536) as usize);
        let count = self.reader.read(&mut buffer[..length])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}
fn budget<R: Read>(reader: R, options: &ArchiveOptions) -> DecodedBudget<R> {
    DecodedBudget {
        reader,
        remaining: options
            .max_unpacked_bytes
            .saturating_add(options.max_metadata_bytes as u64)
            .saturating_add((options.max_entries as u64).saturating_mul(1024))
            .saturating_add(1024),
    }
}

pub(crate) fn decoded<'a>(
    reader: impl Read + 'a,
    format: Format,
    options: &ArchiveOptions,
) -> Result<Box<dyn Read + 'a>> {
    Ok(match format {
        Format::Tar => Box::new(reader),
        Format::Gzip => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        Format::Xz => Box::new(lzma_rust2::XzReader::new_mem_limit(
            reader,
            true,
            (options.max_decoder_bytes / 1024).min(u32::MAX as u64) as u32,
        )),
        Format::Zstd => Box::new(
            ruzstd::decoding::StreamingDecoder::new_with_max_window_size(
                reader,
                options.max_decoder_bytes,
            )
            .map_err(|e| ArchiveError::Invalid(e.to_string()))?,
        ),
        Format::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(reader)),
        _ => return Err(ArchiveError::Unsupported(format!("{format:?}"))),
    })
}

fn metadata<R: Read>(entry: &tar::Entry<'_, R>) -> Result<Entry> {
    let header = entry.header();
    let ty = header.entry_type();
    let kind = if ty.is_dir() {
        EntryKind::Directory
    } else if ty.is_file() || ty.is_contiguous() {
        EntryKind::File
    } else if ty.is_symlink() {
        EntryKind::Symlink
    } else if ty.is_hard_link() {
        EntryKind::Hardlink
    } else {
        return Err(if ty.is_fifo() {
            safety::SafetyError::Fifo
        } else if ty.is_block_special() || ty.is_character_special() {
            safety::SafetyError::DeviceNode
        } else {
            safety::SafetyError::SpecialFile
        }
        .into());
    };
    let name = safety::decode_name(&entry.path_bytes(), safety::NameEncoding::Detect)?;
    // Conventional tar root entries are handled separately by callers.
    if name != "." && name != "./" {
        safety::sanitize_path(&name)?;
    }
    let target = entry
        .link_name_bytes()
        .map(|bytes| safety::decode_name(&bytes, safety::NameEncoding::Detect))
        .transpose()?;
    Ok(Entry {
        name,
        kind,
        size: entry.size(),
        mode: safety::safe_mode(header.mode()?),
        target,
    })
}

#[derive(Default)]
struct Extensions {
    name: Option<String>,
    link: Option<String>,
    size: Option<u64>,
    bytes: usize,
}
impl Extensions {
    fn metadata<R: Read>(
        &mut self,
        entry: &mut tar::Entry<'_, R>,
        options: &ArchiveOptions,
    ) -> Result<Option<Entry>> {
        let ty = entry.header().entry_type();
        if ty.is_gnu_longname()
            || ty.is_gnu_longlink()
            || ty.is_pax_local_extensions()
            || ty.is_pax_global_extensions()
        {
            if entry.size() > options.max_metadata_bytes as u64 {
                return Err(ArchiveError::MemoryLimit);
            }
            let mut bytes = Vec::new();
            (&mut *entry)
                .take(options.max_metadata_bytes as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 != entry.size() {
                return Err(ArchiveError::Invalid("truncated tar extension".into()));
            }
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .ok_or(ArchiveError::MemoryLimit)?;
            if self.bytes > options.max_metadata_bytes {
                return Err(ArchiveError::MemoryLimit);
            }
            if ty.is_gnu_longname() || ty.is_gnu_longlink() {
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                let value = safety::decode_name(&bytes, safety::NameEncoding::Detect)?;
                let slot = if ty.is_gnu_longname() {
                    &mut self.name
                } else {
                    &mut self.link
                };
                if slot.replace(value).is_some() {
                    return Err(ArchiveError::Invalid("duplicate tar extension".into()));
                }
            } else {
                let mut remaining = bytes.as_slice();
                while !remaining.is_empty() {
                    let space = remaining
                        .iter()
                        .position(|b| *b == b' ')
                        .ok_or_else(|| ArchiveError::Invalid("invalid PAX record".into()))?;
                    let length: usize = std::str::from_utf8(&remaining[..space])
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| ArchiveError::Invalid("invalid PAX length".into()))?;
                    if length <= space + 2
                        || length > remaining.len()
                        || remaining[length - 1] != b'\n'
                    {
                        return Err(ArchiveError::Invalid("invalid PAX length".into()));
                    }
                    let record = &remaining[space + 1..length - 1];
                    let equals = record
                        .iter()
                        .position(|b| *b == b'=')
                        .ok_or_else(|| ArchiveError::Invalid("invalid PAX value".into()))?;
                    let key = &record[..equals];
                    let value = &record[equals + 1..];
                    if key.starts_with(b"GNU.sparse") {
                        return Err(ArchiveError::Unsupported("sparse tar entries".into()));
                    }
                    if matches!(key, b"path" | b"linkpath" | b"size")
                        && ty.is_pax_global_extensions()
                    {
                        return Err(ArchiveError::Unsupported("global PAX path/size".into()));
                    }
                    match key {
                        b"path" => {
                            self.name =
                                Some(safety::decode_name(value, safety::NameEncoding::Utf8)?)
                        }
                        b"linkpath" => {
                            self.link =
                                Some(safety::decode_name(value, safety::NameEncoding::Utf8)?)
                        }
                        b"size" => {
                            self.size = Some(
                                std::str::from_utf8(value)
                                    .ok()
                                    .and_then(|v| v.parse().ok())
                                    .ok_or_else(|| {
                                        ArchiveError::Invalid("invalid PAX size".into())
                                    })?,
                            )
                        }
                        _ => (),
                    }
                    remaining = &remaining[length..];
                }
            }
            return Ok(None);
        }
        let mut meta = metadata(entry)?;
        if let Some(name) = self.name.take() {
            if name != "." && name != "./" {
                safety::sanitize_path(&name)?;
            }
            meta.name = name;
        }
        if let Some(link) = self.link.take() {
            meta.target = Some(link);
        }
        if let Some(size) = self.size.take()
            && size != meta.size
        {
            return Err(ArchiveError::Unsupported(
                "PAX size differs from raw tar header".into(),
            ));
        }
        Ok(Some(meta))
    }
    fn finish(&self) -> Result<()> {
        if self.name.is_some() || self.link.is_some() || self.size.is_some() {
            Err(ArchiveError::Invalid("orphan tar extension".into()))
        } else {
            Ok(())
        }
    }
}

pub(super) fn list(
    reader: impl Read,
    format: Format,
    options: &ArchiveOptions,
) -> Result<Vec<Entry>> {
    let stream = decoded(reader, format, options)?;
    let mut archive = tar::Archive::new(budget(stream, options));
    let mut entries = Vec::new();
    let mut raw_count = 0;
    let mut total = 0_u64;
    let mut extensions = Extensions::default();
    for entry in archive.entries()?.raw(true) {
        raw_count += 1;
        if raw_count > options.max_entries {
            return Err(safety::SafetyError::EntryLimit.into());
        }
        let mut raw = entry?;
        let Some(entry) = extensions.metadata(&mut raw, options)? else {
            continue;
        };
        if matches!(entry.name.as_str(), "." | "./") {
            continue;
        }
        total = total
            .checked_add(entry.size)
            .ok_or(safety::SafetyError::ByteLimit)?;
        if total > options.max_unpacked_bytes {
            return Err(safety::SafetyError::ByteLimit.into());
        }
        entries.push(entry);
    }
    extensions.finish()?;
    std::io::copy(&mut archive.into_inner(), &mut std::io::sink())?;
    Ok(entries)
}

pub(super) fn extract(
    reader: impl Read,
    format: Format,
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let shared = std::cell::RefCell::new(control);
    let mut input_control = SharedControl(&shared);
    let checked = Checked {
        reader,
        control: &mut input_control,
    };
    // Decompression reads are checked too, including skipped entry contents.
    let stream = decoded(checked, format, options)?;
    let mut decoded_control = SharedControl(&shared);
    let stream = Checked {
        reader: budget(stream, options),
        control: &mut decoded_control,
    };
    let mut archive = tar::Archive::new(stream);
    let selection = Selection::new(options)?;
    let mut links = Vec::new();
    let mut output_control = SharedControl(&shared);
    let mut count = 0;
    let mut extensions = Extensions::default();
    for entry in archive.entries()?.raw(true) {
        count += 1;
        if count > options.max_entries {
            return Err(safety::SafetyError::EntryLimit.into());
        }
        let mut entry = entry?;
        let Some(meta) = extensions.metadata(&mut entry, options)? else {
            continue;
        };
        if matches!(meta.name.as_str(), "." | "./") {
            continue;
        }
        let Some(name) = selection.path(&meta.name)? else {
            continue;
        };
        match meta.kind {
            EntryKind::Directory => {
                dir.create_dir(&name, options.depth)?;
                output_control.progress(0, 1)?;
            }
            EntryKind::File => write_stream(
                dir,
                &name,
                &mut entry,
                meta.size,
                FileMode {
                    bits: meta.mode,
                    restore_exec: false,
                },
                options,
                &mut output_control,
            )?,
            EntryKind::Symlink => links.push((
                name,
                meta.target
                    .ok_or_else(|| ArchiveError::Invalid("symlink has no target".into()))?,
            )),
            EntryKind::Hardlink => {
                let target = selection.hardlink_target(
                    &meta
                        .target
                        .ok_or_else(|| ArchiveError::Invalid("hardlink has no target".into()))?,
                )?;
                let before = dir.actual_bytes();
                dir.hardlink(&name, &target, options.depth)?;
                output_control.progress(dir.actual_bytes() - before, 1)?;
            }
        }
    }
    extensions.finish()?;
    std::io::copy(&mut archive.into_inner(), &mut std::io::sink())?;
    finish_links(dir, links, options, &mut output_control)
}
