use super::metadata::invalid;
use crate::{HostEnv, sources::read};
use std::{
    io,
    path::{Path, PathBuf},
};

/// RGBX, a little-endian path count, then NUL-terminated UTF-16LE paths.
/// The marker's paths are drive-relative; never follow a different drive,
/// parent traversal, UNC/device path, or a link outside the declared root.
pub(super) fn roots(env: &HostEnv, volume: &Path) -> io::Result<Vec<PathBuf>> {
    let bytes = read(env, &volume.join(".GamingRoot"), 16 * 1024)?;
    if bytes.len() < 8 || bytes[..4] != *b"RGBX" || bytes.len() % 2 != 0 {
        return Err(invalid("invalid GamingRoot header"));
    }
    let count = u32::from_le_bytes(bytes[4..8].try_into().expect("header length checked"));
    if count == 0 || count > 64 {
        return Err(invalid("GamingRoot path count limit"));
    }
    let units: Vec<_> = bytes[8..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    let volume = env.resolve_path(volume)?;
    let anchor = volume.canonicalize()?;
    let mut cursor = 0;
    let mut out = Vec::new();
    for _ in 0..count {
        let end = units[cursor..]
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| invalid("unterminated GamingRoot path"))?
            + cursor;
        if end - cursor > 1024 {
            return Err(invalid("GamingRoot path length limit"));
        }
        let relative = String::from_utf16(&units[cursor..end]).map_err(invalid)?;
        cursor = end + 1;
        if relative.starts_with("\\\\") || relative.starts_with("//") {
            return Err(invalid("UNC GamingRoot path is not drive relative"));
        }
        let relative = relative.trim_start_matches(['\\', '/']);
        let path = child(env, &volume, relative)?;
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&anchor) {
            return Err(invalid("GamingRoot escapes drive"));
        }
        if !path.is_dir() {
            return Err(invalid("GamingRoot library is not a directory"));
        }
        out.push(path);
    }
    if units[cursor..].iter().any(|&u| u != 0) {
        return Err(invalid("unexpected GamingRoot trailing data"));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub(super) fn child(env: &HostEnv, base: &Path, relative: &str) -> io::Result<PathBuf> {
    if relative.is_empty()
        || relative.chars().any(char::is_control)
        || relative.contains(':')
        || relative.starts_with(['\\', '/'])
    {
        return Err(invalid("expected a relative package path"));
    }
    let mut path = base.to_path_buf();
    for part in relative.split(['\\', '/']) {
        if part == ".." {
            return Err(invalid("parent traversal in package path"));
        }
        if part.is_empty() || part == "." {
            continue;
        }
        path.push(part);
    }
    let path = env.resolve_path(&path)?;
    // Missing files can still produce a Broken record; check the nearest
    // existing ancestor, which catches links used to escape a package.
    let anchor = base.canonicalize()?;
    let mut ancestor = path.as_path();
    loop {
        match ancestor.canonicalize() {
            Ok(canonical) => {
                if !canonical.starts_with(&anchor) {
                    return Err(invalid("package path escapes its root"));
                }
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| invalid("path has no parent"))?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(path)
}
