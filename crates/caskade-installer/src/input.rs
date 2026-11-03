//! Resolve local inputs before probing. Numeric sets are checked for gaps from
//! part one through the highest sibling present; an absent final volume cannot
//! be inferred from filenames alone and must also be checked by the parser.

pub use crate::types::Input;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("URLs must be downloaded before local input resolution")]
    DownloadRequired,
    #[error("input is not a regular file or directory: {0}")]
    UnsupportedFile(PathBuf),
    #[error("input name is not Unicode: {0}")]
    InvalidName(PathBuf),
    #[error("missing {kind:?} parts: {missing:?}")]
    MissingParts {
        kind: SetKind,
        missing: Vec<PathBuf>,
    },
    #[error("duplicate part number {number} in {kind:?} set")]
    DuplicatePart { kind: SetKind, number: u32 },
    #[error("invalid cue sheet: {0}")]
    InvalidCue(PathBuf),
    #[error("folder exceeds the input scan limit")]
    ScanLimit,
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, InputError>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum SetKind {
    Inno,
    Rar,
    SevenZip,
    SplitZip,
    CueBin,
    Discs,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum FolderKind {
    InstallerSet,
    PortableGame,
    Mixed,
    Unknown,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, specta::Type,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedInput {
    File {
        path: PathBuf,
    },
    MultiPart {
        kind: SetKind,
        primary: PathBuf,
        files: Vec<PathBuf>,
    },
    /// A directory remains a single input even when it contains installers.
    Folder {
        path: PathBuf,
        classification: FolderKind,
        files: Vec<PathBuf>,
        installers: Vec<PathBuf>,
    },
}

pub fn resolve(input: &Input) -> Result<ResolvedInput> {
    match input {
        Input::Path(path) => resolve_path(path),
        Input::Urls(_) => Err(InputError::DownloadRequired),
    }
}

pub fn resolve_path(path: &Path) -> Result<ResolvedInput> {
    let path = dunce::canonicalize(path)?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.is_dir() {
        return resolve_folder(&path);
    }
    if !metadata.is_file() {
        return Err(InputError::UnsupportedFile(path));
    }
    let siblings = sibling_files(path.parent().unwrap_or(Path::new(".")))?;
    resolve_file(&path, &siblings)
}

fn filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| InputError::InvalidName(path.into()))
}
fn sibling_files(parent: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
        if paths.len() > 100_000 {
            return Err(InputError::ScanLimit);
        }
    }
    paths.sort();
    Ok(paths)
}

#[derive(Debug, PartialEq, Eq)]
struct Numbered {
    kind: SetKind,
    prefix: String,
    suffix: String,
    number: u32,
    width: usize,
}
fn numbered(name: &str) -> Option<Numbered> {
    let name = name.to_lowercase();
    let (kind, prefix, digits, suffix) = if let Some((base, tail)) = name.rsplit_once(".part") {
        (
            SetKind::Rar,
            format!("{base}.part"),
            tail.strip_suffix(".rar")?,
            ".rar".to_owned(),
        )
    } else if let Some((base, digits)) = name.rsplit_once(".7z.") {
        (
            SetKind::SevenZip,
            format!("{base}.7z."),
            digits,
            String::new(),
        )
    } else if let Some((base, digits)) = name.rsplit_once(".z").filter(|(_, n)| *n != "ip") {
        (
            SetKind::SplitZip,
            format!("{base}.z"),
            digits,
            String::new(),
        )
    } else if let Some(start) = name.rfind("(disc ") {
        let tail = &name[start + 6..];
        let (digits, suffix) = tail.split_once(')')?;
        (
            SetKind::Discs,
            name[..start + 6].to_owned(),
            digits,
            format!("){suffix}"),
        )
    } else {
        let stem = name.strip_suffix(".bin")?;
        let (base, digits) = stem.rsplit_once('-')?;
        (SetKind::Inno, format!("{base}-"), digits, ".bin".to_owned())
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse().ok()?;
    Some(Numbered {
        kind,
        prefix,
        suffix,
        number,
        width: digits.len(),
    })
}
/// Whether `name` is a numbered volume of a multipart set.
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
pub(crate) fn numbered_name(name: &str) -> bool {
    numbered(name).is_some()
}

/// The multipart set a file named `name` belongs to, by name alone (#159):
/// numbered volumes and the `.exe`, `.zip`, `.rar` or `.cue` that leads
/// them share a key; any other name is a set of its own.
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
pub(crate) fn set_key(name: &str) -> String {
    let lower = name.to_lowercase();
    if let Some(pattern) = numbered(&lower) {
        return format!("{}*{}", pattern.prefix, pattern.suffix);
    }
    if let Some(stem) = lower.strip_suffix(".exe") {
        return format!("{stem}-*.bin");
    }
    if let Some(stem) = lower.strip_suffix(".zip") {
        return format!("{stem}.z*");
    }
    // Old-style RAR volumes: game.rar, game.r00, game.r01, …
    if let Some(stem) = lower.strip_suffix(".rar") {
        return format!("{stem}.r*");
    }
    if let Some((stem, extension)) = lower.rsplit_once('.')
        && extension.len() == 3
        && extension.starts_with('r')
        && extension[1..].bytes().all(|b| b.is_ascii_digit())
    {
        return format!("{stem}.r*");
    }
    // A cue sheet and its tracks: game.cue, game.bin, game (Track 2).bin.
    if let Some(stem) = lower
        .strip_suffix(".cue")
        .or_else(|| lower.strip_suffix(".bin"))
    {
        let stem = stem.split(" (track ").next().unwrap_or(stem);
        return format!("{stem}.cue*");
    }
    lower
}

/// Whether a file named `name` can be what an install is about: an
/// installer, archive, package, program, disc image or set volume, not a
/// document or artwork.
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
pub(crate) fn payload_name(name: &str) -> bool {
    let path = Path::new(name);
    let lower = name.to_lowercase();
    installer(path)
        || application(path)
        || numbered(&lower).is_some()
        || [".iso", ".cue", ".chd", ".cso", ".img", ".appimage"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

fn same_set(a: &Numbered, b: &Numbered) -> bool {
    a.kind == b.kind && a.prefix == b.prefix && a.suffix == b.suffix
}

fn resolve_file(path: &Path, siblings: &[PathBuf]) -> Result<ResolvedInput> {
    let name = filename(path)?.to_lowercase();
    let pattern = numbered(&name);
    if let Some(pattern) = &pattern
        && pattern.kind == SetKind::Discs
    {
        return gather(path, pattern, siblings);
    }
    if name.ends_with(".cue") {
        return resolve_cue(path, siblings);
    }
    // Track names such as track-01.bin also match the Inno slice syntax.
    // Explicit cue references take precedence over that filename heuristic.
    if name.ends_with(".bin") {
        for cue in siblings
            .iter()
            .filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("cue")))
        {
            if let Ok(references) = cue_references(cue)
                && references.iter().any(|p| {
                    p == path
                        || p.to_string_lossy()
                            .eq_ignore_ascii_case(&path.to_string_lossy())
                })
            {
                return resolve_file(cue, siblings);
            }
        }
    }
    if let Some(pattern) = pattern {
        return gather(path, &pattern, siblings);
    }
    // Only these primary extensions can own numbered sibling volumes.
    if name.ends_with(".exe") || name.ends_with(".zip") {
        for sibling in siblings {
            if let Some(pattern) = numbered(&filename(sibling)?) {
                let primary = match pattern.kind {
                    SetKind::Inno => format!("{}.exe", pattern.prefix.trim_end_matches('-')),
                    SetKind::SplitZip => format!("{}ip", pattern.prefix),
                    _ => continue,
                };
                if primary == name {
                    return gather(path, &pattern, siblings);
                }
            }
        }
    }
    Ok(ResolvedInput::File { path: path.into() })
}

fn gather(selected: &Path, pattern: &Numbered, siblings: &[PathBuf]) -> Result<ResolvedInput> {
    let parent = selected.parent().unwrap_or(Path::new("."));
    let mut parts = BTreeMap::new();
    for sibling in siblings {
        if let Some(other) = numbered(&filename(sibling)?)
            && same_set(pattern, &other)
            && parts.insert(other.number, sibling.clone()).is_some()
        {
            return Err(InputError::DuplicatePart {
                kind: pattern.kind,
                number: other.number,
            });
        }
    }
    let last = *parts.keys().next_back().unwrap_or(&1);
    // Avoid allocating billions of missing filenames for a hostile suffix.
    if last > 100_000 || parts.contains_key(&0) {
        return Err(InputError::ScanLimit);
    }
    let mut missing = Vec::new();
    for number in 1..=last {
        if !parts.contains_key(&number) {
            missing.push(parent.join(format!(
                "{}{:0width$}{}",
                pattern.prefix,
                number,
                pattern.suffix,
                width = pattern.width
            )));
        }
    }
    let primary_name = match pattern.kind {
        SetKind::Inno => Some(format!("{}.exe", pattern.prefix.trim_end_matches('-'))),
        SetKind::SplitZip => Some(format!("{}ip", pattern.prefix)),
        _ => None,
    };
    let primary = if let Some(name) = primary_name {
        match siblings
            .iter()
            .find(|p| filename(p).is_ok_and(|s| s.eq_ignore_ascii_case(&name)))
        {
            Some(path) => Some(path.clone()),
            None => {
                missing.push(parent.join(name));
                None
            }
        }
    } else {
        parts.get(&1).cloned()
    };
    if !missing.is_empty() {
        return Err(InputError::MissingParts {
            kind: pattern.kind,
            missing,
        });
    }
    let primary = primary.ok_or_else(|| InputError::MissingParts {
        kind: pattern.kind,
        missing: vec![parent.join(format!("{}1{}", pattern.prefix, pattern.suffix))],
    })?;
    let mut files: Vec<_> = parts.into_values().collect();
    if pattern.kind == SetKind::Inno {
        files.insert(0, primary.clone());
    }
    if pattern.kind == SetKind::SplitZip {
        files.push(primary.clone());
    }
    if pattern.kind == SetKind::Discs {
        let discs = files.clone();
        for disc in discs
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue")))
        {
            if let ResolvedInput::MultiPart { files: tracks, .. } = resolve_cue(disc, siblings)? {
                for track in tracks {
                    if !files.contains(&track) {
                        files.push(track);
                    }
                }
            }
        }
    }
    Ok(ResolvedInput::MultiPart {
        kind: pattern.kind,
        primary,
        files,
    })
}

fn cue_references(path: &Path) -> Result<Vec<PathBuf>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(InputError::ScanLimit);
    }
    let content = crate::safety::decode_name(&bytes, crate::safety::NameEncoding::Detect)
        .map_err(|_| InputError::InvalidCue(path.into()))?;
    let parent = path.parent().unwrap_or(Path::new("."));
    Ok(parse_cue_references(&content)
        .map_err(|error| match error {
            InputError::InvalidCue(_) => InputError::InvalidCue(path.into()),
            error => error,
        })?
        .into_iter()
        .map(|p| parent.join(p))
        .collect())
}

/// Parse a bounded CUE sheet without filesystem access. Names are confined
/// relative paths; the caller resolves them against the sheet's directory.
pub fn parse_cue_references(content: &str) -> Result<Vec<PathBuf>> {
    if content.len() > 1024 * 1024 {
        return Err(InputError::ScanLimit);
    }
    let invalid = || InputError::InvalidCue(PathBuf::from("<cue text>"));
    let mut references = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        let Some((keyword, tail)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if !keyword.eq_ignore_ascii_case("FILE") {
            continue;
        }
        let tail = tail.trim_start();
        let name = if let Some(quoted) = tail.strip_prefix('"') {
            quoted.split_once('"').map(|(name, _)| name)
        } else {
            tail.split_whitespace().next()
        }
        .ok_or_else(invalid)?;
        let relative = crate::safety::sanitize_path(name).map_err(|_| invalid())?;
        references.push(relative);
    }
    if references.is_empty() {
        return Err(invalid());
    }
    Ok(references)
}

fn resolve_cue(path: &Path, siblings: &[PathBuf]) -> Result<ResolvedInput> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut files = vec![path.into()];
    let mut missing = Vec::new();
    for target in cue_references(path)? {
        // Cue references may contain subdirectories; canonicalize and confine.
        match dunce::canonicalize(&target) {
            Ok(actual) if actual.starts_with(parent) && actual.is_file() => {
                if !files.contains(&actual) {
                    files.push(actual);
                }
            }
            Ok(_) => return Err(InputError::InvalidCue(path.into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Case-insensitive matching supports cue sheets authored on Windows.
                if let Some(found) = siblings.iter().find(|p| {
                    filename(p).is_ok_and(|s| {
                        s.eq_ignore_ascii_case(&filename(&target).unwrap_or_default())
                    })
                }) {
                    if !files.contains(found) {
                        files.push(found.clone());
                    }
                } else {
                    missing.push(target);
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !missing.is_empty() {
        return Err(InputError::MissingParts {
            kind: SetKind::CueBin,
            missing,
        });
    }
    if files.len() == 1 {
        return Err(InputError::InvalidCue(path.into()));
    }
    Ok(ResolvedInput::MultiPart {
        kind: SetKind::CueBin,
        primary: path.into(),
        files,
    })
}

fn installer(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    [
        ".msi", ".deb", ".rpm", ".pkg", ".dmg", ".zip", ".7z", ".rar", ".tar", ".tar.bz2",
        ".tar.gz", ".tar.xz", ".tar.zst",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || ((name.starts_with("setup") || name.starts_with("install"))
            && [".exe", ".sh"].iter().any(|suffix| name.ends_with(suffix)))
}
fn application(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    [".exe", ".appimage", ".x86_64", ".sh"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        && !installer(path)
}
fn document(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    [".txt", ".md", ".pdf", ".nfo", ".url", ".html"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}
fn resolve_folder(path: &Path) -> Result<ResolvedInput> {
    let mut pending = vec![(path.to_path_buf(), 0)];
    let mut files = Vec::new();
    let mut installers = BTreeSet::new();
    let mut applications = false;
    let mut extra = false;
    let mut visited = 0;
    while let Some((folder, depth)) = pending.pop() {
        if depth > 128 {
            return Err(InputError::ScanLimit);
        }
        let siblings = sibling_files(&folder)?;
        let mut handled = BTreeSet::new();
        for file in &siblings {
            if handled.contains(file) {
                files.push(file.clone());
                continue;
            }
            // Validate every set before returning the folder for probing.
            if let ResolvedInput::MultiPart {
                kind,
                primary,
                files: members,
            } = resolve_file(file, &siblings)?
            {
                handled.extend(members);
                if !matches!(kind, SetKind::CueBin | SetKind::Discs) {
                    installers.insert(primary);
                }
                files.push(file.clone());
                continue;
            } else if installer(file) {
                installers.insert(file.clone());
            }
            applications |= application(file);
            extra |= !installer(file)
                && !application(file)
                && !document(file)
                && numbered(&filename(file)?).is_none();
            files.push(file.clone());
        }
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            visited += 1;
            if visited > 100_000 {
                return Err(InputError::ScanLimit);
            }
            if entry.file_type()?.is_dir() {
                applications |= entry
                    .path()
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("app"));
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    files.sort();
    let classification = if installers.is_empty() {
        if applications {
            FolderKind::PortableGame
        } else {
            FolderKind::Unknown
        }
    } else if applications || extra {
        FolderKind::Mixed
    } else {
        FolderKind::InstallerSet
    };
    Ok(ResolvedInput::Folder {
        path: path.into(),
        classification,
        files,
        installers: installers.into_iter().collect(),
    })
}
