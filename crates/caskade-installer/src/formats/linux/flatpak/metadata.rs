use super::{FlatpakError, Result};
use gvariant::{Marker, Structure, aligned_bytes::copy_to_align, gv};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};
const MAX_TEXT: usize = 64 * 1024;
const MAX_MEMBERS: u64 = 10_000;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakMetadata {
    pub id: String,
    pub arch: String,
    pub branch: String,
    pub runtime: Option<String>,
    pub title: Option<String>,
    pub bundle: bool,
    pub application: bool,
}
impl FlatpakMetadata {
    pub(super) fn valid(&self) -> bool {
        valid_id(&self.id) && token(&self.branch) && (self.arch.is_empty() || token(&self.arch))
    }
}
pub fn valid_id(value: &str) -> bool {
    value.len() <= 255
        && value.split('.').count() >= 3
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
                && part.as_bytes()[0].is_ascii_alphabetic()
        })
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'_' | b'-'))
}
pub fn keyfile(text: &str, group: &str) -> Result<BTreeMap<String, String>> {
    if text.len() > MAX_TEXT || text.contains('\0') {
        return Err(FlatpakError::Limit("keyfile"));
    }
    let mut current = "";
    let mut result = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current = section;
            continue;
        }
        if current != group {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| FlatpakError::Invalid("keyfile member".into()))?;
        if result
            .insert(key.trim().to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(FlatpakError::Invalid("duplicate keyfile member".into()));
        }
    }
    Ok(result)
}
pub fn inspect_ref(bytes: &[u8]) -> Result<FlatpakMetadata> {
    if bytes.len() > MAX_TEXT {
        return Err(FlatpakError::Limit("keyfile"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FlatpakError::Invalid("non UTF8 flatpakref".into()))?;
    let values = keyfile(text, "Flatpak Ref")?;
    let id = values
        .get("Name")
        .filter(|id| valid_id(id))
        .ok_or_else(|| FlatpakError::Invalid("Flatpak Ref Name".into()))?
        .clone();
    let branch = values
        .get("Branch")
        .cloned()
        .unwrap_or_else(|| "stable".into());
    if !token(&branch) {
        return Err(FlatpakError::Invalid("Flatpak Ref branch".into()));
    }
    let url = url::Url::parse(
        values
            .get("Url")
            .ok_or_else(|| FlatpakError::Invalid("Flatpak Ref Url".into()))?,
    )
    .map_err(|_| FlatpakError::Invalid("Flatpak Ref Url".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(FlatpakError::Invalid("unsupported Flatpak Ref URL".into()));
    }
    Ok(FlatpakMetadata {
        id,
        arch: String::new(),
        branch,
        runtime: None,
        title: values.get("Title").cloned(),
        bundle: false,
        application: values.get("IsRuntime").is_none_or(|value| value == "false"),
    })
}
fn width(size: u64) -> usize {
    if size <= u8::MAX as u64 {
        1
    } else if size <= u16::MAX as u64 {
        2
    } else if size <= u32::MAX as u64 {
        4
    } else {
        8
    }
}
fn read_at(source: &mut (impl Read + Seek), at: u64, len: usize) -> Result<Vec<u8>> {
    source.seek(SeekFrom::Start(at))?;
    let mut bytes = vec![0; len];
    source.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn offset(bytes: &[u8]) -> u64 {
    let mut out = [0; 8];
    out[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(out)
}
/// Read only the outer tuple's metadata framing and small dictionary members.
/// Inline OSTree delta payloads can be gigabytes and are skipped by offsets.
pub fn inspect_bundle(source: &mut (impl Read + Seek)) -> Result<FlatpakMetadata> {
    let size = source.seek(SeekFrom::End(0))?;
    if size < 32 {
        return Err(FlatpakError::Invalid("truncated bundle".into()));
    }
    let magic = read_at(source, 0, 12)?;
    if &magic[..8] != b"flatpak\0"
        || !matches!(&magic[8..], b"\x01\x00\x89\xe5" | b"\xe5\x89\x00\x01")
    {
        return Err(FlatpakError::Invalid("bundle header".into()));
    }
    // OSTREE_STATIC_DELTA_SUPERBLOCK: first field a{sv}; tuple framing offsets
    // occur in reverse order, so its end is the last little-endian offset.
    let outer_width = width(size);
    let metadata_end = offset(&read_at(source, size - outer_width as u64, outer_width)?);
    if metadata_end < 16 || metadata_end > size - outer_width as u64 {
        return Err(FlatpakError::Invalid("metadata framing".into()));
    }
    let w = width(metadata_end);
    let data_end = offset(&read_at(source, metadata_end - w as u64, w)?);
    let table_len = metadata_end
        .checked_sub(data_end)
        .ok_or_else(|| FlatpakError::Invalid("dictionary offsets".into()))?;
    if table_len == 0 || !table_len.is_multiple_of(w as u64) || table_len / w as u64 > MAX_MEMBERS {
        return Err(FlatpakError::Limit("metadata dictionary"));
    }
    let table = read_at(source, data_end, table_len as usize)?;
    let mut start = 0u64;
    let mut members = BTreeMap::new();
    for item in table.chunks(w) {
        let end = offset(item);
        start = start
            .checked_add(7)
            .ok_or_else(|| FlatpakError::Invalid("member alignment".into()))?
            & !7;
        if end < start || end > data_end {
            return Err(FlatpakError::Invalid("member framing".into()));
        }
        let head = read_at(source, start, (end - start).min(128) as usize)?;
        let key = head.split(|ch| *ch == 0).next().unwrap_or_default();
        if matches!(key, b"ref" | b"metadata") {
            let len = usize::try_from(end - start)
                .map_err(|_| FlatpakError::Limit("dictionary member"))?;
            if len > MAX_TEXT + 256 {
                return Err(FlatpakError::Limit("dictionary member"));
            }
            let bytes = read_at(source, start, len)?;
            let aligned = copy_to_align(&bytes);
            let (key, value) = gv!("{sv}").cast(aligned.as_ref()).to_tuple();
            let value = value
                .get(gv!("s"))
                .ok_or_else(|| FlatpakError::Invalid("dictionary value type".into()))?
                .to_str();
            if members
                .insert(key.to_str().to_owned(), value.to_owned())
                .is_some()
            {
                return Err(FlatpakError::Invalid("duplicate bundle member".into()));
            }
        }
        start = end;
    }
    let reference = members
        .get("ref")
        .ok_or_else(|| FlatpakError::Invalid("missing bundle ref".into()))?;
    let parts: Vec<_> = reference.split('/').collect();
    if parts.len() != 4
        || !matches!(parts[0], "app" | "runtime")
        || !valid_id(parts[1])
        || !token(parts[2])
        || !token(parts[3])
    {
        return Err(FlatpakError::Invalid("bundle ref".into()));
    }
    let application = parts[0] == "app";
    let data = keyfile(
        members
            .get("metadata")
            .ok_or_else(|| FlatpakError::Invalid("missing app metadata".into()))?,
        if application {
            "Application"
        } else {
            "Runtime"
        },
    )?;
    if data.get("name").map(String::as_str) != Some(parts[1]) {
        return Err(FlatpakError::Invalid(
            "bundle id disagrees with metadata".into(),
        ));
    }
    Ok(FlatpakMetadata {
        id: parts[1].into(),
        arch: parts[2].into(),
        branch: parts[3].into(),
        runtime: data.get("runtime").cloned(),
        title: None,
        bundle: true,
        application,
    })
}
