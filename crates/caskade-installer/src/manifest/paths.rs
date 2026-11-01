use super::{ManifestError, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// A nonempty, portable, game-relative file path. Both separators are accepted
/// on input; serialization always uses `/`, even when running on Windows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManifestPath(String);

impl ManifestPath {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let mut path = PathBuf::from(value.as_ref());
        normalize_path(&mut path, false)?;
        Ok(Self(portable(&path)?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}
impl AsRef<Path> for ManifestPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl fmt::Display for ManifestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Serialize for ManifestPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ManifestPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Validated lowercase hexadecimal SHA-256. Algorithm and representation are
/// unambiguous; malformed hashes cannot silently turn verification off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);
impl Sha256Digest {
    pub fn new(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref();
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ManifestError::Invalid(
                "SHA-256 must contain 64 hexadecimal digits".into(),
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

pub(super) fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| ManifestError::Invalid("path must be Unicode".into()))
}
pub(super) fn portable(path: &Path) -> Result<String> {
    Ok(path_text(path)?.replace('\\', "/"))
}

pub(super) fn normalize_path(path: &mut PathBuf, allow_root: bool) -> Result<()> {
    let value = path_text(path)?;
    if allow_root
        && !value.starts_with(['/', '\\'])
        && value.split(['/', '\\']).all(|p| p.is_empty() || p == ".")
    {
        *path = ".".into();
        return Ok(());
    }
    let normalized = crate::safety::sanitize_path(value)
        .map_err(|e| ManifestError::Invalid(format!("relative path {value:?}: {e}")))?;
    // Keep `/` in PathBuf itself. PathBuf's serializer otherwise uses `\\` on
    // Windows for paths assembled by the sanitizer's component pushes.
    *path = portable(&normalized)?.into();
    Ok(())
}

/// Symlink targets may use `..` while remaining inside the game tree. The target
/// is interpreted relative to the link's parent, independently of the host OS.
pub(super) fn normalize_link(path: &Path, target: &mut PathBuf) -> Result<()> {
    let value = portable(target)?;
    if value.is_empty() || value.starts_with('/') || value.contains([':', '\0']) {
        return Err(ManifestError::Invalid(
            "symlink target must be relative".into(),
        ));
    }
    let mut depth = portable(path)?.split('/').count() - 1;
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    ManifestError::Invalid("symlink target escapes game directory".into())
                })?;
            }
            component => {
                crate::safety::sanitize_path(component)
                    .map_err(|e| ManifestError::Invalid(format!("symlink target: {e}")))?;
                depth += 1;
            }
        }
    }
    *target = value.into();
    Ok(())
}
