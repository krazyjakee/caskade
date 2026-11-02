use super::{LocationKind, LocationSpec, SaveLocation};
use crate::{Error, Result};
use globset::GlobBuilder;
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;

fn limit() -> Error {
    Error::Invalid(crate::tr!(
        "The save scan exceeded its file or folder limit."
    ))
}

/// Evaluate one component at a time, so a glob under a known game/home folder
/// never starts an unbounded scan of every other folder on the device.
pub fn expand_glob(pattern: &str, case_sensitive: bool) -> Result<Vec<PathBuf>> {
    let pattern = pattern.replace('\\', "/");
    let (root, rest) = if let Some(unc) = pattern.strip_prefix("//") {
        let mut parts = unc.splitn(3, '/');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        if server.is_empty() || share.is_empty() {
            return Err(Error::Invalid(crate::tr!(
                "The network save path has no server or share."
            )));
        }
        (
            PathBuf::from(format!("//{server}/{share}/")),
            parts.next().unwrap_or_default(),
        )
    } else if pattern.starts_with('/') {
        (PathBuf::from("/"), pattern.trim_start_matches('/'))
    } else if pattern.as_bytes().get(1) == Some(&b':') && pattern.as_bytes().get(2) == Some(&b'/') {
        (PathBuf::from(&pattern[..3]), &pattern[3..])
    } else {
        return Err(Error::Invalid(crate::tr!(
            "Save paths must resolve to an absolute location."
        )));
    };
    let parts: Vec<_> = rest.split('/').filter(|part| !part.is_empty()).collect();
    let mut paths = vec![root];
    let mut inspected = 0;
    for (index, part) in parts.iter().enumerate() {
        let mut next = BTreeSet::new();
        if *part == "**" {
            for path in &paths {
                for path in walk(path, index + 1 == parts.len(), &mut inspected)? {
                    next.insert(path);
                }
            }
        } else if *part == "." {
            continue;
        } else if *part == ".." {
            for path in &paths {
                if let Some(parent) = path.parent() {
                    next.insert(parent.to_owned());
                }
            }
        } else {
            let matcher = GlobBuilder::new(part)
                .literal_separator(true)
                .backslash_escape(false)
                .case_insensitive(!case_sensitive)
                .build()
                .map_err(|error| {
                    Error::Invalid(crate::tr!(
                        "Invalid save glob {pattern}: {error}",
                        pattern = pattern,
                        error = error
                    ))
                })?
                .compile_matcher();
            let literal = !part.contains(['*', '?', '[', ']', '{', '}']);
            for dir in &paths {
                if literal && case_sensitive {
                    let path = dir.join(part);
                    if fs::symlink_metadata(&path).is_ok() {
                        next.insert(path);
                    }
                    continue;
                }
                for entry in entries(dir)? {
                    inspected += 1;
                    if inspected > MAX_ENTRIES {
                        return Err(limit());
                    }
                    if matcher.is_match(entry.file_name()) {
                        next.insert(entry.path());
                    }
                }
            }
        }
        paths = next.into_iter().collect();
    }
    Ok(paths)
}

pub fn scan_path(path: &Path, tags: Vec<String>) -> Result<Option<SaveLocation>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let kind = if metadata.is_file() {
        LocationKind::File
    } else if metadata.is_dir() {
        LocationKind::Folder
    } else {
        return Ok(None);
    };
    let files = if kind == LocationKind::File {
        vec![path.to_owned()]
    } else {
        walk(path, true, &mut 0)?
            .into_iter()
            .filter(|path| path.is_file())
            .collect()
    };
    Ok(Some(SaveLocation {
        spec: LocationSpec {
            kind,
            path: super::context::text(path),
        },
        tags,
        files: files
            .iter()
            .map(|path| super::context::text(path))
            .collect(),
        registry: Vec::new(),
    }))
}

fn entries(path: &Path) -> Result<Vec<fs::DirEntry>> {
    match fs::read_dir(path) {
        Ok(entries) => {
            let mut entries = entries.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(fs::DirEntry::file_name);
            Ok(entries)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error.into()),
    }
}

fn walk(root: &Path, include_files: bool, inspected: &mut usize) -> Result<Vec<PathBuf>> {
    let mut pending = VecDeque::from([(root.to_owned(), 0)]);
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    while let Some((path, depth)) = pending.pop_front() {
        if depth > MAX_DEPTH {
            return Err(limit());
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.is_dir() {
            // Wine's Documents folders are often symlinks. Follow them, but
            // never visit the same physical directory twice or follow cycles.
            if !seen.insert(fs::canonicalize(&path)?) {
                continue;
            }
            found.push(path.clone());
            for entry in entries(&path)? {
                *inspected += 1;
                if *inspected > MAX_ENTRIES {
                    return Err(limit());
                }
                pending.push_back((entry.path(), depth + 1));
            }
        } else if metadata.is_file() && include_files {
            found.push(path);
        }
    }
    Ok(found)
}
