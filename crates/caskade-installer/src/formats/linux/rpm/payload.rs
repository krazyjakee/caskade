use super::super::deb::{PackageError, Result, common::Budget};
use crate::{
    ArtifactNode, Format,
    formats::archive::{self, ArchiveOptions, Entry, EntryKind, ExtractionControl, PayloadReader},
    safety::{NameEncoding, SafeDir, SafetyError, decode_name, sanitize_path},
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Seek},
};
type Hardlinks = BTreeMap<(u32, u32, u32), (Option<String>, Vec<String>)>;

fn format(compression: rpm::CompressionType) -> Format {
    match compression {
        rpm::CompressionType::None => Format::Tar,
        rpm::CompressionType::Gzip => Format::Gzip,
        rpm::CompressionType::Xz => Format::Xz,
        rpm::CompressionType::Zstd => Format::Zstd,
        rpm::CompressionType::Bzip2 => Format::Bzip2,
    }
}
struct Checked<'a, R> {
    reader: R,
    control: &'a mut dyn ExtractionControl,
    pending: u64,
    checksum: u32,
    actual: u64,
}
impl<R: Read> Read for Checked<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pending != 0 {
            self.control
                .progress(self.pending, 0)
                .map_err(std::io::Error::other)?;
            self.pending = 0;
        }
        let length = out.len().min(64 * 1024);
        self.control
            .checkpoint(length as u64)
            .map_err(std::io::Error::other)?;
        let count = self.reader.read(&mut out[..length])?;
        self.checksum = out[..count].iter().fold(self.checksum, |sum, byte| {
            sum.wrapping_add(u32::from(*byte))
        });
        self.actual += count as u64;
        self.pending = count as u64;
        Ok(count)
    }
}
fn entries(
    reader: &mut (impl Read + Seek),
    options: &ArchiveOptions,
    mut writer: Option<&mut SafeDir>,
    control: &mut dyn ExtractionControl,
) -> Result<Vec<Entry>> {
    let rpm = super::inspect(reader)?;
    let view = PayloadReader::new(reader, rpm.payload.offset, rpm.payload.length)?;
    let decoder = archive::decoded(view, format(rpm.compression), options)?;
    let unpacked = options.max_unpacked_bytes.min(
        rpm.payload
            .length
            .saturating_mul(options.max_expansion_ratio),
    );
    let mut decoder = Budget::new(
        decoder,
        unpacked
            .saturating_add(options.max_metadata_bytes as u64)
            .saturating_add((options.max_entries as u64).saturating_mul(8192)),
    );
    let mut result = Vec::new();
    let mut total = 0_u64;
    let mut links = Vec::new();
    let mut hard = Hardlinks::new();
    loop {
        control.checkpoint(0)?;
        let mut header = [0; 110];
        decoder.read_exact(&mut header)?;
        if &header[..6] != b"070701" && &header[..6] != b"070702" {
            return Err(PackageError::Invalid("invalid CPIO header".into()));
        }
        let name_size = std::str::from_utf8(&header[94..102])
            .ok()
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or_else(|| PackageError::Invalid("invalid CPIO name size".into()))?;
        if !(1..=4096).contains(&name_size) {
            return Err(PackageError::MetadataLimit);
        }
        let mut entry = cpio::NewcReader::new(Cursor::new(header).chain(&mut decoder))?;
        if entry.entry().is_trailer() {
            let _ = entry.finish()?;
            break;
        }
        if result.len() >= options.max_entries {
            return Err(SafetyError::EntryLimit.into());
        }
        let meta = entry.entry().clone();
        let name = meta.name().to_owned();
        if name == "." || name == "./" {
            let _ = entry.finish()?;
            continue;
        }
        let path = sanitize_path(&name)?;
        let name = path.to_string_lossy().replace('\\', "/");
        let mode = meta.mode();
        let size = u64::from(meta.file_size());
        total = total.checked_add(size).ok_or(SafetyError::ByteLimit)?;
        if total > unpacked {
            return Err(SafetyError::ByteLimit.into());
        }
        let kind = match mode & 0o170000 {
            0o100000 | 0 => EntryKind::File,
            0o040000 => EntryKind::Directory,
            0o120000 => EntryKind::Symlink,
            0o010000 => return Err(SafetyError::Fifo.into()),
            0o020000 | 0o060000 => return Err(SafetyError::DeviceNode.into()),
            _ => return Err(SafetyError::SpecialFile.into()),
        };
        let mut target = None;
        if kind == EntryKind::Symlink {
            if size > options.max_link_bytes as u64 {
                return Err(PackageError::MetadataLimit);
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size {
                return Err(PackageError::Invalid("truncated CPIO symlink".into()));
            }
            target = Some(decode_name(&bytes, NameEncoding::Utf8)?);
            if writer.is_some() {
                links.push((name.clone(), target.clone().unwrap()));
            }
        } else if let Some(dir) = writer.as_mut() {
            match kind {
                EntryKind::Directory => {
                    if size != 0 {
                        return Err(PackageError::Invalid("CPIO directory has payload".into()));
                    }
                    dir.create_dir(&name, options.depth)?;
                    control.progress(0, 1)?;
                }
                EntryKind::File => {
                    let group = (meta.dev_major(), meta.dev_minor(), meta.ino());
                    if meta.nlink() > 1 && size == 0 {
                        hard.entry(group).or_default().1.push(name.clone());
                    } else {
                        control.checkpoint(size)?;
                        let mut checked = Checked {
                            reader: &mut entry,
                            control,
                            pending: 0,
                            checksum: 0,
                            actual: 0,
                        };
                        dir.write_file(&name, &mut checked, Some(size), options.depth, mode)?;
                        if meta
                            .checksum()
                            .is_some_and(|expected| expected != checked.checksum)
                        {
                            return Err(PackageError::Invalid(
                                "CPIO payload checksum mismatch".into(),
                            ));
                        }
                        control.progress(0, 1)?;
                        if meta.nlink() > 1 {
                            hard.entry(group).or_default().0 = Some(name.clone());
                        }
                    }
                }
                _ => unreachable!(),
            }
        } else {
            let mut checked = Checked {
                reader: &mut entry,
                control,
                pending: 0,
                checksum: 0,
                actual: 0,
            };
            std::io::copy(&mut checked, &mut std::io::sink())?;
            if checked.actual != size
                || meta
                    .checksum()
                    .is_some_and(|expected| expected != checked.checksum)
            {
                return Err(PackageError::Invalid(
                    "CPIO payload length or checksum mismatch".into(),
                ));
            }
        }
        result.push(Entry {
            name,
            kind,
            size,
            mode: crate::safety::safe_mode(mode),
            target,
        });
        let _ = entry.finish()?;
    }
    std::io::copy(&mut decoder, &mut std::io::sink())?;
    if let Some(dir) = writer {
        for (_, (target, mut names)) in hard {
            let target = if let Some(target) = target {
                target
            } else {
                let target = names
                    .pop()
                    .ok_or_else(|| PackageError::Invalid("empty CPIO hardlink group".into()))?;
                dir.write_file(&target, &[][..], Some(0), options.depth, 0o644)?;
                target
            };
            for name in names {
                dir.hardlink(&name, &target, options.depth)?;
                control.progress(0, 1)?;
            }
        }
        while !links.is_empty() {
            let count = links.len();
            let mut pending = Vec::new();
            for (name, target) in links {
                match dir.symlink(&name, &target, options.depth) {
                    Ok(_) => control.progress(0, 1)?,
                    Err(SafetyError::InvalidSymlinkTarget) => pending.push((name, target)),
                    Err(error) => return Err(error.into()),
                }
            }
            if pending.len() == count {
                return Err(SafetyError::InvalidSymlinkTarget.into());
            }
            links = pending;
        }
    }
    Ok(result)
}
pub fn list(reader: &mut (impl Read + Seek), options: &ArchiveOptions) -> Result<Vec<Entry>> {
    entries(reader, options, None, &mut archive::NoopControl)
}
pub fn extract(
    reader: &mut (impl Read + Seek),
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<(super::super::deb::PackageInfo, Vec<ArtifactNode>)> {
    let info = super::inspect(reader)?.info;
    entries(reader, options, Some(dir), control)?;
    let engine = crate::probe::ProbeEngine::with_builtin_formats(Default::default());
    let mut nodes = Vec::new();
    for path in dir.extracted_files() {
        let mut file = dir.open_read(
            path.to_str()
                .ok_or_else(|| PackageError::Invalid("payload name encoding".into()))?,
        )?;
        let mut node = engine
            .probe_reader(&mut file, path)
            .map_err(|e| PackageError::Invalid(e.to_string()))?
            .root;
        node.id = crate::ArtifactId(format!("rpm:entry:{}", nodes.len()));
        nodes.push(node);
    }
    Ok((info, nodes))
}
