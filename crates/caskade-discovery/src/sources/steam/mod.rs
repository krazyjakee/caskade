//! Steam (§5, #72): every installed game in every library of every Steam
//! install (native, Flatpak, Snap), without tools, runtimes, redistributables
//! or DLC.
//!
//! Install locations come from `steamlocate` (live mode) and the same
//! well-known paths read through `HostEnv`. Files are parsed with the crate's
//! bounded readers. File locations follow steamlocate (MIT, a dependency);
//! nothing is ported from GPL projects.

mod apps;
mod art;
mod library;
mod roots;
pub mod shortcuts;
mod users;
mod vdf;

use super::{canonical, is_dir, join};
use crate::{
    OperatingSystem,
    env::HostEnv,
    types::{
        Confidence, DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget,
        LauncherStatus, OsSet, Prefix, RunnerHint, RunnerKind, ScanCtx, Source, SourceError,
        SourceId, TrackingHint, UninstallInfo, WatchPath,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("steam");

#[derive(Debug, Default)]
pub struct Steam;

impl Steam {
    /// The games of one library folder (`<library>/steamapps`), such as an
    /// orphan library on a removable drive with no Steam install. Tool
    /// filtering falls back to `data/steam-tools.toml` and names.
    pub fn scan_library(
        env: &HostEnv,
        library: &Path,
        ctx: &ScanCtx,
    ) -> Result<Vec<DetectedGame>, SourceError> {
        let tools = BTreeMap::new();
        let mut games = Vec::new();
        for manifest in library::manifests(env, library, ctx)? {
            ctx.check()?;
            games.extend(game(env, library, library, &manifest, None, &tools, ctx));
        }
        Ok(games)
    }
}

impl Source for Steam {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let roots = roots::roots(env);
        if roots.is_empty() {
            return vec![LauncherStatus::not_found(ID)];
        }
        roots
            .into_iter()
            .map(|root| LauncherStatus {
                version: roots::version(env, &root.path),
                ..LauncherStatus::found(ID, root.kind, Some(root.path.clone()), Some(root.path))
            })
            .collect()
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games: BTreeMap<u32, DetectedGame> = BTreeMap::new();
        let mut played = Vec::new();
        let mut seen_libraries = BTreeSet::new();
        for root in roots::roots(env) {
            ctx.check()?;
            if let Some(account) = users::account(env, &root.path, ctx) {
                played.push(users::played(env, &root.path, account, ctx));
            }
            let tools = users::compat_tools(env, &root.path, ctx);
            let mut found = Vec::new();
            for library in library::libraries(env, &root.path, ctx) {
                if !answers(env, &library, Some(ctx)) {
                    continue;
                }
                let key = canonical(env, &library);
                if !seen_libraries.insert(key) {
                    continue;
                }
                for manifest in library::manifests(env, &library, ctx)? {
                    found.push((library.clone(), manifest));
                }
            }
            let wanted = found
                .iter()
                .map(|(_, manifest)| manifest.app_id)
                .filter(|id| !games.contains_key(id))
                .collect();
            let info = apps::app_info(env, &root.path, &wanted, ctx);
            for (library, manifest) in found {
                ctx.check()?;
                if games.contains_key(&manifest.app_id) {
                    continue;
                }
                if let Some(game) = game(
                    env,
                    &root.path,
                    &library,
                    &manifest,
                    info.get(&manifest.app_id),
                    &tools,
                    ctx,
                ) {
                    games.insert(manifest.app_id, game);
                }
            }
        }
        // Several installs can share a library and an account's history.
        for game in games.values_mut() {
            let Some(app_id) = game
                .tracking
                .as_ref()
                .and_then(|tracking| tracking.steam_app_id)
            else {
                continue;
            };
            for played in played.iter().filter_map(|played| played.get(&app_id)) {
                game.playtime_secs = game.playtime_secs.max(played.playtime_secs);
                game.last_played_unix = game.last_played_unix.max(played.last_played_unix);
            }
        }
        Ok(games.into_values().collect())
    }

    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        let mut candidates = roots::candidates(env);
        candidates.dedup();
        candidates
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths = BTreeSet::new();
        let ctx = ScanCtx::default();
        for root in roots::roots(env) {
            for relative in [
                "steamapps/libraryfolders.vdf",
                "config/loginusers.vdf",
                "config/config.vdf",
            ] {
                paths.insert((join(&root.path, relative), false));
            }
            if let Some(account) = users::account(env, &root.path, &ctx) {
                let config = format!("userdata/{account}/config/localconfig.vdf");
                paths.insert((join(&root.path, &config), false));
            }
            for library in library::libraries(env, &root.path, &ctx) {
                if answers(env, &library, None) {
                    paths.insert((join(&library, "steamapps"), false));
                }
            }
        }
        paths
            .into_iter()
            .map(|(path, recursive)| WatchPath { path, recursive })
            .collect()
    }
}

/// Whether a library's drive answers. One that doesn't (a WSL, network or
/// unplugged path that blocks) is skipped with a warning, so the other
/// libraries' games still count instead of the whole source timing out.
/// A library that answers but has no `steamapps` warns later, as before.
fn answers(env: &HostEnv, library: &Path, ctx: Option<&ScanCtx>) -> bool {
    if super::dir_within(env, &join(library, "steamapps"), super::ANSWER_WAIT).is_some() {
        return true;
    }
    tracing::warn!(library = %library.display(), "Steam library did not answer; skipped");
    if let Some(ctx) = ctx {
        ctx.warn(format!(
            "library {} did not answer within {} s; skipped",
            library.display(),
            super::ANSWER_WAIT.as_secs()
        ));
    }
    false
}

fn game(
    env: &HostEnv,
    root: &Path,
    library: &Path,
    manifest: &library::Manifest,
    info: Option<&apps::AppInfo>,
    tools: &BTreeMap<u32, String>,
    ctx: &ScanCtx,
) -> Option<DetectedGame> {
    let app_id = manifest.app_id;
    let state = library::state(manifest.flags)?;
    let install_dir_name = manifest.install_dir.clone().unwrap_or_default();
    let title = manifest
        .name
        .clone()
        .or_else(|| info.and_then(|info| info.name.clone()))
        .unwrap_or_else(|| install_dir_name.clone());
    if let Some(reason) = apps::not_a_game(app_id, info, &title, &install_dir_name) {
        tracing::debug!(app_id, %reason, "skipping Steam app");
        return None;
    }
    if title.is_empty() {
        ctx.warn(format!(
            "{}: no name or installdir",
            manifest.file.display()
        ));
        return None;
    }
    let mut game = DetectedGame::new(ID, app_id.to_string(), title);
    game.ids = vec![ExternalId::Steam { app_id }];
    game.install_dir = (!install_dir_name.is_empty())
        .then(|| join(library, &format!("steamapps/common/{install_dir_name}")));
    game.launch = vec![LaunchTarget::Uri {
        uri: format!("steam://rungameid/{app_id}"),
    }];
    game.runtime = runtime(env, library, app_id, tools);
    game.state = state;
    // Only advertise a folder that exists. A download may not have created
    // it yet; any other record without its folder is a broken install that
    // Steam still lists.
    if game
        .install_dir
        .as_ref()
        .is_some_and(|dir| !is_dir(env, dir))
    {
        game.install_dir = None;
        if game.state != InstallState::Partial {
            game.state = InstallState::Broken {
                reason: "install folder missing".into(),
            };
            game.confidence = Confidence::Likely;
        }
    }
    game.size = manifest.size.filter(|size| *size > 0);
    game.last_played_unix = manifest.last_played;
    (game.icon, game.artwork) = art::artwork(env, root, app_id);
    game.uninstall = Some(UninstallInfo {
        target: LaunchTarget::Uri {
            uri: format!("steam://uninstall/{app_id}"),
        },
        quiet: None,
        registry_key: None,
    });
    game.evidence = vec![Evidence::File {
        path: manifest.file.clone(),
    }];
    game.tracking = Some(TrackingHint {
        steam_app_id: Some(app_id),
        install_dir: game.install_dir.clone(),
        exe_names: Vec::new(),
    });
    Some(game)
}

/// Native on macOS and for Linux games without a compatibility tool; a
/// Windows game on Windows, or under Proton with its `compatdata` prefix.
pub(super) fn runtime(
    env: &HostEnv,
    library: &Path,
    app_id: u32,
    tools: &BTreeMap<u32, String>,
) -> GameRuntime {
    match env.os() {
        OperatingSystem::Windows => {
            return GameRuntime::Windows {
                prefix: None,
                runner: None,
            };
        }
        OperatingSystem::Macos => return GameRuntime::Native,
        OperatingSystem::Linux => {}
    }
    let pfx = join(library, &format!("steamapps/compatdata/{app_id}/pfx"));
    let prefix = is_dir(env, &pfx).then(|| Prefix {
        arch: super::prefixes::arch(env, &pfx),
        path: pfx,
        owner: Some(ID),
    });
    let explicit = tools.get(&app_id);
    if prefix.is_none() && explicit.is_none() {
        return GameRuntime::Native;
    }
    let runner = explicit.or_else(|| tools.get(&0)).map(|name| RunnerHint {
        kind: if name.to_ascii_lowercase().contains("proton") {
            RunnerKind::Proton
        } else {
            RunnerKind::Other
        },
        name: Some(name.clone()),
        path: None,
    });
    GameRuntime::Windows { prefix, runner }
}

/// Proton prefixes of non-Steam shortcuts (`compatdata/<id>/pfx` with a
/// shortcut app id, which has the top bit set) and their compatibility
/// tool. Prefixes of Steam games are left out: Steam reports those games.
pub(crate) fn shortcut_prefixes(env: &HostEnv, ctx: &ScanCtx) -> Vec<(PathBuf, Option<String>)> {
    let mut prefixes = Vec::new();
    for root in roots::roots(env) {
        let tools = users::compat_tools(env, &root.path, ctx);
        let compatdata = join(&root.path, "steamapps/compatdata");
        for name in super::list(env, &compatdata).unwrap_or_default() {
            let Ok(id) = name.parse::<u32>() else {
                continue;
            };
            if id & 0x8000_0000 == 0 {
                continue;
            }
            let pfx = join(&compatdata, &format!("{name}/pfx"));
            if is_dir(env, &pfx) {
                let tool = tools.get(&id).or_else(|| tools.get(&0)).cloned();
                prefixes.push((pfx, tool));
            }
        }
    }
    prefixes
}
