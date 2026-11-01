//! Confined extraction. Give each job a private staging directory and discard it
//! on any error. Never expose its writable capability to a format parser.
//!
//! Symlink targets must already exist (handlers may defer links until files are
//! extracted). Writes through symlinks and overwriting entries are forbidden.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    #[error("entry name is empty")]
    EmptyName,
    #[error("entry contains a NUL byte")]
    Nul,
    #[error("absolute entry path")]
    AbsolutePath,
    #[error("drive-qualified entry path")]
    DrivePrefix,
    #[error("UNC or Windows device path")]
    UncPath,
    #[error("parent traversal in entry path")]
    ParentTraversal,
    #[error("entry path is inside Caskade's own .caskade folder")]
    Reserved,
    #[error("reserved Windows name: {0}")]
    ReservedName(String),
    #[error("trailing dot or space in path component")]
    TrailingDotOrSpace,
    #[error("invalid Windows character in entry name")]
    InvalidCharacter,
    #[error("name cannot be decoded without replacement")]
    InvalidEncoding,
    #[error("duplicate or colliding entry: {0}")]
    Collision(PathBuf),
    #[error("write through a symbolic link: {0}")]
    WriteThroughLink(PathBuf),
    #[error("symbolic link target escapes staging or does not exist")]
    InvalidSymlinkTarget,
    #[error("hardlink target is not an already extracted regular file")]
    InvalidHardlinkTarget,
    #[error("device nodes are forbidden")]
    DeviceNode,
    #[error("FIFOs are forbidden")]
    Fifo,
    #[error("special entry type is forbidden")]
    SpecialFile,
    #[error("privilege-bearing extended attribute is forbidden")]
    UnsafeAttribute,
    #[error("entry count exceeds limit")]
    EntryLimit,
    #[error("nesting depth exceeds limit")]
    DepthLimit,
    #[error("expanded bytes exceed limit")]
    ByteLimit,
    #[error("expansion ratio exceeds limit")]
    ExpansionRatio,
    #[error("declared size {declared} differs from actual size {actual}")]
    SizeMismatch { declared: u64, actual: u64 },
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SafetyError>;

/// Treat both separator styles identically on every host. Never rewrite a
/// dangerous component into another valid entry, which could hide collisions.
pub fn sanitize_path(name: &str) -> Result<PathBuf> {
    if name.contains('\0') {
        return Err(SafetyError::Nul);
    }
    if name.starts_with("\\\\") || name.starts_with("//") {
        return Err(SafetyError::UncPath);
    }
    if name.as_bytes().get(1) == Some(&b':') && name.as_bytes()[0].is_ascii_alphabetic() {
        return Err(SafetyError::DrivePrefix);
    }
    if name.starts_with(['/', '\\']) {
        return Err(SafetyError::AbsolutePath);
    }
    let mut result = PathBuf::new();
    for part in name.split(['/', '\\']) {
        if part == ".." {
            return Err(SafetyError::ParentTraversal);
        }
        if part.is_empty() || part == "." {
            continue;
        }
        if part.ends_with(['.', ' ']) {
            return Err(SafetyError::TrailingDotOrSpace);
        }
        if part
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        {
            return Err(SafetyError::InvalidCharacter);
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches(' ')
            .to_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if reserved {
            return Err(SafetyError::ReservedName(part.into()));
        }
        result.push(part);
    }
    if result.as_os_str().is_empty() {
        return Err(SafetyError::EmptyName);
    }
    Ok(result)
}

/// CP437 is an archive default, not detectable reliably from a short filename.
/// Prefer a format's declared encoding; use detection only without a declaration.
#[derive(Debug, Clone, Copy)]
pub enum NameEncoding {
    Utf8,
    Cp437,
    Label(&'static [u8]),
    Detect,
}

pub fn decode_name(bytes: &[u8], encoding: NameEncoding) -> Result<String> {
    use codepage_437::{CP437_CONTROL, FromCp437};
    if bytes.contains(&0) {
        return Err(SafetyError::Nul);
    }
    let encoding = match encoding {
        NameEncoding::Utf8 => {
            return std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| SafetyError::InvalidEncoding);
        }
        NameEncoding::Cp437 => return Ok(String::from_cp437(bytes.to_vec(), &CP437_CONTROL)),
        NameEncoding::Label(label) => {
            encoding_rs::Encoding::for_label(label).ok_or(SafetyError::InvalidEncoding)?
        }
        NameEncoding::Detect => {
            if let Ok(name) = std::str::from_utf8(bytes) {
                return Ok(name.into());
            }
            let mut detector = chardetng::EncodingDetector::new();
            detector.feed(bytes, true);
            detector.guess(None, true)
        }
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|name| name.into_owned())
        .ok_or(SafetyError::InvalidEncoding)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, specta::Type)]
pub struct Limits {
    pub max_expansion_ratio: u64,
    pub max_total_bytes: u64,
    pub max_entries: u64,
    /// Container nesting depth; zero is the outermost container.
    pub max_depth: usize,
    pub max_path_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_expansion_ratio: 1000,
            max_total_bytes: 100 * 1024 * 1024 * 1024,
            max_entries: 1_000_000,
            max_depth: 16,
            max_path_depth: 128,
        }
    }
}

/// One budget for the entire extraction tree, including nested archives.
/// `packed_bytes` is the actual source length, never an entry's untrusted claim.
#[derive(Debug)]
pub struct LimitTracker {
    limits: Limits,
    packed_bytes: u64,
    entries: u64,
    declared: u64,
    actual: u64,
}
impl LimitTracker {
    pub fn new(limits: Limits, packed_bytes: u64) -> Self {
        Self {
            limits,
            packed_bytes,
            entries: 0,
            declared: 0,
            actual: 0,
        }
    }
    fn check_bytes(&self, bytes: u64) -> Result<()> {
        if bytes > self.limits.max_total_bytes {
            return Err(SafetyError::ByteLimit);
        }
        if u128::from(bytes)
            > u128::from(self.packed_bytes) * u128::from(self.limits.max_expansion_ratio)
        {
            return Err(SafetyError::ExpansionRatio);
        }
        Ok(())
    }
    pub fn begin_entry(&mut self, declared: u64, depth: usize) -> Result<()> {
        if depth > self.limits.max_depth {
            return Err(SafetyError::DepthLimit);
        }
        if self.entries >= self.limits.max_entries {
            return Err(SafetyError::EntryLimit);
        }
        let total = self
            .declared
            .checked_add(declared)
            .ok_or(SafetyError::ByteLimit)?;
        self.check_bytes(total)?;
        self.declared = total;
        self.entries += 1;
        Ok(())
    }
    pub fn record_bytes(&mut self, bytes: u64) -> Result<()> {
        let total = self
            .actual
            .checked_add(bytes)
            .ok_or(SafetyError::ByteLimit)?;
        self.check_bytes(total)?;
        self.actual = total;
        Ok(())
    }
    pub fn actual_bytes(&self) -> u64 {
        self.actual
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Hardlink,
    Device,
    Fifo,
    Other,
}

pub fn validate_entry_kind(kind: EntryKind) -> Result<()> {
    match kind {
        EntryKind::Device => Err(SafetyError::DeviceNode),
        EntryKind::Fifo => Err(SafetyError::Fifo),
        EntryKind::Other => Err(SafetyError::SpecialFile),
        _ => Ok(()),
    }
}

/// Ordinary permission bits an archive may set: never group or world
/// write, so other local users can't replace the game's files (CSK-12).
pub fn safe_mode(mode: u32) -> u32 {
    mode & 0o755
}

/// Sole writable capability for a private staging root. Callers must discard
/// staging after an error. Existing files are never accepted as hardlink sources.
#[derive(Debug)]
pub struct SafeDir {
    root: Dir,
    budget: LimitTracker,
    case_sensitive: bool,
    names: BTreeMap<String, PathBuf>,
    explicit: BTreeSet<PathBuf>,
    files: BTreeSet<PathBuf>,
}
impl SafeDir {
    pub fn new(root: Dir, limits: Limits, packed_bytes: u64, case_sensitive: bool) -> Self {
        Self {
            root,
            budget: LimitTracker::new(limits, packed_bytes),
            case_sensitive,
            names: BTreeMap::new(),
            explicit: BTreeSet::new(),
            files: BTreeSet::new(),
        }
    }
    pub fn actual_bytes(&self) -> u64 {
        self.budget.actual_bytes()
    }
    /// Completed regular-file paths, for read-only re-probing after extraction.
    pub fn extracted_files(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(PathBuf::as_path)
    }
    pub fn open_read(&self, name: &str) -> Result<cap_std::fs::File> {
        Ok(self.root.open(sanitize_path(name)?)?)
    }
    /// Replace only a completed file owned by this extraction. Used for
    /// explicitly recorded generated references, never arbitrary input edits.
    /// A fresh temporary inode prevents hardlink aliases from being modified.
    pub fn rewrite_generated_file(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = sanitize_path(name)?;
        if !self.files.contains(&path) {
            return Err(SafetyError::Collision(path));
        }
        self.parents(&path)?;
        let metadata = self.root.symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(SafetyError::WriteThroughLink(path));
        }
        self.budget.record_bytes(bytes.len() as u64)?;
        for _ in 0..128 {
            let temporary = PathBuf::from(format!(
                ".caskade-rewrite-{}",
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = match self
                .root
                .open_with(&temporary, OpenOptions::new().write(true).create_new(true))
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let result = (|| {
                file.write_all(bytes)?;
                file.set_permissions(metadata.permissions())?;
                file.flush()?;
                drop(file);
                self.root.rename(&temporary, &self.root, &path)
            })();
            if let Err(error) = result {
                let _ = self.root.remove_file(&temporary);
                return Err(error.into());
            }
            return Ok(());
        }
        Err(SafetyError::Collision(path))
    }
    /// Whether this extraction owns a file, link, or directory (including an
    /// implicitly created ancestor). Never admits pre-existing staging files.
    pub fn has_entry(&self, name: &str) -> Result<bool> {
        let path = sanitize_path(name)?;
        Ok(self
            .names
            .get(&self.key(&path))
            .is_some_and(|owned| owned == &path))
    }
    /// Collision key: NFC, and case-folded unless the target is case sensitive.
    fn key(&self, path: &Path) -> String {
        let normalized: String = path.to_string_lossy().nfc().collect();
        if self.case_sensitive {
            normalized
        } else {
            normalized.to_uppercase().to_lowercase()
        }
    }
    /// The path of an entry this extraction owns, with its parents checked.
    fn owned(&self, name: &str) -> Result<PathBuf> {
        let path = sanitize_path(name)?;
        if !self.has_entry(name)? {
            return Err(SafetyError::Collision(path));
        }
        self.parents(&path)?;
        Ok(path)
    }
    /// Inspect the leaf inode without following a bundle/framework link.
    pub fn entry_is_symlink(&self, name: &str) -> Result<bool> {
        Ok(self.root.symlink_metadata(self.owned(name)?)?.is_symlink())
    }
    fn metadata_file(&self, name: &str) -> Result<cap_std::fs::File> {
        let path = self.owned(name)?;
        if self.root.symlink_metadata(&path)?.is_symlink() {
            return Err(SafetyError::WriteThroughLink(path));
        }
        Ok(self.root.open(path)?)
    }
    /// Apply only ordinary permission bits to an entry created by this job.
    /// Directory modes should be restored after all children are extracted.
    pub fn set_entry_mode(&self, name: &str, mode: u32) -> Result<()> {
        let file = self.metadata_file(name)?;
        #[cfg(unix)]
        {
            use cap_std::fs::{Permissions, PermissionsExt};
            let mut mode = safe_mode(mode);
            // The owner keeps full access to directories, or uninstall
            // couldn't empty them (CSK-12).
            if file.metadata()?.is_dir() {
                mode |= 0o700;
            }
            file.set_permissions(Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = (file, mode);
        Ok(())
    }
    /// Preserve bundle metadata through a confined descriptor. Attributes never
    /// follow symlinks; privilege-bearing Linux capabilities/ACLs are rejected.
    #[cfg(all(unix, feature = "macos-formats"))]
    pub fn set_entry_xattr(
        &mut self,
        name: &str,
        attribute: &std::ffi::OsStr,
        value: &[u8],
    ) -> Result<()> {
        use xattr::FileExt;
        self.admit_xattr(attribute, value)?;
        self.metadata_file(name)?
            .into_std()
            .set_xattr(attribute, value)?;
        Ok(())
    }
    /// Reject privilege-bearing attributes and charge the value to the budget.
    #[cfg(all(unix, feature = "macos-formats"))]
    fn admit_xattr(&mut self, attribute: &std::ffi::OsStr, value: &[u8]) -> Result<()> {
        let text = attribute.to_str().ok_or(SafetyError::InvalidEncoding)?;
        if text.starts_with("security.") || text.starts_with("system.posix_acl_") {
            return Err(SafetyError::UnsafeAttribute);
        }
        if value.len() > 1024 * 1024 {
            return Err(SafetyError::ByteLimit);
        }
        self.budget.record_bytes(value.len() as u64)
    }
    /// macOS can attach metadata to the link inode itself. Open that inode
    /// with O_SYMLINK and refuse resolution of the final component.
    #[cfg(all(target_os = "macos", feature = "macos-formats"))]
    pub fn set_symlink_xattr(
        &mut self,
        name: &str,
        attribute: &std::ffi::OsStr,
        value: &[u8],
    ) -> Result<()> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
        use cap_std::fs::OpenOptionsExt;
        use xattr::FileExt;
        let path = self.owned(name)?;
        if !self.root.symlink_metadata(&path)?.is_symlink() {
            return Err(SafetyError::InvalidSymlinkTarget);
        }
        self.admit_xattr(attribute, value)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(rustix::fs::OFlags::SYMLINK.bits() as i32);
        self.root
            .open_with(path, &options)?
            .into_std()
            .set_xattr(attribute, value)?;
        Ok(())
    }
    fn reserve(&mut self, name: &str, declared: u64, depth: usize) -> Result<PathBuf> {
        let path = sanitize_path(name)?;
        // A top-level `.caskade` holds Caskade's records and the game's Wine
        // prefix; extracted entries never go there (CSK-13).
        if in_caskade_folder(&path) {
            return Err(SafetyError::Reserved);
        }
        self.claim(path, declared, depth)
    }
    fn claim(&mut self, path: PathBuf, declared: u64, depth: usize) -> Result<PathBuf> {
        if path.components().count() > self.budget.limits.max_path_depth {
            return Err(SafetyError::DepthLimit);
        }
        if self.explicit.contains(&path) {
            return Err(SafetyError::Collision(path));
        }
        // Register every ancestor: Foo/a and foo/b must also collide.
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let key = self.key(&prefix);
            if let Some(previous) = self.names.get(&key) {
                if previous != &prefix {
                    return Err(SafetyError::Collision(prefix));
                }
            } else {
                self.names.insert(key, prefix.clone());
            }
        }
        self.budget.begin_entry(declared, depth)?;
        self.explicit.insert(path.clone());
        Ok(path)
    }
    fn parents(&self, path: &Path) -> Result<()> {
        let mut current = PathBuf::new();
        let components: Vec<_> = path.components().collect();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            current.push(component);
            match self.root.symlink_metadata(&current) {
                Ok(meta) if meta.is_symlink() => {
                    return Err(SafetyError::WriteThroughLink(current));
                }
                Ok(meta) if !meta.is_dir() => return Err(SafetyError::Collision(current)),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.root.create_dir(&current)?
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    pub fn create_dir(&mut self, name: &str, depth: usize) -> Result<PathBuf> {
        let path = self.reserve(name, 0, depth)?;
        self.parents(&path)?;
        match self.root.symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.is_symlink() => (),
            Ok(_) => return Err(SafetyError::Collision(path)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.root.create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
        Ok(path)
    }
    /// Size checks happen before writing each chunk; undeclared streams remain
    /// subject to the shared actual-byte budget. Partial files are removed.
    pub fn write_file(
        &mut self,
        name: &str,
        reader: impl Read,
        declared: Option<u64>,
        depth: usize,
        mode: u32,
    ) -> Result<PathBuf> {
        let path = self.reserve(name, declared.unwrap_or(0), depth)?;
        self.write_claimed(path, reader, declared, mode)
    }
    /// Writes one of Caskade's own records (e.g. a package's rewritten
    /// references) into the top-level `.caskade` folder, which
    /// [`Self::write_file`] keeps payload entries out of. Never pass an
    /// archive or payload name here.
    pub fn write_own_file(&mut self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = sanitize_path(name)?;
        debug_assert!(in_caskade_folder(&path), "{name} is outside .caskade");
        let path = self.claim(path, bytes.len() as u64, 0)?;
        self.write_claimed(path, bytes, Some(bytes.len() as u64), 0o644)
    }
    fn write_claimed(
        &mut self,
        path: PathBuf,
        mut reader: impl Read,
        declared: Option<u64>,
        mode: u32,
    ) -> Result<PathBuf> {
        self.parents(&path)?;
        match self.root.symlink_metadata(&path) {
            Ok(meta) if meta.is_symlink() => return Err(SafetyError::WriteThroughLink(path)),
            Ok(_) => return Err(SafetyError::Collision(path)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut file = self
            .root
            .open_with(&path, OpenOptions::new().write(true).create_new(true))?;
        let result = (|| {
            let mut actual = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                actual = actual
                    .checked_add(count as u64)
                    .ok_or(SafetyError::ByteLimit)?;
                if let Some(expected) = declared
                    && actual > expected
                {
                    return Err(SafetyError::SizeMismatch {
                        declared: expected,
                        actual,
                    });
                }
                self.budget.record_bytes(count as u64)?;
                file.write_all(&buffer[..count])?;
            }
            if let Some(expected) = declared
                && actual != expected
            {
                return Err(SafetyError::SizeMismatch {
                    declared: expected,
                    actual,
                });
            }
            #[cfg(unix)]
            {
                use cap_std::fs::{Permissions, PermissionsExt};
                file.set_permissions(Permissions::from_mode(safe_mode(mode)))?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            file.flush()?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = result {
            let _ = self.root.remove_file(&path);
            return Err(error);
        }
        self.files.insert(path.clone());
        Ok(path)
    }
    pub fn hardlink(&mut self, name: &str, target: &str, depth: usize) -> Result<PathBuf> {
        let target = sanitize_path(target).map_err(|_| SafetyError::InvalidHardlinkTarget)?;
        if !self.files.contains(&target) {
            return Err(SafetyError::InvalidHardlinkTarget);
        }
        self.parents(&target)?;
        if !self.root.symlink_metadata(&target)?.is_file() {
            return Err(SafetyError::InvalidHardlinkTarget);
        }
        let size = self.root.metadata(&target)?.len();
        let path = self.reserve(name, size, depth)?;
        self.parents(&path)?;
        self.budget.record_bytes(size)?;
        self.root.hard_link(&target, &self.root, &path)?;
        self.files.insert(path.clone());
        Ok(path)
    }
    /// Resolve through the capability first, then preserve the validated
    /// relative target. Handlers may defer links until all targets exist.
    pub fn symlink(&mut self, name: &str, target: &str, depth: usize) -> Result<PathBuf> {
        let path = sanitize_path(name)?;
        self.parents(&path)?;
        if target.is_empty()
            || target.contains('\0')
            || target.starts_with(['/', '\\'])
            || target.contains(':')
        {
            return Err(SafetyError::InvalidSymlinkTarget);
        }
        let parent = path.parent().unwrap_or(Path::new(""));
        let joined = parent.join(target.replace('\\', "/"));
        let resolved = self
            .root
            .canonicalize(&joined)
            .map_err(|_| SafetyError::InvalidSymlinkTarget)?;
        #[cfg(not(windows))]
        let _ = resolved;
        self.reserve(name, 0, depth)?;
        // Preserve the archive spelling after capability-based resolution;
        // immutable extracted targets cannot later be swapped for escaping links.
        let relative = PathBuf::from(target.replace('\\', "/"));
        #[cfg(not(windows))]
        self.root.symlink(&relative, &path)?;
        #[cfg(windows)]
        {
            if self.root.metadata(&resolved)?.is_dir() {
                self.root.symlink_dir(&relative, &path)?;
            } else {
                self.root.symlink_file(&relative, &path)?;
            }
        }
        Ok(path)
    }
}

/// Whether `path` starts with a top-level `.caskade` (any case).
fn in_caskade_folder(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|first| first.as_os_str().eq_ignore_ascii_case(".caskade"))
}

/// Canonicalize only the trusted staging root at its ambient boundary. Windows
/// std canonicalization supplies the extended-length `\\?\` representation;
/// entry names themselves must never contain that prefix.
pub fn open_staging(path: &Path) -> Result<Dir> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(Dir::open_ambient_dir(
        canonical,
        cap_std::ambient_authority(),
    )?)
}
