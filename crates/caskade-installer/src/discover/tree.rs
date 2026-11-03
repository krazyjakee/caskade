use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use super::DiscoverError;

pub(super) struct Entry {
    pub path: PathBuf,
    pub directory: bool,
    pub size: u64,
}
pub(super) struct Tree {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    paths: BTreeSet<PathBuf>,
    /// Root capability opened once; every read stays confined to it.
    dir: cap_std::fs::Dir,
    /// Last opened parent directory: entries are sorted, so header reads of
    /// siblings reuse one handle instead of resolving each full path.
    parent: std::sync::Mutex<Option<(PathBuf, cap_std::fs::Dir)>>,
}

impl Tree {
    pub fn scan(root: &Path, max_entries: u64) -> Result<Self, DiscoverError> {
        let root = dunce::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::other("discovery root is not a directory").into());
        }
        let directory = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
        let mut entries = vec![];
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            let read_path = if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                &relative
            };
            let mut children = directory
                .read_dir(read_path)?
                .collect::<io::Result<Vec<_>>>()?;
            children.sort_by_key(|entry| entry.file_name());
            for child in children {
                if relative.as_os_str().is_empty() && child.file_name() == ".caskade" {
                    continue;
                }
                let path = relative.join(child.file_name());
                // The entry's own metadata (fstatat on the open directory)
                // avoids resolving the full path again for every child.
                let mut metadata = child.metadata()?;
                if metadata.file_type().is_symlink() {
                    let Ok(target) = dunce::canonicalize(root.join(&path)) else {
                        continue;
                    };
                    if !target.starts_with(&root) {
                        continue;
                    }
                    metadata = directory.metadata(&path)?;
                    if metadata.is_dir() {
                        continue;
                    } // Never recurse through directory links.
                }
                if !metadata.is_file() && !metadata.is_dir() {
                    continue;
                }
                if entries.len() as u64 >= max_entries {
                    return Err(DiscoverError::EntryLimit { limit: max_entries });
                }
                if metadata.is_dir() {
                    pending.push(path.clone());
                }
                entries.push(Entry {
                    path,
                    directory: metadata.is_dir(),
                    size: metadata.len(),
                });
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let paths = entries.iter().map(|entry| entry.path.clone()).collect();
        Ok(Self {
            root,
            entries,
            paths,
            dir: directory,
            parent: std::sync::Mutex::new(None),
        })
    }
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }
    pub fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let text = path.to_string_lossy().replace('\\', "/");
        if text.as_bytes().get(1) == Some(&b':') || text.starts_with("//") || text.contains('\0') {
            return None;
        }
        let path = PathBuf::from(text);
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root).ok()?.to_path_buf()
        } else {
            path
        };
        let mut clean = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => clean.push(part),
                Component::CurDir => {}
                _ => return None,
            }
        }
        // Scanned entries were validated (links resolved inside the root).
        if self.paths.contains(&clean) {
            return Some(clean);
        }
        let target = dunce::canonicalize(self.root.join(&clean)).ok()?;
        if !target.starts_with(&self.root) {
            return None;
        }
        Some(if clean.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            clean
        })
    }
    pub fn dir(&self) -> &cap_std::fs::Dir {
        &self.dir
    }
    /// Open a scanned file for header reads through a cached parent handle.
    pub fn open_entry(&self, path: &Path) -> io::Result<cap_std::fs::File> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return self.dir.open(path);
        };
        if parent.as_os_str().is_empty() {
            return self.dir.open(name);
        }
        let mut cached = self.parent.lock().expect("discovery tree poisoned");
        if cached.as_ref().is_none_or(|(open, _)| open != parent) {
            *cached = Some((parent.to_path_buf(), self.dir.open_dir(parent)?));
        }
        cached
            .as_ref()
            .expect("parent handle cached above")
            .1
            .open(name)
    }
    /// Open a regular file inside the root for bounded random access.
    #[cfg(feature = "linux-formats")]
    pub fn open(&self, path: &Path) -> io::Result<cap_std::fs::File> {
        let relative = self
            .resolve(path)
            .ok_or_else(|| io::Error::other("path is missing or outside the game directory"))?;
        let directory = &self.dir;
        if !directory.metadata(&relative)?.is_file() {
            return Err(io::Error::other("discovery input must be a regular file"));
        }
        directory.open(relative)
    }
    pub fn read(&self, path: &Path, limit: u64) -> io::Result<Vec<u8>> {
        let relative = self.resolve(path).ok_or_else(|| {
            io::Error::other("metadata path is missing or outside the game directory")
        })?;
        let directory = &self.dir;
        if !directory.metadata(&relative)?.is_file() {
            return Err(io::Error::other(
                "discovery metadata must be a regular file",
            ));
        }
        let mut reader = directory.open(relative)?.take(limit.saturating_add(1));
        let mut data = vec![];
        reader.read_to_end(&mut data)?;
        if data.len() as u64 > limit {
            return Err(io::Error::other("metadata exceeds discovery read limit"));
        }
        Ok(data)
    }
}
