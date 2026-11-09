//! Catalogue packages (#182): download a release's archive, check it
//! against the catalogue (size and SHA-256), unpack it safely into a
//! staging folder and check its identity, then swap it into place with a
//! journal so a crash between the two steps puts the previous version
//! back on the next start. What a package is (a plugin, a theme) and how
//! it activates is the adapter's: it extends the plugin host and the
//! theme service rather than adding another package manager.

use super::{AddonKind, CatalogueEntry, CatalogueRelease};
use caskade_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
/// Stops a download (re-exported for the adapter).
pub use tokio_util::sync::CancellationToken;

/// Files an archive may hold, and their total size, as the plugin host's
/// folder installs allow.
const MAX_FILES: usize = 20_000;
const MAX_UNPACKED: u64 = 512 << 20;

/// Why a package didn't install. Integrity, safety and identity failures
/// aren't the network's: retrying them gets the same answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AddonFailure {
    /// The download didn't match the catalogue's size or SHA-256.
    Integrity,
    /// The archive is malformed, too large, or has unsafe paths or links.
    Unsafe,
    /// The archive holds another add-on, or another version.
    Identity,
    /// Its manifest or theme files are invalid, or this Caskade can't run it.
    Incompatible,
    /// It installed but didn't start; the previous version is back.
    Activation,
    /// The download failed; trying again later may work.
    Network,
    /// Disk or file trouble; trying again later may work.
    Storage,
    Cancelled,
}

impl AddonFailure {
    /// Worth retrying as is: the network or the disk may do better.
    pub fn transient(self) -> bool {
        matches!(self, Self::Network | Self::Storage)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    pub kind: AddonFailure,
    pub message: String,
}

impl PackageError {
    pub fn new(kind: AddonFailure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self::new(AddonFailure::Storage, error.to_string())
    }
}

/// Checks `archive` against the catalogue's size and SHA-256.
pub fn verify(archive: &Path, release: &CatalogueRelease) -> std::result::Result<(), PackageError> {
    let mut file = std::fs::File::open(archive)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 << 10];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > release.artifact.size {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if size != release.artifact.size {
        return Err(PackageError::new(
            AddonFailure::Integrity,
            caskade_core::tr!("the download's size doesn't match the catalogue"),
        ));
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != release.artifact.sha256 {
        return Err(PackageError::new(
            AddonFailure::Integrity,
            caskade_core::tr!("the download's checksum doesn't match the catalogue"),
        ));
    }
    Ok(())
}

/// Downloads `release`'s archive to `<dir>/<sha256>.zip`, stopping at the
/// catalogue's size, and checks it; an archive already there that checks
/// out is used as is. `progress` gets the bytes so far.
#[cfg(feature = "net")]
pub async fn download(
    release: &CatalogueRelease,
    dir: &Path,
    cancel: &CancellationToken,
    mut progress: impl FnMut(u64),
) -> std::result::Result<PathBuf, PackageError> {
    use tokio::io::AsyncWriteExt;
    tokio::fs::create_dir_all(dir).await?;
    let archive = dir.join(format!("{}.zip", release.artifact.sha256));
    if archive.is_file() && verify(&archive, release).is_ok() {
        progress(release.artifact.size);
        return Ok(archive);
    }
    let network = |error: reqwest::Error| {
        PackageError::new(
            AddonFailure::Network,
            caskade_core::tr!("the download failed: {error}", error = error.without_url()),
        )
    };
    let client = reqwest::Client::builder()
        .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(network)?;
    let mut response = client
        .get(&release.artifact.url)
        .send()
        .await
        .map_err(network)?;
    if !response.status().is_success() {
        return Err(PackageError::new(
            AddonFailure::Network,
            caskade_core::tr!(
                "the download server answered {status}",
                status = response.status().as_u16()
            ),
        ));
    }
    if cancel.is_cancelled() {
        return Err(PackageError::new(
            AddonFailure::Cancelled,
            caskade_core::tr!("cancelled"),
        ));
    }
    let partial = archive.with_extension("partial");
    let mut file = tokio::fs::File::create(&partial).await?;
    let mut received = 0u64;
    loop {
        let chunk = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                drop(file);
                let _ = tokio::fs::remove_file(&partial).await;
                return Err(PackageError::new(AddonFailure::Cancelled, caskade_core::tr!("cancelled")));
            }
            chunk = response.chunk() => chunk.map_err(network)?,
        };
        let Some(chunk) = chunk else { break };
        received += chunk.len() as u64;
        if received > release.artifact.size {
            drop(file);
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(PackageError::new(
                AddonFailure::Integrity,
                caskade_core::tr!("the download is larger than the catalogue says"),
            ));
        }
        file.write_all(&chunk).await?;
        progress(received);
    }
    file.sync_all().await?;
    drop(file);
    let checked = {
        let (partial, release) = (partial.clone(), release.clone());
        tokio::task::spawn_blocking(move || verify(&partial, &release))
            .await
            .map_err(|error| PackageError::new(AddonFailure::Storage, error.to_string()))?
    };
    if let Err(error) = checked {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    tokio::fs::rename(&partial, &archive).await?;
    Ok(archive)
}

/// Unpacks `archive` into the empty folder `into` and returns the
/// package's folder: `into`, or the one folder everything is in. Refuses
/// absolute and `..` paths, links, and more than the limits allow.
pub fn extract(archive: &Path, into: &Path) -> std::result::Result<PathBuf, PackageError> {
    let unsafe_archive = |message: String| PackageError::new(AddonFailure::Unsafe, message);
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        unsafe_archive(caskade_core::tr!(
            "the download isn't a readable zip archive"
        ))
    })?;
    if zip.len() > MAX_FILES {
        return Err(unsafe_archive(caskade_core::tr!(
            "the package has too many files"
        )));
    }
    std::fs::create_dir_all(into)?;
    let mut total = 0u64;
    let mut tops = std::collections::BTreeSet::new();
    let mut files_at_root = false;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| {
            unsafe_archive(caskade_core::tr!(
                "the download isn't a readable zip archive"
            ))
        })?;
        let raw = entry
            .name()
            .map(|name| name.into_owned())
            .unwrap_or_default();
        // An absolute or drive path is refused, not quietly made relative.
        let absolute = raw.starts_with(['/', '\\']) || raw.as_bytes().get(1) == Some(&b':');
        let Some(relative) = entry.enclosed_name().filter(|_| !absolute) else {
            return Err(unsafe_archive(caskade_core::tr!(
                "the package has an unsafe path: {path}",
                path = entry
                    .name()
                    .map(|name| name.into_owned())
                    .unwrap_or_default()
            )));
        };
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(unsafe_archive(caskade_core::tr!(
                "the package has a link: {path}",
                path = relative.display()
            )));
        }
        let mut parts = relative.components();
        if let Some(first) = parts.next() {
            if parts.next().is_some() || entry.is_dir() {
                tops.insert(first.as_os_str().to_owned());
            } else {
                files_at_root = true;
            }
        }
        let target = into.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        total += entry.size();
        if total > MAX_UNPACKED {
            return Err(unsafe_archive(caskade_core::tr!(
                "the package unpacks to more than 512 MiB"
            )));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        // Bounded by what the header says, so a lying header can't grow it.
        let size = entry.size();
        let copied = std::io::copy(&mut (&mut entry).take(size + 1), &mut out)?;
        if copied > size {
            return Err(unsafe_archive(caskade_core::tr!(
                "the package's sizes don't match its contents"
            )));
        }
    }
    Ok(match (tops.len(), files_at_root) {
        (1, false) => into.join(tops.into_iter().next().expect("one")),
        _ => into.to_path_buf(),
    })
}

/// Whether a plugin manifest contributes the provider interface the
/// catalogue never offers (custom metadata sources, #6).
pub fn contributes_metadata(manifest: &serde_json::Value) -> bool {
    manifest
        .pointer("/contributes/providers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .any(|provider| provider.as_str() == Some(super::validate::RESERVED_PROVIDER))
}

/// A package folder's manifest, read as JSON.
fn manifest(kind: AddonKind, root: &Path) -> std::result::Result<serde_json::Value, PackageError> {
    let file = match kind {
        AddonKind::Plugin => "plugin.json",
        AddonKind::Theme => "theme.json",
    };
    let wrong = || {
        PackageError::new(
            AddonFailure::Identity,
            caskade_core::tr!("the package has no readable {file}", file = file),
        )
    };
    let bytes = std::fs::read(root.join(file)).map_err(|_| wrong())?;
    serde_json::from_slice(&bytes).map_err(|_| wrong())
}

/// The id and version a package folder's manifest declares.
pub fn identity(
    kind: AddonKind,
    root: &Path,
) -> std::result::Result<(String, String), PackageError> {
    let file = match kind {
        AddonKind::Plugin => "plugin.json",
        AddonKind::Theme => "theme.json",
    };
    let wrong = || {
        PackageError::new(
            AddonFailure::Identity,
            caskade_core::tr!("the package has no readable {file}", file = file),
        )
    };
    let manifest = manifest(kind, root)?;
    let field = |key: &str| {
        manifest
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let (Some(id), Some(version)) = (field("id"), field("version")) else {
        return Err(wrong());
    };
    Ok((id, version))
}

/// Checks the package holds `entry` at `release`'s version, and that a
/// plugin doesn't contribute the metadata provider the catalogue keeps out,
/// whatever its record declares.
pub fn check_identity(
    entry: &CatalogueEntry,
    release: &CatalogueRelease,
    root: &Path,
) -> std::result::Result<(), PackageError> {
    let (id, version) = identity(entry.kind, root)?;
    if entry.kind == AddonKind::Plugin && contributes_metadata(&manifest(entry.kind, root)?) {
        return Err(PackageError::new(
            AddonFailure::Incompatible,
            caskade_core::tr!("the package provides metadata, which the catalogue doesn't offer"),
        ));
    }
    let same_version = semver::Version::parse(&version)
        .ok()
        .zip(semver::Version::parse(&release.version).ok())
        .is_some_and(|(a, b)| a == b);
    if id != entry.id || !same_version {
        return Err(PackageError::new(
            AddonFailure::Identity,
            caskade_core::tr!(
                "the package is {id} {version}, not {expected} {expected_version}",
                id = id,
                version = version,
                expected = entry.id,
                expected_version = release.version
            ),
        ));
    }
    Ok(())
}

/// The journal a swap writes before it moves anything.
#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    target: PathBuf,
    backup: PathBuf,
}

/// A package swapped into place, until it is confirmed or rolled back.
#[must_use = "a swap must be committed or rolled back"]
pub struct Swap {
    journal: PathBuf,
    target: PathBuf,
    backup: PathBuf,
}

const JOURNAL: &str = "journal.json";

/// Moves `staged` to `target`, keeping what was there as a backup until the
/// swap is committed. `work` (the add-ons folder, on the same file system)
/// holds the journal and the backup.
pub fn swap_in(
    staged: &Path,
    target: &Path,
    work: &Path,
) -> std::result::Result<Swap, PackageError> {
    std::fs::create_dir_all(work)?;
    let name = target
        .file_name()
        .ok_or_else(|| PackageError::new(AddonFailure::Storage, "no folder name"))?
        .to_string_lossy();
    // Beside the journal, not beside the target: a plugins or themes folder
    // would read the backup as a second copy of the add-on.
    let backup = work.join("previous").join(name.as_ref());
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let journal = work.join(JOURNAL);
    let record = Journal {
        target: target.to_path_buf(),
        backup: backup.clone(),
    };
    std::fs::write(
        &journal,
        serde_json::to_vec(&record)
            .map_err(|e| PackageError::new(AddonFailure::Storage, e.to_string()))?,
    )?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        std::fs::rename(target, &backup)?;
    }
    if let Err(error) = std::fs::rename(staged, target) {
        if backup.exists() {
            std::fs::rename(&backup, target)?;
        }
        std::fs::remove_file(&journal)?;
        return Err(error.into());
    }
    Ok(Swap {
        journal,
        target: target.to_path_buf(),
        backup,
    })
}

impl Swap {
    /// Keeps the new version; the backup goes.
    pub fn commit(self) -> std::result::Result<(), PackageError> {
        if self.backup.exists() {
            std::fs::remove_dir_all(&self.backup)?;
        }
        std::fs::remove_file(&self.journal)?;
        Ok(())
    }

    /// Puts the previous version back (or nothing, for a first install).
    pub fn rollback(self) -> std::result::Result<(), PackageError> {
        restore(&Journal {
            target: self.target.clone(),
            backup: self.backup.clone(),
        })?;
        std::fs::remove_file(&self.journal)?;
        Ok(())
    }
}

fn restore(journal: &Journal) -> std::io::Result<()> {
    if journal.target.exists() {
        std::fs::remove_dir_all(&journal.target)?;
    }
    if journal.backup.exists() {
        std::fs::rename(&journal.backup, &journal.target)?;
    }
    Ok(())
}

/// At startup, before plugins and themes load: rolls back a swap that was
/// never confirmed (the app stopped mid-install) and clears staging.
/// Whether a swap was rolled back.
pub fn recover(work: &Path) -> Result<bool> {
    let journal_path = work.join(JOURNAL);
    let rolled_back = match std::fs::read(&journal_path) {
        Ok(bytes) => {
            if let Ok(journal) = serde_json::from_slice::<Journal>(&bytes) {
                restore(&journal)?;
                tracing::warn!(target = %journal.target.display(), "an interrupted add-on install was rolled back");
            }
            std::fs::remove_file(&journal_path)?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let staging = work.join("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    Ok(rolled_back)
}

/// What Caskade knows about an add-on it installed from the catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonRecord {
    /// The version installed from the catalogue.
    pub version: Option<String>,
    /// An update the user chose not to take.
    pub skipped: Option<String>,
    /// A release that failed a check that won't pass by retrying; it isn't
    /// offered again until the user asks.
    pub rejected: Option<RejectedRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RejectedRelease {
    pub version: String,
    pub kind: AddonFailure,
    pub message: String,
}

/// The user's choice about updates from the catalogue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePreference {
    /// Check when Settings opens and show what can be updated.
    #[default]
    Notify,
    /// Only when the user presses Check for updates.
    Manual,
}

/// The add-on records and the update preference, kept in the library's
/// settings table (backed up with it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AddonRecords {
    pub updates: UpdatePreference,
    pub addons: BTreeMap<String, AddonRecord>,
}

const RECORDS: &str = "addons.records";

impl AddonRecords {
    pub async fn load(store: &caskade_core::db::LibraryStore) -> Result<Self> {
        Ok(store
            .get_setting(RECORDS)
            .await?
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default())
    }

    pub async fn save(&self, store: &caskade_core::db::LibraryStore) -> Result<()> {
        store
            .set_setting(RECORDS, &serde_json::to_value(self).map_err(Error::from)?)
            .await
    }
}
