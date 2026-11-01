//! ROM enumeration and a pure, bounded scan pipeline (EMULATION-PLAN §6).
//! No emulators are started, archives are listed without extracting, and no
//! library records are mutated. The library adapter owns reconciliation.
mod enumerate;
mod exclusions;
mod names;
mod pipeline;
mod playlists;
use super::{GameRom, Platform, Region, RomScanner};
pub use enumerate::{enumerate, scan_folder, scan_folder_with_progress};
pub use exclusions::Exclusions;
pub use names::{DiscPart, ParsedName, parse_name};
pub use pipeline::scan_files;
pub use playlists::playlist_references;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
pub use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("{}", crate::tr!("ROM scan cancelled"))]
    Cancelled,
    #[error("{}", limit_text(.0))]
    Limit(&'static str),
    #[error("{}", invalid_text(.0))]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, ScanError>;

fn limit_text(limit: &str) -> String {
    crate::tr!("ROM scan limit exceeded: {limit}", limit = limit)
}

fn invalid_text(reason: &str) -> String {
    crate::tr!("invalid ROM scan: {reason}", reason = reason)
}
#[derive(Debug, Clone)]
pub struct ScanLimits {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_playlist_bytes: usize,
    pub max_total_text_bytes: usize,
    pub max_references: usize,
    pub max_archive_entries: usize,
    pub max_total_archive_entries: usize,
    pub max_archive_read_bytes: u64,
    pub max_total_archive_read_bytes: u64,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_files: 100_000,
            max_depth: 64,
            max_playlist_bytes: 1024 * 1024,
            max_total_text_bytes: 16 * 1024 * 1024,
            max_references: 100_000,
            max_archive_entries: 10_000,
            max_total_archive_entries: 100_000,
            max_archive_read_bytes: 32 * 1024 * 1024,
            max_total_archive_read_bytes: 256 * 1024 * 1024,
        }
    }
}
/// All input is caller-supplied, including platform and region definitions.
pub struct ScanRequest<'a> {
    pub scanner: &'a RomScanner,
    /// Resolved definition/custom profile's extension list, without dots.
    pub extensions: &'a [String],
    /// Only platforms compatible with the scanner profile.
    pub platforms: &'a [Platform],
    pub regions: &'a [Region],
    pub global_exclusions: &'a [String],
    pub limits: ScanLimits,
    /// Reserved for DAT/serial identification: supplied by the caller, not IO
    /// performed by this pure pipeline. No identifier is shipped yet.
    pub identifier: Option<&'a dyn RomIdentifier>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomFile {
    /// Confined path relative to the scanner's selected folder.
    pub path: PathBuf,
    pub size: u64,
    pub hidden: bool,
    pub cloud_placeholder: bool,
    /// Bounded decoded text for m3u/cue/gdi; never arbitrary ROM bytes.
    pub playlist: Option<String>,
    pub archive_entries: Vec<ArchiveMember>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMember {
    pub path: PathBuf,
    pub size: u64,
}
pub trait RomIdentifier: Send + Sync {
    fn identify(&self, file: &RomFile, parsed: &ParsedName) -> Option<IdentifiedRom>;
}
#[derive(Debug, Clone)]
pub struct IdentifiedRom {
    pub title: String,
    pub platform: String,
    pub regions: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RomGame {
    pub title: String,
    pub platform: String,
    pub regions: Vec<String>,
    pub properties: Vec<String>,
    pub roms: Vec<GameRom>,
    /// Includes claimed playlists/tracks, for retained review/exclusion choices.
    pub files: Vec<PathBuf>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarning {
    pub path: Option<PathBuf>,
    pub message: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub games: Vec<RomGame>,
    pub warnings: Vec<ScanWarning>,
    pub enumerated: u32,
    pub skipped: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    Enumerate,
    Match,
    Group,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub files: u32,
    pub games: u32,
}
pub(super) fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}
pub(super) fn path_text(path: &std::path::Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| ScanError::Invalid(crate::tr!("non-Unicode ROM path")))?;
    if text.len() > 4096 {
        return Err(ScanError::Limit("ROM path length"));
    }
    let safe = caskade_installer::safety::sanitize_path(text)
        .map_err(|e| ScanError::Invalid(e.to_string()))?;
    Ok(safe.to_string_lossy().replace('\\', "/"))
}
pub(super) fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("<none>")
        .to_ascii_lowercase()
}
