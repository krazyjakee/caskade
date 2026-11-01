//! Synthetic machine loader. No live path, registry or environment fallback.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub schema_version: u32,
    pub os: String,
    #[serde(default = "default_time")]
    pub now_unix_secs: u64,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub known_folders: BTreeMap<String, String>,
    #[serde(default)]
    pub volumes: Vec<Volume>,
}
fn default_time() -> u64 {
    1_700_000_000
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Volume {
    pub path: String,
    pub kind: String,
    pub case_sensitive: bool,
}
pub struct Scenario {
    pub path: PathBuf,
    pub machine: Machine,
}

/// Writes a file below `root`, creating its parent folders.
pub fn write(root: &Path, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn component(name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || matches!(name, "." | "..") {
        return Err(invalid("scenario id must be one ordinary path component"));
    }
    Ok(())
}
impl Scenario {
    pub fn load(fixtures: &Path, id: &str) -> io::Result<Self> {
        component(id)?;
        let base = fixtures.join("machines").canonicalize()?;
        let path = base.join(id);
        if fs::symlink_metadata(&path)?.is_symlink() {
            return Err(invalid("scenario cannot be a symlink"));
        }
        let path = path.canonicalize()?;
        if path.parent() != Some(base.as_path()) {
            return Err(invalid("scenario escapes machine corpus"));
        }
        let machine: Machine = toml::from_str(&fs::read_to_string(path.join("machine.toml"))?)
            .map_err(|e| invalid(e.to_string()))?;
        if machine.schema_version != 1
            || !matches!(machine.os.as_str(), "linux" | "windows" | "macos")
        {
            return Err(invalid("unsupported machine schema or OS"));
        }
        Ok(Self { path, machine })
    }
    pub fn root(&self) -> PathBuf {
        self.path.join("root")
    }
    pub fn file(&self, relative: impl AsRef<Path>) -> io::Result<PathBuf> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid("fixture path must be relative"));
        }
        let candidate = self.path.join(relative).canonicalize()?;
        if !candidate.starts_with(&self.path) {
            return Err(invalid("fixture path escapes scenario"));
        }
        Ok(candidate)
    }
    /// Materialize only this synthetic scenario; never copy a repository/live tree.
    pub fn materialize(&self, destination: &Path) -> io::Result<()> {
        if destination.exists() {
            return Err(invalid("materialization destination already exists"));
        }
        copy_tree(&self.path, destination)
    }
}
fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    let mut entries: Vec<_> = fs::read_dir(source)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source = entry.path();
        let destination = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.is_symlink() {
            return Err(invalid("fixture materialization refuses symlinks"));
        }
        if metadata.is_dir() {
            copy_tree(&source, &destination)?;
        } else if metadata.is_file() {
            fs::copy(source, destination)?;
        } else {
            return Err(invalid("fixture materialization refuses special files"));
        }
    }
    Ok(())
}
pub fn all(fixtures: &Path) -> io::Result<Vec<Scenario>> {
    let mut names = vec![];
    for entry in fs::read_dir(fixtures.join("machines"))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(
                entry
                    .file_name()
                    .to_str()
                    .ok_or_else(|| invalid("non-UTF8 scenario"))?
                    .to_owned(),
            );
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| Scenario::load(fixtures, &name))
        .collect()
}

/// Replace fixture-root prefixes in values and object keys, preserving paths' tails.
/// Durations/clock values remain visible; the scenario supplies a fixed clock.
pub fn relative_value(value: &impl Serialize, root: &Path) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(value)?;
    let mut prefixes = vec![root.to_string_lossy().into_owned()];
    if let Ok(canonical) = root.canonicalize() {
        prefixes.push(canonical.to_string_lossy().into_owned());
    }
    prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    prefixes.dedup();
    fn text(value: &str, prefixes: &[String]) -> String {
        let mut result = value.to_owned();
        let mut changed = false;
        for prefix in prefixes {
            for prefix in [
                prefix.clone(),
                prefix.replace('\\', "/"),
                prefix.replace('\\', "\\\\"),
            ] {
                let mut output = String::new();
                let mut start = 0;
                for (offset, _) in result.match_indices(&prefix) {
                    let end = offset + prefix.len();
                    if result[end..].chars().next().is_some_and(|c| {
                        !matches!(c, '/' | '\\' | '"' | '\'') && !c.is_whitespace()
                    }) {
                        continue;
                    }
                    output.push_str(&result[start..offset]);
                    output.push_str("$FIXTURE");
                    start = end;
                    changed = true;
                }
                output.push_str(&result[start..]);
                result = output;
            }
        }
        if changed {
            result = result.replace("\\\\", "/").replace('\\', "/");
        }
        result
    }
    fn visit(value: &mut Value, prefixes: &[String]) {
        match value {
            Value::String(string) => *string = text(string, prefixes),
            Value::Array(values) => {
                for value in values {
                    visit(value, prefixes);
                }
            }
            Value::Object(object) => {
                let old = std::mem::take(object);
                for (key, mut value) in old {
                    visit(&mut value, prefixes);
                    object.insert(text(&key, prefixes), value);
                }
            }
            _ => {}
        }
    }
    visit(&mut value, &prefixes);
    Ok(value)
}
pub fn assert_snapshot(name: &str, value: &impl Serialize, root: &Path) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"));
    let _guard = settings.bind_to_scope();
    insta::assert_json_snapshot!(
        name,
        relative_value(value, root).expect("serializable snapshot")
    );
}
