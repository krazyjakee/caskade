//! Epic Games Launcher (§5, #83): `Data/Manifests/*.item` under the
//! launcher's data dir (from the registry's `AppDataPath`, else
//! `ProgramData\Epic\EpicGamesLauncher\Data`), cross-checked with
//! `UnrealEngineLauncher\LauncherInstalled.dat`.
//!
//! The manifest fields follow their JSON layout as documented by Playnite's
//! EpicLibrary and game-scanner (both MIT); the code is original.

use super::{is_dir, is_file, join, json, json_text, list_or_warn, registry_string, resolve};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    registry::{RegistryHive, RegistryView},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("epic");
const LIMIT: u64 = 4 << 20;

/// `EpicGamesLauncher\Data`, from the registry or the default location.
fn data_dir(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        let path = env
            .home()
            .join("Library/Application Support/Epic/EpicGamesLauncher/Data");
        return is_dir(env, &path).then_some(path);
    }
    let registered = [
        (
            RegistryView::Registry32,
            r"SOFTWARE\Epic Games\EpicGamesLauncher",
        ),
        (
            RegistryView::Registry64,
            r"SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher",
        ),
        (
            RegistryView::Registry64,
            r"SOFTWARE\Epic Games\EpicGamesLauncher",
        ),
    ]
    .into_iter()
    .find_map(|(view, key)| {
        registry_string(env, RegistryHive::LocalMachine, view, key, "AppDataPath")
    })
    .and_then(|path| resolve(env, &path));
    registered.filter(|dir| is_dir(env, dir)).or_else(|| {
        let program_data = env
            .known_folder(KnownFolder::ProgramData)
            .map(Path::to_path_buf)
            .or_else(|| resolve(env, r"C:\ProgramData"))?;
        Some(join(&program_data, "Epic/EpicGamesLauncher/Data")).filter(|dir| is_dir(env, dir))
    })
}

/// `LauncherInstalled.dat` app names, or `None` when the file is absent.
fn launcher_installed(env: &HostEnv, ctx: &ScanCtx) -> Option<BTreeSet<String>> {
    let program_data = env.known_folder(KnownFolder::ProgramData)?;
    let file = join(
        program_data,
        "Epic/UnrealEngineLauncher/LauncherInstalled.dat",
    );
    if !is_file(env, &file) {
        return None;
    }
    let value = json(env, &file, LIMIT)
        .map_err(|error| ctx.warn(error.to_string()))
        .ok()?;
    Some(
        value
            .get("InstallationList")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                entry
                    .get("AppName")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect(),
    )
}

fn launcher_exe(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        return [KnownFolder::Applications, KnownFolder::UserApplications]
            .into_iter()
            .filter_map(|f| env.known_folder(f))
            .find_map(|p| super::macos_bundle_executable(env, &p.join("Epic Games Launcher.app")));
    }
    [KnownFolder::ProgramFilesX86, KnownFolder::ProgramFiles]
        .into_iter()
        .filter_map(|folder| env.known_folder(folder))
        .flat_map(|folder| {
            ["Win64", "Win32"].map(|arch| {
                join(
                    folder,
                    &format!("Epic Games/Launcher/Portal/Binaries/{arch}/EpicGamesLauncher.exe"),
                )
            })
        })
        .find(|exe| is_file(env, exe))
}

#[derive(Debug, Default)]
pub struct EpicGamesLauncher;

impl Source for EpicGamesLauncher {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS.with(OperatingSystem::Macos)
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let data = data_dir(env);
        let exe = launcher_exe(env);
        if data.is_none() && exe.is_none() {
            return vec![LauncherStatus::not_found(ID)];
        }
        vec![LauncherStatus::found(ID, LauncherKind::Native, exe, data)]
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let Some(data) = data_dir(env) else {
            return Ok(Vec::new());
        };
        let manifests = join(&data, "Manifests");
        let names = list_or_warn(env, &manifests, Some(ctx));
        let installed = launcher_installed(env, ctx);
        let mut games: BTreeMap<String, DetectedGame> = BTreeMap::new();
        for name in names
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(".item"))
        {
            ctx.check()?;
            let file = join(&manifests, name);
            let manifest = match json(env, &file, LIMIT) {
                Ok(manifest) => manifest,
                Err(error) => {
                    ctx.warn(error.to_string());
                    continue;
                }
            };
            if let Some(game) = game(env, &file, &manifest, installed.as_ref(), ctx)
                && !games.contains_key(&game.key)
            {
                games.insert(game.key.clone(), game);
            }
        }
        Ok(games.into_values().collect())
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths = Vec::new();
        if let Some(data) = data_dir(env) {
            paths.push(WatchPath {
                path: join(&data, "Manifests"),
                recursive: false,
            });
        }
        if let Some(program_data) = env.known_folder(KnownFolder::ProgramData) {
            paths.push(WatchPath {
                path: join(
                    program_data,
                    "Epic/UnrealEngineLauncher/LauncherInstalled.dat",
                ),
                recursive: false,
            });
        }
        paths
    }
}

fn game(
    env: &HostEnv,
    file: &Path,
    manifest: &Value,
    installed: Option<&BTreeSet<String>>,
    ctx: &ScanCtx,
) -> Option<DetectedGame> {
    let flag = |name| manifest.get(name).and_then(Value::as_bool) == Some(true);
    let app_name = json_text(manifest, "AppName")?;
    if flag("bIsIncompleteInstall") {
        tracing::debug!(app_name, "skipping incomplete Epic install");
        return None;
    }
    // DLC and add-ons carry their base game's app name.
    if json_text(manifest, "MainGameAppName").is_some_and(|main| main != app_name) {
        return None;
    }
    let categories: Vec<&str> = manifest
        .get("AppCategories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if categories.contains(&"addons")
        || (categories.contains(&"applications") && !categories.contains(&"games"))
    {
        return None;
    }
    let (Some(namespace), Some(item)) = (
        json_text(manifest, "CatalogNamespace"),
        json_text(manifest, "CatalogItemId"),
    ) else {
        ctx.warn(format!("{}: no catalog namespace or item", file.display()));
        return None;
    };
    let title = json_text(manifest, "DisplayName").unwrap_or(app_name);
    let install_dir = json_text(manifest, "InstallLocation").and_then(|path| resolve(env, path));
    let listed = installed.is_none_or(|installed| installed.contains(app_name));
    let present = install_dir.as_ref().is_some_and(|dir| is_dir(env, dir));
    if !listed && !present {
        ctx.warn(format!("{}: stale manifest for {app_name}", file.display()));
        return None;
    }
    let mut game = DetectedGame::new(ID, app_name, title);
    game.ids = vec![ExternalId::Epic {
        namespace: namespace.into(),
        catalog_item: item.into(),
        app_name: app_name.into(),
    }];
    game.launch = vec![LaunchTarget::Uri {
        uri: format!(
            "com.epicgames.launcher://apps/{namespace}%3A{item}%3A{app_name}?action=launch&silent=true"
        ),
    }];
    game.runtime = if env.os() == OperatingSystem::Windows {
        GameRuntime::Windows {
            prefix: None,
            runner: None,
        }
    } else {
        GameRuntime::Native
    };
    game.state = if present {
        InstallState::Installed
    } else {
        InstallState::Broken {
            reason: "install folder missing".into(),
        }
    };
    game.size = manifest
        .get("InstallSize")
        .and_then(Value::as_u64)
        .filter(|size| *size > 0);
    game.install_dir = install_dir;
    game.evidence = vec![Evidence::File {
        path: file.to_path_buf(),
    }];
    let mut exe_names: Vec<String> = json_text(manifest, "LaunchExecutable")
        .and_then(|exe| exe.rsplit(['/', '\\']).next())
        .map(str::to_owned)
        .into_iter()
        .collect();
    for name in manifest
        .get("ProcessNames")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !exe_names
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
        {
            exe_names.push(name.to_owned());
        }
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: game.install_dir.clone(),
        exe_names,
    });
    Some(game)
}
