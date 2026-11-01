use super::{Result, SevenZipError};
use crate::formats::archive::{Entry, EntryKind};
use crate::safety::{self, LimitTracker, Limits};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

pub(super) fn parse(
    bytes: &[u8],
    limits: Limits,
    packed: u64,
    case_sensitive: bool,
) -> Result<Vec<Entry>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| SevenZipError::Listing("non-UTF-8 technical listing".into()))?;
    let mut fields = BTreeMap::new();
    let mut entries = Vec::new();
    for line in text.lines().chain(std::iter::once("")) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !fields.is_empty() {
                entries.push(record(&fields)?);
                fields.clear();
            }
        } else {
            let (key, value) = line.split_once(" = ").ok_or_else(|| {
                SevenZipError::Listing(format!("unexpected listing line: {line}"))
            })?;
            if fields.insert(key, value).is_some() {
                return Err(SevenZipError::Listing(format!(
                    "duplicate listing key: {key}"
                )));
            }
        }
    }
    let mut tracker = LimitTracker::new(limits, packed);
    let mut seen = BTreeMap::<String, PathBuf>::new();
    let mut explicit = BTreeSet::new();
    let mut regular = BTreeMap::new();
    let mut links = BTreeSet::new();
    for entry in &mut entries {
        let path = safety::sanitize_path(&entry.name)?;
        if path.components().count() > limits.max_path_depth {
            return Err(safety::SafetyError::DepthLimit.into());
        }
        if !explicit.insert(path.clone()) {
            return Err(safety::SafetyError::Collision(path).into());
        }

        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let normalized: String = prefix.to_string_lossy().nfc().collect();
            let key = if case_sensitive {
                normalized
            } else {
                normalized.to_uppercase().to_lowercase()
            };
            if let Some(previous) = seen.insert(key, prefix.clone())
                && previous != prefix
            {
                return Err(safety::SafetyError::Collision(prefix).into());
            }
        }
        match entry.kind {
            EntryKind::File => {
                regular.insert(path, entry.size);
            }
            EntryKind::Symlink => {
                validate_link(&path, entry.target.as_deref().unwrap_or_default())?;
                links.insert(path);
            }
            EntryKind::Hardlink => {
                let target = safety::sanitize_path(entry.target.as_deref().unwrap_or_default())
                    .map_err(|_| safety::SafetyError::InvalidHardlinkTarget)?;
                if !regular.contains_key(&target) {
                    return Err(safety::SafetyError::InvalidHardlinkTarget.into());
                }
                entry.size = *regular
                    .get(&target)
                    .ok_or(safety::SafetyError::InvalidHardlinkTarget)?;
                regular.insert(path, entry.size);
            }
            EntryKind::Directory => (),
        }
    }
    // External extraction may never write through an archive-created link,
    // even when its final target would be internal. Files are imported later.
    for path in &explicit {
        for parent in path.ancestors().skip(1) {
            if links.contains(parent) {
                return Err(safety::SafetyError::WriteThroughLink(parent.into()).into());
            }
        }
    }
    for entry in &entries {
        tracker.begin_entry(entry.size, 0)?;
    }
    validate_link_graph(&entries, case_sensitive)?;
    Ok(entries)
}
fn record(fields: &BTreeMap<&str, &str>) -> Result<Entry> {
    let name = fields
        .get("Path")
        .ok_or_else(|| SevenZipError::Listing("entry has no path".into()))?
        .to_string();
    safety::sanitize_path(&name)?;
    if fields.get("Encrypted") == Some(&"+") {
        return Err(SevenZipError::Encrypted);
    }
    if fields.get("Alternate Stream") == Some(&"+") {
        return Err(safety::SafetyError::SpecialFile.into());
    }
    let size = fields
        .get("Size")
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| SevenZipError::Listing("entry has no valid size".into()))?;
    let attrs = fields.get("Attributes").copied().unwrap_or_default();
    let mode_text = attrs.split_whitespace().find(|value| {
        value.len() == 10
            && matches!(
                value.as_bytes()[0],
                b'-' | b'd' | b'l' | b'b' | b'c' | b'p' | b's'
            )
    });
    if let Some(mode) = mode_text {
        match mode.as_bytes()[0] {
            b'b' | b'c' => return Err(safety::SafetyError::DeviceNode.into()),
            b'p' => return Err(safety::SafetyError::Fifo.into()),
            b's' => return Err(safety::SafetyError::SpecialFile.into()),
            _ => (),
        }
    }
    let target = fields
        .get("Symbolic Link")
        .filter(|v| !v.is_empty())
        .map(|v| (*v).to_owned());
    let hard = fields
        .get("Hard Link")
        .filter(|v| !v.is_empty())
        .or_else(|| fields.get("Copy Link").filter(|v| !v.is_empty()))
        .map(|v| (*v).to_owned());
    let kind = if target.is_some() {
        EntryKind::Symlink
    } else if hard.is_some() {
        EntryKind::Hardlink
    } else if fields.get("Folder") == Some(&"+")
        || mode_text.is_some_and(|v| v.starts_with('d'))
        || attrs.starts_with('D')
    {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    let mode = mode_text
        .map(|v| {
            let mut mode = 0;
            for (i, b) in v.as_bytes()[1..].iter().enumerate() {
                if matches!(*b, b'r' | b'w' | b'x' | b's' | b't') {
                    mode |= 1 << (8 - i);
                }
            }
            mode
        })
        .unwrap_or(if kind == EntryKind::Directory {
            0o755
        } else {
            0o644
        });
    Ok(Entry {
        name,
        kind,
        size,
        mode,
        target: target.or(hard),
    })
}
fn validate_link(path: &Path, target: &str) -> Result<()> {
    if target.is_empty() || target.contains(['\0', ':']) || target.starts_with(['/', '\\']) {
        return Err(safety::SafetyError::InvalidSymlinkTarget.into());
    }
    let mut depth = path.parent().map(|p| p.components().count()).unwrap_or(0);
    for part in target.split(['/', '\\']) {
        match part {
            "" | "." => (),
            ".." => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(safety::SafetyError::InvalidSymlinkTarget)?;
            }
            _ => {
                safety::sanitize_path(part)?;
                depth += 1;
            }
        }
    }
    Ok(())
}

fn validate_link_graph(entries: &[Entry], case_sensitive: bool) -> Result<()> {
    use std::collections::VecDeque;
    let key = |path: &Path| {
        let normalized: String = path.to_string_lossy().nfc().collect();
        if case_sensitive {
            normalized
        } else {
            normalized.to_uppercase().to_lowercase()
        }
    };
    let paths: BTreeMap<_, _> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Symlink)
        .map(|entry| {
            Ok((
                safety::sanitize_path(&entry.name)?,
                entry.target.as_deref().unwrap_or_default(),
            ))
        })
        .collect::<std::result::Result<_, safety::SafetyError>>()?;
    let links: BTreeMap<_, _> = paths
        .iter()
        .map(|(path, target)| (key(path), *target))
        .collect();
    for (path, target) in &paths {
        let mut queue: VecDeque<String> = path
            .parent()
            .into_iter()
            .flat_map(|p| p.components())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .chain(target.split(['/', '\\']).map(str::to_owned))
            .collect();
        let mut resolved = PathBuf::new();
        let mut expansions = 0;
        while let Some(part) = queue.pop_front() {
            match part.as_str() {
                "" | "." => continue,
                ".." => {
                    if !resolved.pop() {
                        return Err(safety::SafetyError::InvalidSymlinkTarget.into());
                    }
                    continue;
                }
                _ => resolved.push(part),
            }
            if let Some(target) = links.get(&key(&resolved)) {
                expansions += 1;
                if expansions > 40 {
                    return Err(safety::SafetyError::InvalidSymlinkTarget.into());
                }
                resolved.pop();
                for part in target.split(['/', '\\']).rev() {
                    queue.push_front(part.into());
                }
            }
        }
    }
    Ok(())
}
