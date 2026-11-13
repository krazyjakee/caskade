//! Where a theme's files come from: a folder on disk, or memory for the
//! themes built into the app.
use std::path::{Component, Path, PathBuf};

const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 8;

/// A theme's files by `/`-separated relative path.
pub trait Files: Send + Sync {
    /// Every file, sorted.
    fn list(&self) -> Vec<String>;
    /// A file's bytes; `None` when it doesn't exist or isn't a plain file.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// A theme folder. Symlinks are neither listed nor read, so nothing outside
/// the folder is reachable.
#[derive(Clone, Debug)]
pub struct DirFiles {
    root: PathBuf,
}

impl DirFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The file `path` names, if it is a plain file inside the folder.
    fn locate(&self, path: &str) -> Option<PathBuf> {
        let mut file = self.root.clone();
        for segment in path.split('/') {
            let part = Path::new(segment);
            if segment.is_empty()
                || segment.contains('\\')
                || !matches!(part.components().next(), Some(Component::Normal(_)))
                || part.components().count() != 1
            {
                return None;
            }
            file.push(segment);
            if std::fs::symlink_metadata(&file)
                .ok()?
                .file_type()
                .is_symlink()
            {
                return None;
            }
        }
        std::fs::symlink_metadata(&file)
            .ok()?
            .is_file()
            .then_some(file)
    }

    fn walk(&self, dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if out.len() >= MAX_FILES {
                return;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = format!("{prefix}{name}");
            if kind.is_file() {
                out.push(path);
            } else if kind.is_dir() && depth < MAX_DEPTH {
                self.walk(&entry.path(), &format!("{path}/"), depth + 1, out);
            }
        }
    }
}

impl Files for DirFiles {
    fn list(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&self.root, "", 0, &mut out);
        out.sort();
        out
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.locate(path)?).ok()
    }
}

/// Files held in memory, such as the built-in themes.
#[derive(Clone, Debug, Default)]
pub struct MemFiles {
    files: Vec<(String, Vec<u8>)>,
}

impl MemFiles {
    pub fn new(files: impl IntoIterator<Item = (impl Into<String>, impl Into<Vec<u8>>)>) -> Self {
        let mut files: Vec<(String, Vec<u8>)> = files
            .into_iter()
            .map(|(path, bytes)| (path.into(), bytes.into()))
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Self { files }
    }
}

impl Files for MemFiles {
    fn list(&self) -> Vec<String> {
        self.files.iter().map(|(path, _)| path.clone()).collect()
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files
            .iter()
            .find(|(known, _)| known == path)
            .map(|(_, bytes)| bytes.clone())
    }
}
