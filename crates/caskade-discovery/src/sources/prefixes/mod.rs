//! Wine, Proton and CrossOver prefixes (§6.5, #91): the Windows sources run
//! inside every prefix on Linux and macOS.
//!
//! Prefixes come from Lutris, Heroic, Bottles, non-Steam shortcuts' Proton
//! prefixes, CrossOver and Whisky bottles, `$WINEPREFIX` and `~/.wine`.
//! Each becomes a Windows [`HostEnv`] (`HostEnv::wine_prefix`: the prefix's
//! registry, `drive_c` and `dosdevices`), and the uninstall-key, GameUX and
//! Windows launcher sources scan it unchanged. Results carry
//! `GameRuntime::Windows { prefix, runner }` with the prefix's owner, so a
//! launcher that owns the prefix (Lutris, Bottles, Heroic) launches it and
//! Caskade's Proton runner takes the rest. Prefixes of Steam games are
//! skipped: Steam reports those games.

mod find;

pub use find::is_prefix;

use super::join;
use crate::{
    env::HostEnv,
    types::{
        DetectedGame, Evidence, GameRuntime, LauncherKind, LauncherStatus, OsSet, Prefix,
        PrefixArch, RunnerHint, ScanCtx, Source, SourceError, SourceId, WatchPath,
    },
};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::SystemTime,
};

pub const ID: SourceId = SourceId::new("prefixes");

/// The Windows sources that run inside a prefix.
fn windows_sources() -> Vec<&'static dyn Source> {
    vec![
        #[cfg(feature = "windows-sources")]
        &crate::sources::uninstall::UninstallSource,
        #[cfg(feature = "windows-sources")]
        &crate::sources::gameux::GameUx,
        &crate::sources::epic::EpicGamesLauncher,
        &crate::sources::gog::GogGalaxy,
        &crate::sources::ubisoft::UbisoftConnect,
        &crate::sources::ea::Ea,
        &crate::sources::battlenet::BattleNet,
        #[cfg(feature = "sqlite")]
        &crate::sources::amazon::AmazonGames,
        &crate::sources::riot::RiotClient,
        &crate::sources::rockstar::RockstarLauncher,
    ]
}

/// `#arch=` from the head of `system.reg`, which Wine writes on its first
/// lines; the rest of the file (often many MiB) is never read.
pub(crate) fn arch(env: &HostEnv, prefix: &Path) -> Option<PrefixArch> {
    let file = env.resolve_path(&join(prefix, "system.reg")).ok()?;
    // A named pipe would block the open.
    if !file.is_file() {
        return None;
    }
    let mut head = Vec::new();
    fs::File::open(file)
        .ok()?
        .take(4096)
        .read_to_end(&mut head)
        .ok()?;
    let head = String::from_utf8_lossy(&head);
    if head.contains("#arch=win64") {
        Some(PrefixArch::Win64)
    } else if head.contains("#arch=win32") {
        Some(PrefixArch::Win32)
    } else {
        None
    }
}

/// Every game the Windows sources find inside one prefix, tagged with the
/// prefix. Also usable for prefixes found by deep scans.
pub fn scan_prefix(
    env: &HostEnv,
    prefix: &Path,
    owner: Option<SourceId>,
    runner: Option<RunnerHint>,
    ctx: &ScanCtx,
) -> Result<Vec<DetectedGame>, SourceError> {
    let windows = env
        .wine_prefix(prefix)
        .map_err(|error| SourceError::io(prefix, error))?;
    let tag = Prefix {
        path: prefix.to_path_buf(),
        arch: arch(env, prefix),
        owner,
    };
    let mut games = Vec::new();
    for source in windows_sources() {
        ctx.check()?;
        let found = match source.scan(&windows, ctx) {
            Ok(found) => found,
            Err(error) => {
                ctx.warn(format!("{} in {}: {error}", source.id(), prefix.display()));
                continue;
            }
        };
        for mut game in found {
            game.key = format!("{}|{}|{}", prefix.display(), game.source, game.key);
            game.evidence.push(Evidence::Rule {
                rule: format!("prefix.source.{}", game.source),
                score: 0,
            });
            game.evidence.push(Evidence::File {
                path: join(prefix, "system.reg"),
            });
            game.source = ID;
            game.runtime = GameRuntime::Windows {
                prefix: Some(tag.clone()),
                runner: runner.clone(),
            };
            games.push(game);
        }
    }
    Ok(games)
}

/// What a prefix's watch paths are worked out from: its registry and the
/// folders [`HostEnv::wine_prefix`] finds its drives, user and known folders
/// in. Sizes and modification times, so checking costs a few `stat`s. The
/// launchers whose files the sources also look for are installed through
/// their installers, which write the registry.
#[derive(Debug, PartialEq, Eq)]
struct Stamp(Vec<Option<(u64, Option<SystemTime>)>>);

impl Stamp {
    fn of(prefix: &Path) -> Self {
        Self(
            [
                "system.reg",
                "user.reg",
                "userdef.reg",
                "drive_c",
                "drive_c/users",
                "dosdevices",
            ]
            .into_iter()
            .map(|name| {
                fs::metadata(prefix.join(name))
                    .ok()
                    .map(|meta| (meta.len(), meta.modified().ok()))
            })
            .collect(),
        )
    }
}

/// Each prefix's own watch paths, by its folder, while its [`Stamp`] holds.
/// The watcher works its plan out again after every rescan, the 5-minute
/// poll's included, and a prefix's registry is often tens of MiB to parse
/// (#71). Prefixes no longer found are dropped.
static WATCHED: Mutex<BTreeMap<PathBuf, (Stamp, Vec<WatchPath>)>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Default)]
pub struct Prefixes;

impl Source for Prefixes {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::LINUX.union(OsSet::MACOS)
    }

    /// Windows launchers installed inside prefixes.
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let ctx = ScanCtx::default();
        let mut statuses = Vec::new();
        for prefix in find::prefixes(env, &ctx) {
            let Ok(windows) = env.wine_prefix(&prefix.path) else {
                continue;
            };
            for source in windows_sources() {
                for mut status in source
                    .detect(&windows)
                    .into_iter()
                    .filter(|status| status.found)
                {
                    status.kind = Some(LauncherKind::Wine {
                        prefix: prefix.path.clone(),
                    });
                    statuses.push(status);
                }
            }
        }
        statuses
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games = Vec::new();
        for prefix in find::prefixes(env, ctx) {
            ctx.check()?;
            match scan_prefix(env, &prefix.path, prefix.owner, prefix.runner, ctx) {
                Ok(found) => games.extend(found),
                Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
                Err(error) => ctx.warn(error.to_string()),
            }
        }
        Ok(games)
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let ctx = ScanCtx::default();
        let mut paths = Vec::new();
        let mut watched = WATCHED.lock().unwrap_or_else(PoisonError::into_inner);
        let mut kept = BTreeMap::new();
        for prefix in find::prefixes(env, &ctx) {
            for name in ["system.reg", "user.reg"] {
                paths.push(WatchPath {
                    path: join(&prefix.path, name),
                    recursive: false,
                });
            }
            let Ok(dir) = env.resolve_path(&prefix.path) else {
                continue;
            };
            let stamp = Stamp::of(&dir);
            let inside = match watched.remove(&dir) {
                Some((was, inside)) if was == stamp => inside,
                _ => env
                    .wine_prefix(&prefix.path)
                    .map(|windows| {
                        windows_sources()
                            .into_iter()
                            .flat_map(|source| source.watch_paths(&windows))
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            paths.extend(inside.iter().cloned());
            kept.insert(dir, (stamp, inside));
        }
        *watched = kept;
        paths
    }

    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        vec![join(env.home(), ".wine")]
    }
}
