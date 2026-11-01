//! AppleDouble v2 and Apple's ATTR table are decoded as bounded metadata.
//! Layout reference: apple-oss-distributions/copyfile, copyfile.c (AppleDouble
//! and attr_header declarations); all offsets below are absolute file offsets.
use super::{AppError, Result};
use crate::safety::{SafeDir, sanitize_path};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};
fn invalid() -> AppError {
    AppError::Invalid("malformed AppleDouble metadata".into())
}
fn u32be(data: &[u8], offset: usize) -> Result<usize> {
    Ok(u32::from_be_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ) as usize)
}
fn u16be(data: &[u8], offset: usize) -> Result<usize> {
    Ok(u16::from_be_bytes(
        data.get(offset..offset + 2)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    ) as usize)
}
fn span(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    data.get(offset..offset.checked_add(length).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}
pub fn parse(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    if data.len() > 1024 * 1024 {
        return Err(AppError::MetadataLimit);
    }
    if u32be(data, 0)? != 0x00051607 || u32be(data, 4)? != 0x00020000 {
        return Err(invalid());
    }
    let count = u16be(data, 24)?;
    if count > 64 {
        return Err(AppError::MetadataLimit);
    }
    let table_end = 26 + count * 12;
    span(data, 26, count * 12)?;
    let mut attrs = Vec::new();
    let mut ids = BTreeSet::new();
    for index in 0..count {
        let entry = 26 + index * 12;
        let id = u32be(data, entry)?;
        let offset = u32be(data, entry + 4)?;
        let length = u32be(data, entry + 8)?;
        if offset < table_end || !ids.insert(id) {
            return Err(invalid());
        }
        let bytes = span(data, offset, length)?;
        if id == 2 && !bytes.is_empty() {
            attrs.push(("com.apple.ResourceFork".into(), bytes.to_vec()));
        }
        if id == 9 {
            let finder = span(bytes, 0, 32)?;
            if finder.iter().any(|b| *b != 0) {
                attrs.push(("com.apple.FinderInfo".into(), finder.to_vec()));
            }
            if bytes.len() > 34 {
                let header = offset + 34;
                if u32be(data, header)? != 0x41545452 {
                    return Err(invalid());
                }
                let total = u32be(data, header + 8)?;
                let start = u32be(data, header + 12)?;
                let len = u32be(data, header + 16)?;
                span(data, start, len)?;
                if total > data.len() || start < header + 36 {
                    return Err(invalid());
                }
                let count = u16be(data, header + 34)?;
                if count > 1024 {
                    return Err(AppError::MetadataLimit);
                }
                let mut current = header + 36;
                for _ in 0..count {
                    let value_offset = u32be(data, current)?;
                    let value_length = u32be(data, current + 4)?;
                    let name_length = usize::from(*data.get(current + 10).ok_or_else(invalid)?);
                    if !(2..=129).contains(&name_length) {
                        return Err(invalid());
                    }
                    let name = span(data, current + 11, name_length)?;
                    if name.last() != Some(&0) || name[..name.len() - 1].contains(&0) {
                        return Err(invalid());
                    }
                    let name =
                        std::str::from_utf8(&name[..name.len() - 1]).map_err(|_| invalid())?;
                    let value_end = value_offset.checked_add(value_length).ok_or_else(invalid)?;
                    if value_offset < start || value_end > start + len {
                        return Err(invalid());
                    }
                    attrs.push((
                        name.into(),
                        span(data, value_offset, value_length)?.to_vec(),
                    ));
                    current = current
                        .checked_add((11 + name_length + 3) & !3)
                        .ok_or_else(invalid)?;
                    if current > start {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    let mut names = BTreeSet::new();
    if attrs.iter().any(|(name, _)| !names.insert(name)) {
        return Err(invalid());
    }
    Ok(attrs)
}
fn target(path: &Path) -> Result<Option<PathBuf>> {
    let Ok(path) = path.strip_prefix("__MACOSX") else {
        return Ok(None);
    };
    let Some(name) = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("._"))
    else {
        return Ok(None);
    };
    let target = path.parent().unwrap_or(Path::new("")).join(name);
    Ok(Some(sanitize_path(
        target
            .to_str()
            .ok_or(crate::safety::SafetyError::InvalidEncoding)?,
    )?))
}
pub fn restore_zip_attributes(dir: &mut SafeDir) -> Result<()> {
    let files: Vec<_> = dir.extracted_files().map(Path::to_path_buf).collect();
    let mut total = 0_usize;
    for file in files {
        let Some(target) = target(&file)? else {
            continue;
        };
        if !dir.has_entry(&target.to_string_lossy())? {
            return Err(AppError::Invalid("AppleDouble target is absent".into()));
        }
        let mut bytes = Vec::new();
        dir.open_read(&file.to_string_lossy())?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        total = total
            .checked_add(bytes.len())
            .ok_or(AppError::MetadataLimit)?;
        if total > 16 * 1024 * 1024 {
            return Err(AppError::MetadataLimit);
        }
        let attributes = parse(&bytes)?;
        #[cfg(unix)]
        for (name, value) in attributes {
            #[cfg(target_os = "macos")]
            if dir.entry_is_symlink(&target.to_string_lossy())? {
                dir.set_symlink_xattr(
                    &target.to_string_lossy(),
                    std::ffi::OsStr::new(&name),
                    &value,
                )?;
                continue;
            }
            dir.set_entry_xattr(
                &target.to_string_lossy(),
                std::ffi::OsStr::new(&name),
                &value,
            )?;
        }
        #[cfg(not(unix))]
        if !attributes.is_empty() {
            return Err(AppError::Invalid(
                "extended attributes require a compatible filesystem".into(),
            ));
        }
    }
    Ok(())
}
