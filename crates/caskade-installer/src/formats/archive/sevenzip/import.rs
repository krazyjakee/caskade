use super::{Result, SevenZipError};
use crate::{
    formats::archive::{ArchiveOptions, Entry, EntryKind, ExtractionControl},
    safety::{self, Limits, SafeDir},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn walk(
    root: &Path,
    path: &Path,
    limits: Limits,
    entries: &mut Vec<(PathBuf, std::fs::Metadata)>,
) -> Result<()> {
    for item in std::fs::read_dir(path)? {
        let item = item?;
        let path = item.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SevenZipError::Listing("scratch root mismatch".into()))?
            .to_path_buf();
        safety::sanitize_path(
            relative
                .to_str()
                .ok_or(safety::SafetyError::InvalidEncoding)?,
        )?;
        if relative.components().count() > limits.max_path_depth {
            return Err(safety::SafetyError::DepthLimit.into());
        }
        if entries.len() as u64 >= limits.max_entries {
            return Err(safety::SafetyError::EntryLimit.into());
        }
        if !meta.is_file() && !meta.is_dir() && !meta.is_symlink() {
            return Err(safety::SafetyError::SpecialFile.into());
        }
        let directory = meta.is_dir() && !meta.is_symlink();
        entries.push((relative, meta));
        if directory {
            walk(root, &path, limits, entries)?;
        }
    }
    Ok(())
}
pub(super) fn monitor(
    root: &Path,
    limits: Limits,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let mut entries = Vec::new();
    walk(root, root, limits, &mut entries)?;
    let total = entries
        .iter()
        .filter(|(_, m)| m.is_file())
        .try_fold(0_u64, |sum, (_, m)| {
            sum.checked_add(m.len())
                .ok_or(safety::SafetyError::ByteLimit)
        })?;
    if total > limits.max_total_bytes {
        return Err(safety::SafetyError::ByteLimit.into());
    }
    control.checkpoint(64 * 1024)?;
    Ok(())
}

pub(super) fn import(
    root: &Path,
    listed: &[Entry],
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    limits: Limits,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let mut actual = Vec::new();
    walk(root, root, limits, &mut actual)?;
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    let listed: BTreeMap<_, _> = listed
        .iter()
        .map(|entry| Ok((safety::sanitize_path(&entry.name)?, entry)))
        .collect::<std::result::Result<_, safety::SafetyError>>()?;
    let selection = crate::formats::archive::common::Selection::new(options)?;
    let mut links = Vec::new();
    for (path, meta) in &actual {
        control.checkpoint(0)?;
        let name = path.to_str().ok_or(safety::SafetyError::InvalidEncoding)?;
        let expected = listed.get(path);
        // Some formats omit directory records, but never accept an unlisted
        // payload file or link. Names are checked before this comparison.
        if expected.is_none() && !meta.is_dir() {
            return Err(SevenZipError::Listing(format!(
                "unlisted extracted entry: {name}"
            )));
        }
        let Some(destination) = selection.path(name)? else {
            continue;
        };
        if meta.is_symlink() {
            let expected =
                expected.ok_or_else(|| SevenZipError::Listing("unlisted link".into()))?;
            if expected.kind != EntryKind::Symlink {
                return Err(SevenZipError::Listing(
                    "extracted type differs from listing".into(),
                ));
            }
            let target = std::fs::read_link(root.join(path))?;
            let target = target
                .to_str()
                .ok_or(safety::SafetyError::InvalidEncoding)?;
            if expected.target.as_deref() != Some(target) {
                return Err(SevenZipError::Listing(
                    "extracted link differs from listing".into(),
                ));
            }
            links.push((destination, target.to_owned()));
        } else if meta.is_dir() {
            if expected.is_some_and(|entry| entry.kind != EntryKind::Directory) {
                return Err(SevenZipError::Listing(
                    "extracted type differs from listing".into(),
                ));
            }
            dir.create_dir(&destination, options.depth)?;
            control.progress(0, 1)?;
        } else {
            let expected =
                expected.ok_or_else(|| SevenZipError::Listing("unlisted file".into()))?;
            if !matches!(expected.kind, EntryKind::File | EntryKind::Hardlink)
                || meta.len() != expected.size
            {
                return Err(safety::SafetyError::SizeMismatch {
                    declared: expected.size,
                    actual: meta.len(),
                }
                .into());
            }
            let file = std::fs::File::open(root.join(path))?;
            crate::formats::archive::common::write_stream(
                dir,
                &destination,
                file,
                meta.len(),
                crate::formats::archive::common::FileMode {
                    bits: expected.mode,
                    restore_exec: false,
                },
                options,
                control,
            )?;
        }
    }
    let paths: std::collections::BTreeSet<_> = actual.iter().map(|(path, _)| path).collect();
    for (path, entry) in &listed {
        if !paths.contains(path) && entry.kind != EntryKind::Directory {
            return Err(SevenZipError::Listing(format!(
                "listed entry missing after extraction: {}",
                path.display()
            )));
        }
    }
    crate::formats::archive::common::finish_links(dir, links, options, control)?;
    Ok(())
}
