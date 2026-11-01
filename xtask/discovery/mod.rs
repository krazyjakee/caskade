//! Discovery machine fixtures have a separate lock and never inspect live data.
mod privacy;
#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const FIXTURES: &str = "crates/caskade-discovery/tests/fixtures";
pub const MAX_FILE: u64 = 2_000_000;
pub const MAX_TOTAL: u64 = 16_000_000;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    size: u64,
    sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    schema_version: u32,
    source_sha256: String,
    outputs: BTreeMap<String, FileRecord>,
}

pub fn run(mut args: impl Iterator<Item = String>, workspace: &Path) -> Result<()> {
    let command = args.next().unwrap_or_else(|| "check".into());
    let mut root = workspace.to_owned();
    while let Some(argument) = args.next() {
        if argument != "--root" {
            return Err(format!("unknown discovery argument: {argument}").into());
        }
        root = args.next().ok_or("--root needs a workspace path")?.into();
    }
    match command.as_str() {
        "build" => {
            let status = std::process::Command::new(
                std::env::var("PYTHON").unwrap_or_else(|_| "python3".into()),
            )
            .arg(root.join("xtask/discovery/generate.py"))
            .arg("--root")
            .arg(&root)
            .status()?;
            if !status.success() {
                return Err("discovery fixture generator failed".into());
            }
            write_lock(&root)?;
            check(&root)?;
        }
        "record-build" => {
            write_lock(&root)?;
            check(&root)?;
        }
        "check" => check(&root)?,
        "check-stale" => check_stale(&root)?,
        "check-size" => check_size(&root)?,
        "check-privacy" => check_privacy(&root)?,
        _ => return Err(format!("unknown discovery command: {command}").into()),
    }
    println!("discovery fixtures {command}: ok");
    Ok(())
}

fn files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut found = vec![];
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| name == "__pycache__" || name == ".gitignore")
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_symlink() {
            return Err(format!("fixture symlink is forbidden: {}", path.display()).into());
        }
        if metadata.is_dir() {
            found.extend(files(&path)?);
        } else if metadata.is_file() {
            found.push(path);
        } else {
            return Err(format!("fixture special file is forbidden: {}", path.display()).into());
        }
    }
    found.sort();
    Ok(found)
}

fn hash_sources(root: &Path) -> Result<String> {
    let mut paths = vec![];
    for directory in [
        root.join(FIXTURES).join("src"),
        root.join(FIXTURES).join("support"),
        root.join("xtask/discovery"),
    ] {
        paths.extend(files(&directory)?);
    }
    paths.sort();
    let mut hash = Sha256::new();
    for path in paths {
        let name = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(path)?;
        let normalized = match std::str::from_utf8(&bytes) {
            Ok(text) => text.replace("\r\n", "\n").into_bytes(),
            Err(_) => bytes,
        };
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((normalized.len() as u64).to_le_bytes());
        hash.update(&normalized);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn outputs(root: &Path) -> Result<BTreeMap<String, FileRecord>> {
    let fixtures = root.join(FIXTURES);
    let mut result = BTreeMap::new();
    for directory in ["machines", "formats"] {
        for path in files(&fixtures.join(directory))? {
            let size = path.metadata()?.len();
            if size > MAX_FILE {
                return Err(format!(
                    "discovery fixture exceeds {MAX_FILE} bytes: {}",
                    path.display()
                )
                .into());
            }
            result.insert(
                path.strip_prefix(&fixtures)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                FileRecord {
                    size,
                    sha256: format!("{:x}", Sha256::digest(fs::read(path)?)),
                },
            );
        }
    }
    if result.is_empty() {
        return Err("discovery fixture corpus is empty".into());
    }
    Ok(result)
}

pub fn write_lock(root: &Path) -> Result<()> {
    check_size(root)?;
    check_privacy(root)?;
    let lock = Lock {
        schema_version: 1,
        source_sha256: hash_sources(root)?,
        outputs: outputs(root)?,
    };
    fs::write(
        root.join(FIXTURES).join("build.lock"),
        serde_json::to_string_pretty(&lock)? + "\n",
    )?;
    Ok(())
}
pub fn check_stale(root: &Path) -> Result<()> {
    let lock: Lock = serde_json::from_slice(&fs::read(root.join(FIXTURES).join("build.lock"))?)?;
    if lock.schema_version != 1
        || lock.source_sha256 != hash_sources(root)?
        || lock.outputs != outputs(root)?
    {
        return Err("discovery fixtures are stale; rebuild from committed sources".into());
    }
    Ok(())
}
pub fn check_size(root: &Path) -> Result<()> {
    let mut total = 0u64;
    for path in files(&root.join(FIXTURES))? {
        let size = path.metadata()?.len();
        if size > MAX_FILE {
            return Err(format!(
                "discovery fixture exceeds per-file size limit: {}",
                path.display()
            )
            .into());
        }
        total = total.checked_add(size).ok_or("fixture size overflow")?;
    }
    if total > MAX_TOTAL {
        return Err(
            format!("discovery fixtures exceed total size limit: {total}/{MAX_TOTAL}").into(),
        );
    }
    Ok(())
}
pub fn check_privacy(root: &Path) -> Result<()> {
    let fixtures = root.join(FIXTURES);
    for directory in ["src", "machines", "formats"] {
        for path in files(&fixtures.join(directory))? {
            if path.metadata()?.len() > MAX_FILE {
                return Err("privacy input exceeds fixture size limit".into());
            }
            let name = path
                .strip_prefix(&fixtures)?
                .to_string_lossy()
                .replace('\\', "/");
            privacy::check_bytes(&path, name.as_bytes())?;
            privacy::check_bytes(&path, &fs::read(&path)?)?;
        }
    }
    Ok(())
}
pub fn check(root: &Path) -> Result<()> {
    check_size(root)?;
    check_privacy(root)?;
    check_stale(root)
}
