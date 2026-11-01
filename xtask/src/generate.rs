//! Archive builders only: extraction belongs to the installer (#9).
use crate::{
    Result,
    checks::{self, FIXTURE_ROOT, GAME_ROOT},
    fixtures::{self, EntryKind, Expectation, HostExpectation, TreeEntry},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub schema_version: u32,
    pub layouts: BTreeMap<String, Layout>,
    pub fixtures: Vec<Archive>,
    #[serde(default)]
    pub handmade: Vec<Handmade>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub platform: String,
    pub launch_target: String,
    #[serde(default)]
    pub binaries: Vec<String>,
    #[serde(default)]
    pub executable_files: Vec<String>,
    #[serde(default)]
    pub empty_directories: Vec<String>,
    #[serde(default)]
    pub symlinks: Vec<Link>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub id: String,
    pub layout: String,
    pub format: String,
    pub output: String,
    #[serde(default)]
    pub prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handmade {
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub tree: TreeEntry,
    pub data: Vec<u8>,
}

pub fn load_corpus(root: &Path) -> Result<Corpus> {
    let corpus: Corpus = toml::from_str(&fs::read_to_string(
        root.join(FIXTURE_ROOT).join("src/corpus.toml"),
    )?)?;
    if corpus.schema_version != 1 {
        return Err("unsupported corpus schema".into());
    }
    let mut ids = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for archive in &corpus.fixtures {
        fixtures::safe_relative(&archive.id)?;
        fixtures::safe_relative(&archive.output)?;
        if !archive.prefix.is_empty() {
            fixtures::safe_relative(&archive.prefix)?;
        }
        if !ids.insert(&archive.id) || !outputs.insert(&archive.output) {
            return Err("duplicate fixture id/output".into());
        }
        if !corpus.layouts.contains_key(&archive.layout) {
            return Err(format!("unknown layout: {}", archive.layout).into());
        }
    }
    for item in &corpus.handmade {
        if !ids.insert(&item.id) {
            return Err("duplicate handmade fixture id".into());
        }
    }
    Ok(corpus)
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "caskade-fixtures-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{command:?} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn version(command: &mut Command, expected: &str) -> Result<()> {
    let output = command.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() || !text.contains(expected) {
        return Err(format!(
            "builder {command:?} does not match pinned version {expected:?}: {text}"
        )
        .into());
    }
    Ok(())
}

fn sevenzip() -> String {
    std::env::var("CASKADE_7Z").unwrap_or_else(|_| "7z".into())
}

fn build_games(root: &Path, out: &Path) -> Result<()> {
    let targets = if cfg!(target_os = "linux") {
        vec![
            (
                "x86_64-unknown-linux-gnu",
                "caskade-fixture-game",
                "fixture-game-linux-x86_64",
            ),
            (
                "x86_64-pc-windows-gnu",
                "caskade-fixture-game.exe",
                "fixture-game-windows-x86_64.exe",
            ),
        ]
    } else {
        Vec::new()
    };
    for (target, name, destination) in targets {
        command(
            Command::new("cargo")
                .current_dir(root)
                .args(["build", "--locked", "--release", "--manifest-path"])
                .arg(root.join(GAME_ROOT).join("Cargo.toml"))
                .args(["--target", target, "--target-dir"])
                .arg(root.join("target/fixture-game")),
        )?;
        fs::copy(
            root.join("target/fixture-game")
                .join(target)
                .join("release")
                .join(name),
            out.join(destination),
        )?;
    }
    // Other hosts can rebuild archives from the committed Linux/Windows payloads.
    // They do not need cross-linkers to produce the same archive layouts.
    for name in [
        "fixture-game-linux-x86_64",
        "fixture-game-windows-x86_64.exe",
    ] {
        if !out.join(name).is_file() {
            fs::copy(
                root.join(FIXTURE_ROOT).join("bin").join(name),
                out.join(name),
            )?;
        }
    }
    Ok(())
}

pub fn build(root: &Path) -> Result<()> {
    let corpus = load_corpus(root)?;
    let versions = checks::builder_versions(root)?;
    let required = |name: &str| {
        versions
            .get(name)
            .ok_or_else(|| format!("missing pinned builder version: {name}"))
    };
    version(Command::new("rustc").arg("--version"), required("rustc")?)?;
    version(Command::new(sevenzip()).arg("i"), required("sevenzip")?)?;
    version(Command::new("zstd").arg("--version"), required("zstd")?)?;
    if cfg!(target_os = "linux") {
        version(
            Command::new("x86_64-w64-mingw32-gcc").arg("-dumpfullversion"),
            required("mingw")?,
        )?;
    }
    let fixtures = root.join(FIXTURE_ROOT);
    // A rebuild must not silently drop a handmade case, or regenerate its golden
    // tree from today's payload. Its committed expectation is an immutable input.
    for item in &corpus.handmade {
        fixtures::Fixture::load(&fixtures, &item.id)?;
    }
    let scratch = Scratch::new()?;
    let bin = scratch.0.join("bin");
    let expected = scratch.0.join("expected");
    fs::create_dir(&bin)?;
    fs::create_dir(&expected)?;
    build_games(root, &bin)?;
    for archive in &corpus.fixtures {
        let layout = &corpus.layouts[&archive.layout];
        let entries = layout_entries(root, &bin, &archive.layout, layout, &archive.prefix)?;
        let parts = build_archive(&entries, archive, &bin, &scratch.0)?;
        let tree_name = format!("{}.tree.json", archive.id);
        let tree: Vec<_> = entries.iter().map(|e| &e.tree).collect();
        fs::write(
            expected.join(&tree_name),
            format!("{}\n", serde_json::to_string_pretty(&tree)?),
        )?;
        let launch_target = if archive.prefix.is_empty() {
            layout.launch_target.clone()
        } else {
            format!("{}/{}", archive.prefix, layout.launch_target)
        };
        let expectation = Expectation {
            schema_version: 1,
            id: archive.id.clone(),
            title: "Fixture Game".into(),
            layout: archive.layout.clone(),
            format: archive.format.clone(),
            platform: layout.platform.clone(),
            input: parts[0].clone(),
            parts,
            tree: tree_name,
            launch_target,
            host: hosts(&layout.platform),
            windows: None,
        };
        fs::write(
            expected.join(format!("{}.toml", archive.id)),
            toml::to_string_pretty(&expectation)?,
        )?;
    }
    // Check all newly-built file sizes before publishing any output. The aggregate
    // check below includes sources, metadata and handmade data as well.
    for path in checks::files_under(&bin)? {
        if fs::metadata(&path)?.len() > checks::MAX_FIXTURE_BYTES {
            return Err(format!("new fixture exceeds 3 MB: {}", path.display()).into());
        }
    }
    for path in checks::files_under(&bin)? {
        fs::copy(&path, fixtures.join("bin").join(path.file_name().unwrap()))?;
    }
    for path in checks::files_under(&expected)? {
        fs::copy(
            &path,
            fixtures.join("expected").join(path.file_name().unwrap()),
        )?;
    }
    // Preserve immutable handmade outputs and future builder families sharing
    // these directories. Every published file is included in the integrity lock.
    checks::write_lock(root)?;
    checks::check_stale(root)?;
    checks::check_size(root)?;
    Ok(())
}

pub fn hosts(platform: &str) -> BTreeMap<String, HostExpectation> {
    ["windows", "linux", "macos"]
        .into_iter()
        .map(|host| {
            let runtime = match (platform, host) {
                ("windows", "linux") => "ProtonPrefix",
                ("windows", "macos") => "WinePrefix",
                _ => "Native",
            };
            // Cross-platform archives are extracted on all hosts. The payload's
            // platform still determines which launch runtime can run it afterwards.
            (
                host.to_owned(),
                HostExpectation {
                    outcome: "Installed".into(),
                    strategy: "Extract".into(),
                    runtime: runtime.into(),
                },
            )
        })
        .collect()
}

pub fn layout_entries(
    root: &Path,
    binaries: &Path,
    name: &str,
    layout: &Layout,
    prefix: &str,
) -> Result<Vec<Entry>> {
    let mut entries = BTreeMap::new();
    let sources = root.join(FIXTURE_ROOT).join("src/layouts").join(name);
    for path in checks::files_under(&sources)? {
        let relative = path
            .strip_prefix(&sources)?
            .to_str()
            .ok_or("non-UTF8 layout path")?
            .replace('\\', "/");
        let mode = layout.executable_files.contains(&relative);
        add_file(&mut entries, &relative, fs::read(&path)?, mode)?;
    }
    for path in &layout.binaries {
        let source = match layout.platform.as_str() {
            "windows" => "fixture-game-windows-x86_64.exe",
            "linux" => "fixture-game-linux-x86_64",
            other => return Err(format!("no committed game binary for platform {other}").into()),
        };
        add_file(&mut entries, path, fs::read(binaries.join(source))?, true)?;
    }
    for path in &layout.empty_directories {
        add_directory(&mut entries, path)?;
    }
    for link in &layout.symlinks {
        fixtures::safe_relative(&link.path)?;
        add_parents(&mut entries, &link.path)?;
        let entry = Entry {
            tree: TreeEntry {
                path: link.path.clone(),
                kind: EntryKind::Symlink,
                size: 0,
                sha256: None,
                executable: false,
                symlink_target: Some(link.target.clone()),
            },
            data: Vec::new(),
        };
        if entries.insert(link.path.clone(), entry).is_some() {
            return Err("duplicate layout path".into());
        }
    }
    if !prefix.is_empty() {
        let mut prefixed = BTreeMap::new();
        add_directory(&mut prefixed, prefix)?;
        for (_, mut entry) in entries {
            entry.tree.path = format!("{prefix}/{}", entry.tree.path);
            prefixed.insert(entry.tree.path.clone(), entry);
        }
        entries = prefixed;
    }
    Ok(entries.into_values().collect())
}

fn add_parents(entries: &mut BTreeMap<String, Entry>, path: &str) -> Result<()> {
    if let Some((parent, _)) = path.rsplit_once('/') {
        add_directory(entries, parent)?;
    }
    Ok(())
}

fn add_directory(entries: &mut BTreeMap<String, Entry>, path: &str) -> Result<()> {
    fixtures::safe_relative(path)?;
    add_parents(entries, path)?;
    if let Some(entry) = entries.get(path) {
        if entry.tree.kind != EntryKind::Directory {
            return Err(format!("non-directory parent: {path}").into());
        }
    } else {
        entries.insert(
            path.to_owned(),
            Entry {
                tree: TreeEntry {
                    path: path.to_owned(),
                    kind: EntryKind::Directory,
                    size: 0,
                    sha256: None,
                    executable: false,
                    symlink_target: None,
                },
                data: Vec::new(),
            },
        );
    }
    Ok(())
}

fn add_file(
    entries: &mut BTreeMap<String, Entry>,
    path: &str,
    data: Vec<u8>,
    executable: bool,
) -> Result<()> {
    fixtures::safe_relative(path)?;
    add_parents(entries, path)?;
    let entry = Entry {
        tree: TreeEntry {
            path: path.to_owned(),
            kind: EntryKind::File,
            size: data.len() as u64,
            sha256: Some(format!("{:x}", Sha256::digest(&data))),
            executable,
            symlink_target: None,
        },
        data,
    };
    if entries.insert(path.to_owned(), entry).is_some() {
        return Err(format!("duplicate layout path: {path}").into());
    }
    Ok(())
}

pub fn zip_bytes(entries: &[Entry]) -> Result<Vec<u8>> {
    use zip::{ZipWriter, write::SimpleFileOptions};
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries {
        let permissions = if entry.tree.kind == EntryKind::Directory || entry.tree.executable {
            0o755
        } else {
            0o644
        };
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(permissions);
        match entry.tree.kind {
            EntryKind::Directory => zip.add_directory(format!("{}/", entry.tree.path), options)?,
            EntryKind::File => {
                zip.start_file(&entry.tree.path, options)?;
                zip.write_all(&entry.data)?;
            }
            EntryKind::Symlink => {
                zip.add_symlink(
                    &entry.tree.path,
                    entry.tree.symlink_target.as_ref().unwrap(),
                    options,
                )?;
            }
        }
    }
    Ok(zip.finish()?.into_inner())
}

pub fn tar_bytes(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut tar = tar::Builder::new(Vec::new());
    tar.mode(tar::HeaderMode::Deterministic);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_mode(
            if entry.tree.kind == EntryKind::Directory || entry.tree.executable {
                0o755
            } else {
                0o644
            },
        );
        match entry.tree.kind {
            EntryKind::Directory => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
            }
            EntryKind::File => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(entry.data.len() as u64);
            }
            EntryKind::Symlink => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_link_name(entry.tree.symlink_target.as_ref().unwrap())?;
            }
        }
        header.set_cksum();
        tar.append_data(&mut header, &entry.tree.path, &entry.data[..])?;
    }
    Ok(tar.into_inner()?)
}

fn materialize(entries: &[Entry], root: &Path) -> Result<()> {
    for entry in entries {
        let path = root.join(&entry.tree.path);
        match entry.tree.kind {
            EntryKind::Directory => fs::create_dir_all(path)?,
            EntryKind::File => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, &entry.data)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(
                        path,
                        fs::Permissions::from_mode(if entry.tree.executable {
                            0o755
                        } else {
                            0o644
                        }),
                    )?;
                }
            }
            EntryKind::Symlink => {
                return Err("7z builder requires a layout without symlinks".into());
            }
        }
    }
    Ok(())
}

fn build_archive(
    entries: &[Entry],
    archive: &Archive,
    bin: &Path,
    scratch: &Path,
) -> Result<Vec<String>> {
    let output = bin.join(&archive.output);
    match archive.format.as_str() {
        "zip" => fs::write(&output, zip_bytes(entries)?)?,
        "tar" => fs::write(&output, tar_bytes(entries)?)?,
        "tar.gz" => {
            let mut encoder = flate2::GzBuilder::new()
                .mtime(0)
                .write(Vec::new(), flate2::Compression::best());
            encoder.write_all(&tar_bytes(entries)?)?;
            fs::write(&output, encoder.finish()?)?;
        }
        "tar.bz2" | "tar.xz" | "tar.zst" => {
            let input = scratch.join(format!("{}.tar", archive.id));
            fs::write(&input, tar_bytes(entries)?)?;
            if archive.format == "tar.zst" {
                command(
                    Command::new("zstd")
                        .args(["-q", "-f", "-19", "-T1"])
                        .arg(&input)
                        .arg("-o")
                        .arg(&output),
                )?;
            } else {
                let kind = if archive.format == "tar.bz2" {
                    "-tbzip2"
                } else {
                    "-txz"
                };
                command(
                    Command::new(sevenzip())
                        .args(["a", kind, "-y", "-mx=9", "-mmt=1", "-bd", "-bb0"])
                        .arg(&output)
                        .arg(&input),
                )?;
            }
        }
        "7z" | "7z-multivolume" => {
            let input = scratch.join(&archive.id);
            fs::create_dir(&input)?;
            materialize(entries, &input)?;
            let mut builder = Command::new(sevenzip());
            builder.current_dir(&input).args([
                "a", "-t7z", "-y", "-mx=9", "-mmt=1", "-mtc=off", "-mta=off", "-mtm=off", "-bd",
                "-bb0",
            ]);
            if archive.format == "7z-multivolume" {
                builder.arg("-v32k");
            }
            command(builder.arg(&output).arg("."))?;
        }
        format => return Err(format!("unsupported archive builder: {format}").into()),
    }
    if archive.format == "7z-multivolume" {
        let mut parts = Vec::new();
        for path in checks::files_under(bin)? {
            let name = path
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("non-UTF8 volume")?;
            if name.starts_with(&format!("{}.", archive.output)) {
                parts.push(format!("bin/{name}"));
            }
        }
        parts.sort();
        if parts.len() < 2 {
            return Err("multi-volume fixture did not produce multiple volumes".into());
        }
        Ok(parts)
    } else {
        Ok(vec![format!("bin/{}", archive.output)])
    }
}
