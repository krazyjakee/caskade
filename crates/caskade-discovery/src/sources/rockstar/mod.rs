//! Rockstar Games Launcher (§5, #88).
//!
//! Format research: the launcher registers itself and every installed
//! title under HKLM `SOFTWARE\Rockstar Games\<name>` (32-bit view; the
//! `WOW6432Node` path in the 64-bit view) with an `InstallFolder` value.
//! `Launcher` is the launcher's own key; Social Club keys belong to the
//! runtime. Titles start through `Launcher.exe -launchTitleInFolder
//! <install folder>`. The registry key name is the only stable id, so it
//! becomes `ExternalId::Rockstar`. Native verification is on the #71
//! checklist.

use super::{is_dir, is_file, join, registry_string, resolve};
use crate::{
    OperatingSystem,
    env::HostEnv,
    registry::{RegistryHive, RegistryView},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use std::{collections::BTreeMap, path::PathBuf};

pub const ID: SourceId = SourceId::new("rockstar");

const KEYS: [(RegistryView, &str); 2] = [
    (RegistryView::Registry32, r"SOFTWARE\Rockstar Games"),
    (
        RegistryView::Registry64,
        r"SOFTWARE\WOW6432Node\Rockstar Games",
    ),
];

/// Registry keys that are not games.
fn runtime_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "launcher" || name.contains("social club") || name == "rockstar games launcher"
}

/// `(key name, install folder, evidence key)` for every registered title.
fn titles(env: &HostEnv) -> Vec<(String, PathBuf, String)> {
    let registry = env.registry();
    let mut found = BTreeMap::new();
    for (view, path) in KEYS {
        let Ok(Some(key)) = registry.open_key(RegistryHive::LocalMachine, view, path) else {
            continue;
        };
        for name in registry.subkeys(&key).unwrap_or_default() {
            let sub = format!(r"{path}\{name}");
            if let Some(dir) =
                registry_string(env, RegistryHive::LocalMachine, view, &sub, "InstallFolder")
                    .and_then(|dir| resolve(env, &dir))
            {
                found
                    .entry(name.clone())
                    .or_insert((dir, format!(r"HKLM\{sub}")));
            }
        }
    }
    found
        .into_iter()
        .map(|(name, (dir, key))| (name, dir, key))
        .collect()
}

fn launcher(env: &HostEnv) -> Option<PathBuf> {
    titles(env)
        .into_iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case("launcher"))
        .map(|(_, dir, _)| join(&dir, "Launcher.exe"))
        .filter(|exe| is_file(env, exe))
}

#[derive(Debug, Default)]
pub struct RockstarLauncher;

impl Source for RockstarLauncher {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        match launcher(env) {
            Some(exe) => {
                let data = exe.parent().map(PathBuf::from);
                vec![LauncherStatus::found(
                    ID,
                    LauncherKind::Native,
                    Some(exe),
                    data,
                )]
            }
            None => vec![LauncherStatus::not_found(ID)],
        }
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let launcher = launcher(env);
        let mut games = Vec::new();
        for (name, dir, key) in titles(env) {
            ctx.check()?;
            if runtime_key(&name) {
                continue;
            }
            let mut game = DetectedGame::new(ID, name.clone(), name.clone());
            game.ids = vec![ExternalId::Rockstar {
                title: name.clone(),
            }];
            game.launch = launcher
                .iter()
                .map(|launcher| LaunchTarget::Exec {
                    program: launcher.clone(),
                    args: vec!["-launchTitleInFolder".into(), dir.display().to_string()],
                    cwd: None,
                    env: BTreeMap::new(),
                })
                .collect();
            game.runtime = if env.os() == OperatingSystem::Windows {
                GameRuntime::Windows {
                    prefix: None,
                    runner: None,
                }
            } else {
                GameRuntime::Native
            };
            game.state = if is_dir(env, &dir) {
                InstallState::Installed
            } else {
                InstallState::Broken {
                    reason: "install folder missing".into(),
                }
            };
            game.evidence = vec![Evidence::Registry {
                key,
                value: Some("InstallFolder".into()),
            }];
            game.tracking = Some(TrackingHint {
                steam_app_id: None,
                install_dir: Some(dir.clone()),
                exe_names: Vec::new(),
            });
            game.install_dir = Some(dir);
            games.push(game);
        }
        Ok(games)
    }

    /// The registry cannot be watched; the source is polled.
    fn watch_paths(&self, _env: &HostEnv) -> Vec<WatchPath> {
        Vec::new()
    }
}
