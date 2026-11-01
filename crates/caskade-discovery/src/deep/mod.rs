//! Opt-in discovery outside launcher inventories (#98).
//!
//! Walks are confined to explicit roots, never follow directory links, check
//! cancellation between entries, and classify bounded inventories without
//! executing candidates. `scan_with_options` exposes progress and Caskade root
//! handoffs for a storage adapter; the built-in source returns external games.
mod control;
mod executable;
mod walk;

use crate::{HostEnv, env::VolumeKind, types::*};
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
    let deep = &ctx.config().deep;
    let mut phases = Vec::new();
    if !deep.folders.is_empty() {
        phases.push((ScanKind::CustomFolders, deep.folders.clone()));
    }
    if deep.external_drives {
        let volumes = env
            .volumes()
            .map_err(|e| SourceError::io(env.fs_root(), e))?;
        phases.push((
            ScanKind::ExternalDrives,
            volumes
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
        let mut roots = env
            .volumes()
            .map_err(|e| SourceError::io(env.fs_root(), e))?
            .into_iter()
            .map(|v| v.root)
            .collect::<Vec<_>>();
        if roots.is_empty() {
            roots.push(env.fs_root().to_path_buf());
        }
        phases.push((ScanKind::FullDisk, roots));
    }
    let mut report = Report::default();
    for (kind, roots) in phases {
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
        .with_drives(std::sync::Arc::new(crate::testing::FakeDrives::new(
            volumes,
        )));
    report
        .games
        .extend(crate::sources::xbox::Xbox.scan(&external, ctx)?);
    control.check()
}

/// Config-aware watch roots, including folders outside the home directory.
pub fn watch_paths(env: &HostEnv, config: &crate::Config) -> Vec<WatchPath> {
    let deep = &config.deep;
    let mut roots = deep.folders.clone();
    if deep.portable {
        roots.extend([env.home().join("Applications"), env.home().join("Games")]);
    }
    if deep.wine_prefixes {
        roots.push(env.home().to_path_buf());
    }
    if deep.external_drives || deep.full_disk {
        roots.extend(
            env.volumes()
                .unwrap_or_default()
                .into_iter()
                .filter(|v| deep.full_disk || v.kind == VolumeKind::Removable)
                .take(256)
                .map(|v| v.root),
        );
    }
    let roots: BTreeSet<_> = roots
        .into_iter()
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
