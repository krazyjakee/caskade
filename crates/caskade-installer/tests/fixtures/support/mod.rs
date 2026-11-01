//! Shared fixture loader: include with `#[path = "fixtures/support/mod.rs"] mod fixtures;`.
//! Requires serde (derive), serde_json, sha2 (0.10), and toml (0.8).
//! No downloads, processes, builders, or installer extraction code live here.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostExpectation {
    pub outcome: String,
    pub strategy: String,
    pub runtime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub layout: String,
    pub format: String,
    pub platform: String,
    /// Path relative to fixtures/, e.g. bin/unity.zip or handmade/unity.rar.
    pub input: String,
    /// Complete ordered volume set, including the first volume.
    pub parts: Vec<String>,
    /// Path relative to expected/.
    pub tree: String,
    pub launch_target: String,
    pub host: BTreeMap<String, HostExpectation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<WindowsExpectation>,
}

/// Builder provenance and effects that static extraction cannot execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsExpectation {
    pub builder: String,
    pub version: String,
    pub features: Vec<String>,
    pub native_generated: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sha256: Option<String>,
    pub executable: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub symlink_target: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub root: PathBuf,
    pub expected: Expectation,
    pub tree: Vec<TreeEntry>,
}

impl Fixture {
    pub fn load(root: &Path, id: &str) -> Result<Self> {
        Self::load_from(root, &root.join("expected"), id)
    }

    fn load_from(root: &Path, expected_dir: &Path, id: &str) -> Result<Self> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("invalid fixture id: {id}").into());
        }
        let path = expected_dir.join(format!("{id}.toml"));
        let expected: Expectation = toml::from_str(
            &fs::read_to_string(&path)
                .map_err(|e| format!("missing fixture expectation {}: {e}", path.display()))?,
        )?;
        if expected.schema_version != 1
            || expected.id != id
            || expected.parts.is_empty()
            || expected.parts.first() != Some(&expected.input)
        {
            return Err(format!("invalid expectation header/volume set: {id}").into());
        }
        safe_relative(&expected.tree)?;
        safe_relative(&expected.launch_target)?;
        for part in &expected.parts {
            safe_relative(part)?;
            if !root.join(part).is_file() {
                return Err(format!("missing committed fixture: {part}").into());
            }
        }
        let tree: Vec<TreeEntry> =
            serde_json::from_slice(&fs::read(root.join("expected").join(&expected.tree))?)?;
        if tree.is_empty() || !tree.windows(2).all(|w| w[0].path < w[1].path) {
            return Err(format!("tree must be nonempty, sorted and unique: {id}").into());
        }
        for entry in &tree {
            safe_relative(&entry.path)?;
            match entry.kind {
                EntryKind::File
                    if entry.sha256.as_ref().is_some_and(|h| {
                        h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
                    }) && entry.symlink_target.is_none() => {}
                EntryKind::Directory
                    if entry.size == 0
                        && entry.sha256.is_none()
                        && entry.symlink_target.is_none() => {}
                EntryKind::Symlink
                    if entry.size == 0
                        && entry.sha256.is_none()
                        && entry.symlink_target.is_some() => {}
                _ => return Err(format!("invalid tree entry in {id}: {}", entry.path).into()),
            }
        }
        if !tree
            .iter()
            .any(|e| e.path == expected.launch_target && e.kind == EntryKind::File)
        {
            return Err(format!("launch target missing from tree: {id}").into());
        }
        Ok(Self {
            root: root.to_owned(),
            expected,
            tree,
        })
    }

    pub fn input(&self) -> PathBuf {
        self.root.join(&self.expected.input)
    }

    pub fn parts(&self) -> Vec<PathBuf> {
        self.expected
            .parts
            .iter()
            .map(|p| self.root.join(p))
            .collect()
    }

    /// Compare the extracted tree, without following symlinks. Execute bits are
    /// compared on Unix; Windows has no Unix permission bits to preserve.
    pub fn assert_tree(&self, extracted: &Path) -> Result<()> {
        let actual = snapshot_tree(extracted)?;
        let expected = self.tree.clone();
        #[cfg(windows)]
        let (actual, expected) = {
            let mut actual = actual;
            let mut expected = expected;
            for entry in actual.iter_mut().chain(expected.iter_mut()) {
                entry.executable = false;
            }
            (actual, expected)
        };
        if actual != expected {
            return Err(format!(
                "fixture {} tree mismatch\nexpected: {}\nactual: {}",
                self.expected.id,
                serde_json::to_string_pretty(&expected)?,
                serde_json::to_string_pretty(&actual)?
            )
            .into());
        }
        Ok(())
    }
}

/// The I0 archive corpus. Windows installer additions deliberately have their
/// own loader so archive-only tests continue to assert their complete corpus.
pub fn load_all(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected"))
}

pub fn load_windows(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/windows"))
}

pub fn load_linux_shell(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/linux-shell"))
}

pub fn load_native_uninstall(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/native-uninstall"))
}

/// T7 Windows acceptance fixtures, separate from the existing extraction corpus.
pub fn load_native_windows(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/native-windows"))
}

/// The seven Linux I2 acceptance minis, reusing canonical payloads/trees.
pub fn load_linux_mini(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/linux-mini"))
}

pub fn load_flatpak(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/flatpak"))
}

pub fn load_disc(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/disc"))
}

pub fn load_burn(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/burn"))
}

pub fn load_packages(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/packages"))
}

pub fn load_macos_native(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/macos-native"))
}

pub fn load_macos_pkg(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/macos-pkg"))
}

pub fn load_appimage(root: &Path) -> Result<Vec<Fixture>> {
    load_directory(root, &root.join("expected/appimage"))
}

fn load_directory(root: &Path, expected: &Path) -> Result<Vec<Fixture>> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(expected)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "toml") {
            ids.push(
                path.file_stem()
                    .unwrap()
                    .to_str()
                    .ok_or("non-UTF8 fixture id")?
                    .to_owned(),
            );
        }
    }
    ids.sort();
    if ids.is_empty() {
        return Err("no committed fixture expectations found".into());
    }
    ids.iter()
        .map(|id| Fixture::load_from(root, expected, id))
        .collect()
}

pub fn safe_relative(path: &str) -> Result<()> {
    // Validate both separator styles on every host; reject Windows drive paths
    // even on Unix. Fixture manifests always use forward slashes.
    if path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path.starts_with('/')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || !Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("unsafe relative fixture path: {path:?}").into());
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn snapshot_tree(root: &Path) -> Result<Vec<TreeEntry>> {
    fn visit(root: &Path, directory: &Path, entries: &mut Vec<TreeEntry>) -> Result<()> {
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF8 tree path")?
                .replace('\\', "/");
            let (kind, size, sha256, symlink_target) = if metadata.is_symlink() {
                (
                    EntryKind::Symlink,
                    0,
                    None,
                    Some(
                        fs::read_link(&path)?
                            .to_str()
                            .ok_or("non-UTF8 link target")?
                            .replace('\\', "/"),
                    ),
                )
            } else if metadata.is_dir() {
                (EntryKind::Directory, 0, None, None)
            } else if metadata.is_file() {
                (
                    EntryKind::File,
                    metadata.len(),
                    Some(sha256_file(&path)?),
                    None,
                )
            } else {
                return Err(io::Error::other(format!(
                    "special file in fixture tree: {}",
                    path.display()
                ))
                .into());
            };
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                kind == EntryKind::File && metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            entries.push(TreeEntry {
                path: relative,
                kind: kind.clone(),
                size,
                sha256,
                executable,
                symlink_target,
            });
            if kind == EntryKind::Directory {
                visit(root, &path, entries)?;
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
