//! Opt-in discovery outside launcher inventories (#98).
//!
//! Walks are confined to explicit roots, never follow directory links, check
//! cancellation between entries, and classify bounded inventories without
//! executing candidates. `scan_with_options` exposes progress and the Caskade
//! storage roots it finds; the built-in source returns external games and
//! reports those roots as warnings.
mod control;
mod executable;
mod walk;

use crate::{
    DeepScans, HostEnv,
    env::{Volume, VolumeKind},
    types::*,
};
use std::{collections::BTreeSet, path::PathBuf};

pub const ID: SourceId = SourceId::new("deep");
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScanKind {
    CustomFolders,
    ExternalDrives,
    Portable,
    WinePrefixes,
    FullDisk,
}
impl ScanKind {
    fn label(self) -> &'static str {
        match self {
            Self::CustomFolders => "custom",
            Self::ExternalDrives => "external",
            Self::Portable => "portable",
            Self::WinePrefixes => "prefixes",
            Self::FullDisk => "full_disk",
        }
    }
}
/// Limits apply independently to each enabled scan kind, across all its roots.
#[derive(Debug, Clone)]
pub struct Options {
    pub budget_ms: u64,
    pub max_entries: u64,
    pub max_depth: u32,
    /// Optional injected ELF/PE architecture (x86, x86_64 or aarch64).
    pub arch: Option<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            budget_ms: 2000,
            max_entries: 20_000,
            max_depth: 8,
            arch: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Progress {
    pub kind: ScanKind,
    pub root: PathBuf,
    pub visited: u64,
    pub candidates: usize,
    pub done: bool,
}
pub type ProgressSink<'a> = dyn Fn(Progress) + Send + Sync + 'a;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaskadeRoot {
    pub path: PathBuf,
    pub root_id: String,
}
#[derive(Debug, Default)]
pub struct Report {
    pub games: Vec<DetectedGame>,
    /// Managed roots belong to storage, not external-game discovery.
    pub caskade_roots: Vec<CaskadeRoot>,
    pub timed_out: Vec<ScanKind>,
    pub visited: u64,
}

pub fn scan_with_options(
    env: &HostEnv,
    ctx: &ScanCtx,
    options: &Options,
    progress: &ProgressSink<'_>,
) -> Result<Report, SourceError> {
    ctx.check()?;
    if options.max_depth > 32 || options.max_entries > 1_000_000 {
        return Err(SourceError::parse(
            env.fs_root(),
            "deep scan limits exceed safety bounds",
        ));
    }
    let phases = roots(env, &ctx.config().deep, || {
        env.volumes().map_err(|e| SourceError::io(env.fs_root(), e))
    })?;
    let mut report = Report::default();
    for (kind, mut roots) in phases {
        // No volumes listed: the full-disk scan starts at the filesystem
        // root, which is never watched.
        if kind == ScanKind::FullDisk && roots.is_empty() {
            roots.push(env.fs_root().to_path_buf());
        }
        ctx.check()?;
        let mut control = control::Control::new(env, ctx, options, kind, progress);
        let mut seen = BTreeSet::new();
        let mut result = Ok(());
        for root in roots.into_iter().take(256) {
            let root = match env.resolve_path(&root) {
                Ok(p) => p,
                Err(e) => {
                    ctx.warn(format!("deep {}: {e}", root.display()));
                    continue;
                }
            };
            if !seen.insert(root.clone()) {
                continue;
            }
            control.emit(&root, report.games.len(), false);
            result = control
                .check()
                .and_then(|()| walk::root(&root, &mut control, &mut report));
            if result.is_err() {
                break;
            }
        }
        #[cfg(feature = "windows-sources")]
        if kind == ScanKind::ExternalDrives && result.is_ok() {
            result = external_xbox(env, ctx, &control, &mut report);
        }
        report.visited = report.visited.saturating_add(control.visited);
        control.emit(env.fs_root(), report.games.len(), true);
        if let Err(error) = control.check() {
            result = Err(error);
        }
        match result {
            Err(SourceError::TimedOut) => {
                report.timed_out.push(kind);
                ctx.warn(format!("deep {} time budget reached", kind.label()));
            }
            Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
            Err(error) => ctx.warn(error.to_string()),
            Ok(()) => (),
        }
    }
    report.caskade_roots.sort_by(|a, b| a.path.cmp(&b.path));
    report.caskade_roots.dedup_by(|a, b| a.path == b.path);
    let mut seen = BTreeSet::new();
    report
        .games
        .retain(|game| seen.insert((game.source.clone(), game.key.clone())));
    ctx.check()?;
    Ok(report)
}

#[cfg(feature = "windows-sources")]
fn external_xbox(
    env: &HostEnv,
    ctx: &ScanCtx,
    control: &control::Control<'_>,
    report: &mut Report,
) -> Result<(), SourceError> {
    control.check()?;
    let volumes = env
        .volumes()
        .map_err(|e| SourceError::io(env.fs_root(), e))?
        .into_iter()
        .filter(|v| v.kind == VolumeKind::Removable)
        .collect();
    let external = env
        .clone()
        .with_drives(std::sync::Arc::new(crate::env::FixedDrives::new(volumes)));
    report
        .games
        .extend(crate::sources::xbox::Xbox.scan(&external, ctx)?);
    control.check()
}

/// The roots of each enabled scan kind. `volumes` is read only for the
/// kinds that need it.
fn roots(
    env: &HostEnv,
    deep: &DeepScans,
    volumes: impl Fn() -> Result<Vec<Volume>, SourceError>,
) -> Result<Vec<(ScanKind, Vec<PathBuf>)>, SourceError> {
    let mut phases = Vec::new();
    if !deep.folders.is_empty() {
        phases.push((ScanKind::CustomFolders, deep.folders.clone()));
    }
    if deep.external_drives {
        phases.push((
            ScanKind::ExternalDrives,
            volumes()?
                .into_iter()
                .filter(|v| v.kind == VolumeKind::Removable)
                .map(|v| v.root)
                .collect(),
        ));
    }
    if deep.portable {
        phases.push((
            ScanKind::Portable,
            vec![env.home().join("Applications"), env.home().join("Games")],
        ));
    }
    if deep.wine_prefixes {
        phases.push((ScanKind::WinePrefixes, vec![env.home().to_path_buf()]));
    }
    if deep.full_disk {
        phases.push((
            ScanKind::FullDisk,
            volumes()?.into_iter().map(|v| v.root).collect(),
        ));
    }
    Ok(phases)
}

/// Config-aware watch roots, including folders outside the home directory.
pub fn watch_paths(env: &HostEnv, config: &crate::Config) -> Vec<WatchPath> {
    // At most 256 drives.
    let volumes = || {
        Ok(env
            .volumes()
            .unwrap_or_default()
            .into_iter()
            .take(256)
            .collect())
    };
    let roots: BTreeSet<_> = roots(env, &config.deep, volumes)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|(_, roots)| roots)
        .filter_map(|p| env.resolve_path(&p).ok())
        .collect();
    roots
        .into_iter()
        .map(|path| WatchPath {
            path,
            recursive: true,
        })
        .collect()
}
#[derive(Debug, Default)]
pub struct Deep;
impl Source for Deep {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, _env: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let deep = &ctx.config().deep;
        let enabled = usize::from(!deep.folders.is_empty())
            + usize::from(deep.external_drives)
            + usize::from(deep.portable)
            + usize::from(deep.wine_prefixes)
            + usize::from(deep.full_disk);
        if enabled == 0 {
            ctx.check()?;
            return Ok(vec![]);
        }
        let options = Options {
            budget_ms: (ctx.config().source_budget_ms / enabled as u64).max(1),
            ..Default::default()
        };
        let report = scan_with_options(env, ctx, &options, &|p| {
            if p.done || p.visited % 128 == 0 {
                tracing::debug!(
                    kind = p.kind.label(),
                    visited = p.visited,
                    candidates = p.candidates,
                    done = p.done,
                    "deep scan progress"
                );
            }
        })?;
        for root in report.caskade_roots {
            ctx.warn(format!(
                "Caskade storage root {} ({}) requires storage handoff",
                root.path.display(),
                root.root_id
            ));
        }
        Ok(report.games)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        watch_paths(env, &crate::Config::default())
    }
}
