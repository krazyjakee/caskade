use super::{Component, PkgError, Result, invalid};
use crate::{
    formats::archive::ExtractionControl,
    safety::{SafeDir, sanitize_path},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
struct Header {
    name: String,
    size: u64,
    mode: u32,
    nlink: u32,
    device: u64,
    inode: u32,
    newc: bool,
    checksum: Option<u32>,
}
fn number(bytes: &[u8], radix: u32) -> Result<u64> {
    u64::from_str_radix(
        std::str::from_utf8(bytes).map_err(|_| invalid("cpio number encoding"))?,
        radix,
    )
    .map_err(|_| invalid("cpio header number"))
}
fn header(reader: &mut impl Read) -> Result<Header> {
    let mut magic = [0; 6];
    reader.read_exact(&mut magic)?;
    let (length, offset, width, radix) = match &magic {
        b"070707" => (76, 59, 6, 8),
        b"070701" | b"070702" => (110, 94, 8, 16),
        _ => return Err(PkgError::Unsupported("cpio format (only odc/newc)".into())),
    };
    let mut raw = vec![0; length];
    raw[..6].copy_from_slice(&magic);
    reader.read_exact(&mut raw[6..])?;
    let names = number(&raw[offset..offset + width], radix)?;
    if names == 0 || names > 4096 {
        return Err(PkgError::Limit);
    }
    let mut name = vec![0; names as usize];
    reader.read_exact(&mut name)?;
    if name.last() != Some(&0) || name[..name.len() - 1].contains(&0) {
        return Err(invalid("cpio path terminator"));
    }
    std::str::from_utf8(&name[..name.len() - 1]).map_err(|_| invalid("cpio path encoding"))?;
    raw.extend_from_slice(&name);
    if length == 110 {
        let padding = (4 - raw.len() % 4) % 4;
        let start = raw.len();
        raw.resize(start + padding, 0);
        reader.read_exact(&mut raw[start..])?;
        let checksum = (&magic == b"070702")
            .then(|| number(&raw[102..110], 16).map(|n| n as u32))
            .transpose()?;
        let parsed = cpio_archive::newc::NewcHeader::from_reader(&mut Cursor::new(&raw[6..]))
            .map_err(|e| invalid(e.to_string()))?;
        Ok(Header {
            name: parsed.name,
            size: parsed.file_size,
            mode: parsed.mode,
            nlink: parsed.nlink,
            device: (u64::from(parsed.dev_major) << 32) | u64::from(parsed.dev_minor),
            inode: parsed.inode,
            newc: true,
            checksum,
        })
    } else {
        let parsed = cpio_archive::odc::OdcHeader::from_reader(&mut Cursor::new(&raw[6..]))
            .map_err(|e| invalid(e.to_string()))?;
        Ok(Header {
            name: parsed.name,
            size: parsed.file_size,
            mode: parsed.mode,
            nlink: parsed.nlink,
            device: u64::from(parsed.dev),
            inode: parsed.inode,
            newc: false,
            checksum: None,
        })
    }
}
fn game_path(name: &str, component: &Component, directory: bool) -> Result<Option<PathBuf>> {
    if matches!(name, "." | "./") && directory {
        return Ok(None);
    }
    let path = sanitize_path(name)?;
    let path = if component.install_location == "/" {
        if directory && path == Path::new("Applications") {
            return Ok(None);
        }
        path.strip_prefix("Applications")
            .map(Path::to_path_buf)
            .map_err(|_| PkgError::RequiresDelegate("root payload outside Applications".into()))?
    } else if component.install_location == "/Applications" {
        path.strip_prefix("Applications")
            .map(Path::to_path_buf)
            .unwrap_or(path)
    } else if let Some(bundle) = component.install_location.strip_prefix("/Applications/") {
        sanitize_path(bundle)?.join(path)
    } else {
        return Err(PkgError::RequiresDelegate("system install location".into()));
    };
    let Some(first) = path.components().next() else {
        return if directory {
            Ok(None)
        } else {
            Err(invalid("empty payload filename"))
        };
    };
    if !Path::new(first.as_os_str())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    {
        return Err(PkgError::RequiresDelegate(
            "payload is not an app bundle".into(),
        ));
    }
    Ok(Some(path))
}
struct Checked<'a, 'b, R> {
    reader: R,
    control: &'a RefCell<&'b mut dyn ExtractionControl>,
    sum: u32,
    pending: u64,
}
impl<R: Read> Read for Checked<'_, '_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let size = buffer.len().min(64 * 1024);
        self.control
            .borrow_mut()
            .checkpoint(size as u64)
            .map_err(std::io::Error::other)?;
        let n = self.reader.read(&mut buffer[..size])?;
        for byte in &buffer[..n] {
            self.sum = self.sum.wrapping_add(u32::from(*byte));
        }
        self.pending += n as u64;
        if self.pending >= 64 * 1024 {
            self.control
                .borrow_mut()
                .progress(self.pending, 0)
                .map_err(std::io::Error::other)?;
            self.pending = 0;
        }
        Ok(n)
    }
}
type Hardlinks = BTreeMap<(u64, u32), (Option<PathBuf>, Vec<(PathBuf, u32)>)>;
pub(super) fn extract(
    mut reader: impl Read,
    component: &Component,
    dir: &mut SafeDir,
    control: &RefCell<&mut dyn ExtractionControl>,
    max: u64,
) -> Result<()> {
    let mut links = Vec::new();
    let mut directories = Vec::new();
    let mut hardlinks: Hardlinks = BTreeMap::new();
    let mut total = 0_u64;
    let mut entries = 0;
    let mut metadata_bytes = 0_usize;
    loop {
        control.borrow_mut().checkpoint(0)?;
        let header = header(&mut reader)?;
        if header.name == "TRAILER!!!" {
            if header.size != 0 {
                return Err(invalid("cpio trailer size"));
            }
            break;
        }
        metadata_bytes = metadata_bytes
            .checked_add(header.name.len())
            .ok_or(PkgError::Limit)?;
        if metadata_bytes > 16 * 1024 * 1024 {
            return Err(PkgError::Limit);
        }
        if header.size == 0 && header.checksum.is_some_and(|sum| sum != 0) {
            return Err(PkgError::Checksum);
        }
        entries += 1;
        if entries > 100_000 {
            return Err(PkgError::Limit);
        }
        total = total.checked_add(header.size).ok_or(PkgError::Limit)?;
        if total > max {
            return Err(PkgError::Limit);
        }
        let kind = header.mode & 0o170000;
        let directory = kind == 0o040000;
        let path = game_path(&header.name, component, directory)?;
        if directory && header.size != 0 {
            return Err(invalid("cpio directory data"));
        }
        if let Some(path) = path {
            let name = path.to_str().ok_or_else(|| invalid("cpio name encoding"))?;
            match kind {
                0o040000 => {
                    dir.create_dir(name, 0)?;
                    directories.push((path, header.mode));
                }
                0o100000 | 0 => {
                    let mut group = if header.nlink > 1 {
                        Some(hardlinks.entry((header.device, header.inode)).or_default())
                    } else {
                        None
                    };
                    if header.size == 0
                        && let Some(group) = group.as_mut()
                    {
                        group.1.push((path, header.mode));
                    } else {
                        if group.as_ref().is_some_and(|group| group.0.is_some()) {
                            return Err(invalid("hardlink group contains duplicate data"));
                        }
                        let mut data = Checked {
                            reader: (&mut reader).take(header.size),
                            control,
                            sum: 0,
                            pending: 0,
                        };
                        dir.write_file(name, &mut data, Some(header.size), 0, header.mode)?;
                        control.borrow_mut().progress(data.pending, 1)?;
                        if header.checksum.is_some_and(|sum| sum != data.sum) {
                            return Err(PkgError::Checksum);
                        }
                        if let Some(group) = group {
                            group.0 = Some(path);
                        }
                    }
                }
                0o120000 => {
                    if header.size > 4096 {
                        return Err(PkgError::Limit);
                    }
                    let mut target = vec![0; header.size as usize];
                    reader.read_exact(&mut target)?;
                    if header.checksum.is_some_and(|sum| {
                        sum != target
                            .iter()
                            .fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte)))
                    }) {
                        return Err(PkgError::Checksum);
                    }
                    let target =
                        String::from_utf8(target).map_err(|_| invalid("cpio link encoding"))?;
                    links.push((path, target));
                }
                0o010000 => return Err(crate::safety::SafetyError::Fifo.into()),
                0o020000 | 0o060000 => return Err(crate::safety::SafetyError::DeviceNode.into()),
                _ => return Err(crate::safety::SafetyError::SpecialFile.into()),
            }
        }
        if header.newc {
            let padding = (4 - header.size % 4) % 4;
            let mut pad = [0; 3];
            reader.read_exact(&mut pad[..padding as usize])?;
        }
    }
    // Consume the decoder and all trailers even after cpio's terminal record.
    // Extra cpio blocks may contain only zero padding.
    let mut buffer = [0; 64 * 1024];
    let mut padding = 0_u64;
    loop {
        control.borrow_mut().checkpoint(buffer.len() as u64)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        padding += count as u64;
        if padding > 1024 * 1024 || buffer[..count].iter().any(|byte| *byte != 0) {
            return Err(invalid("data after cpio trailer"));
        }
    }
    for (_, (target, mut pending)) in hardlinks {
        let target = if let Some(target) = target {
            target
        } else {
            let (path, mode) = pending
                .pop()
                .ok_or_else(|| invalid("empty hardlink group"))?;
            dir.write_file(&path.to_string_lossy(), std::io::empty(), Some(0), 0, mode)?;
            path
        };
        for (path, _) in pending {
            dir.hardlink(&path.to_string_lossy(), &target.to_string_lossy(), 0)?;
            control.borrow_mut().progress(0, 1)?;
        }
    }
    while !links.is_empty() {
        let mut retry = Vec::new();
        let mut progress = false;
        for (path, target) in links {
            control.borrow_mut().checkpoint(0)?;
            match dir.symlink(&path.to_string_lossy(), &target, 0) {
                Ok(_) => {
                    progress = true;
                    control.borrow_mut().progress(0, 1)?;
                }
                Err(crate::safety::SafetyError::InvalidSymlinkTarget) => retry.push((path, target)),
                Err(error) => return Err(error.into()),
            }
        }
        if !progress && !retry.is_empty() {
            return Err(crate::safety::SafetyError::InvalidSymlinkTarget.into());
        }
        links = retry;
    }
    directories.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in directories {
        dir.set_entry_mode(&path.to_string_lossy(), mode)?;
    }
    Ok(())
}
