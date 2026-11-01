use crate::{
    Result,
    fixtures::{self, sha256_file},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const FIXTURE_ROOT: &str = "crates/caskade-installer/tests/fixtures";
pub const GAME_ROOT: &str = "crates/caskade-installer/tests/fixture-game";
pub const MAX_FIXTURE_BYTES: u64 = 3_000_000;
pub const MAX_TOTAL_BYTES: u64 = 80_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildLock {
    pub schema_version: u32,
    pub source_sha256: String,
    pub builders: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, Output>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub bytes: u64,
    pub sha256: String,
}

pub fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(directory: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_symlink() {
                return Err(format!(
                    "unexpected symlink in fixture infrastructure: {}",
                    path.display()
                )
                .into());
            }
            if metadata.is_dir() {
                visit(&path, result)?;
            } else if metadata.is_file() {
                result.push(path);
            } else {
                return Err(format!("unexpected special file: {}", path.display()).into());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn builder_versions(root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join(FIXTURE_ROOT).join("src/builders.toml");
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

/// Hash source paths and bytes with explicit lengths to avoid concatenation
/// collisions. Includes generator and fixture-game changes as well as src/.
pub fn source_hash(root: &Path) -> Result<String> {
    let mut files = files_under(&root.join(FIXTURE_ROOT).join("src"))?;
    for directory in [
        "xtask/src",
        "xtask/tests",
        &format!("{GAME_ROOT}/src"),
        &format!("{GAME_ROOT}/tests"),
        &format!("{FIXTURE_ROOT}/support"),
    ] {
        files.extend(files_under(&root.join(directory))?);
    }
    for file in [
        "xtask/Cargo.toml",
        &format!("{GAME_ROOT}/Cargo.toml"),
        &format!("{GAME_ROOT}/Cargo.lock"),
    ] {
        files.push(root.join(file));
    }
    for entry in fs::read_dir(root.join("xtask"))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "py" || extension == "rs")
        {
            files.push(path);
        }
    }
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF8 source path")?
            .replace('\\', "/");
        let mut bytes = fs::read(&path)?;
        // Sources are text and .gitattributes pins LF; tolerate autocrlf for the
        // generator itself so the read-only check is portable across clones.
        if bytes.contains(&b'\r') {
            bytes = String::from_utf8(bytes)?.replace("\r\n", "\n").into_bytes();
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn output_inventory(fixtures: &Path) -> Result<BTreeMap<String, Output>> {
    let mut outputs = BTreeMap::new();
    for directory in ["bin", "handmade", "expected"] {
        for path in files_under(&fixtures.join(directory))? {
            let relative = path
                .strip_prefix(fixtures)?
                .to_str()
                .ok_or("non-UTF8 output path")?
                .replace('\\', "/");
            outputs.insert(
                relative,
                Output {
                    bytes: fs::metadata(&path)?.len(),
                    sha256: sha256_file(&path)?,
                },
            );
        }
    }
    Ok(outputs)
}

pub fn write_lock(root: &Path) -> Result<()> {
    let fixtures = root.join(FIXTURE_ROOT);
    let lock = BuildLock {
        schema_version: 1,
        source_sha256: source_hash(root)?,
        builders: builder_versions(root)?,
        outputs: output_inventory(&fixtures)?,
    };
    fs::write(fixtures.join("build.lock"), toml::to_string_pretty(&lock)?)?;
    Ok(())
}

pub fn check_stale(root: &Path) -> Result<()> {
    let fixtures = root.join(FIXTURE_ROOT);
    let lock: BuildLock = toml::from_str(
        &fs::read_to_string(fixtures.join("build.lock"))
            .map_err(|e| format!("missing fixture build.lock: {e}"))?,
    )?;
    if lock.schema_version != 1
        || lock.source_sha256 != source_hash(root)?
        || lock.builders != builder_versions(root)?
    {
        return Err(
            "fixtures are stale, run `cargo xtask fixtures` (source or builder versions changed)"
                .into(),
        );
    }
    let actual = output_inventory(&fixtures)?;
    if lock.outputs != actual {
        let changed: Vec<_> = lock
            .outputs
            .keys()
            .chain(actual.keys())
            .filter(|key| lock.outputs.get(*key) != actual.get(*key))
            .collect();
        return Err(format!(
            "committed fixture outputs missing, changed, or unrecorded: {changed:?}"
        )
        .into());
    }
    fixtures::load_all(&fixtures)?;
    if fixtures.join("expected/burn").exists() {
        fixtures::load_burn(&fixtures)?;
    }
    if fixtures.join("expected/linux-mini").exists() {
        fixtures::load_linux_mini(&fixtures)?;
    }
    if fixtures.join("expected/flatpak").exists() {
        fixtures::load_flatpak(&fixtures)?;
    }
    if fixtures.join("expected/native-uninstall").exists() {
        fixtures::load_native_uninstall(&fixtures)?;
    }
    if fixtures.join("expected/native-windows").exists() {
        fixtures::load_native_windows(&fixtures)?;
    }
    if fixtures.join("expected/disc").exists() {
        fixtures::load_disc(&fixtures)?;
    }
    if fixtures.join("expected/windows").exists() {
        fixtures::load_windows(&fixtures)?;
    }
    if fixtures.join("expected/linux-shell").exists() {
        fixtures::load_linux_shell(&fixtures)?;
    }
    if fixtures.join("expected/appimage").exists() {
        fixtures::load_appimage(&fixtures)?;
    }
    if fixtures.join("expected/macos-native").exists() {
        fixtures::load_macos_native(&fixtures)?;
    }
    if fixtures.join("expected/macos-pkg").exists() {
        fixtures::load_macos_pkg(&fixtures)?;
    }
    println!(
        "fixture sources, declared builder versions and {} committed outputs match build.lock",
        actual.len()
    );
    Ok(())
}

pub fn check_size(root: &Path) -> Result<()> {
    check_size_with_limits(&root.join(FIXTURE_ROOT), MAX_FIXTURE_BYTES, MAX_TOTAL_BYTES)
}

pub fn check_size_with_limits(fixtures: &Path, per_fixture: u64, total_limit: u64) -> Result<()> {
    let mut total = 0_u64;
    for path in files_under(fixtures)? {
        let size = fs::metadata(&path)?.len();
        total = total.checked_add(size).ok_or("fixture size sum overflow")?;
        let relative = path.strip_prefix(fixtures)?;
        if (relative.starts_with("bin") || relative.starts_with("handmade")) && size > per_fixture {
            return Err(format!(
                "fixture {} is {size} bytes; limit is {per_fixture}",
                relative.display()
            )
            .into());
        }
    }
    if total > total_limit {
        return Err(format!("fixture directory is {total} bytes; limit is {total_limit}").into());
    }
    println!("fixture size: {total} / {total_limit} bytes; each binary <= {per_fixture} bytes");
    Ok(())
}
