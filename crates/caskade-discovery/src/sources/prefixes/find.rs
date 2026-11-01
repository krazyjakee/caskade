//! Where Wine-family prefixes live, and who owns them.

use super::super::{canonical, is_dir, is_file, join, json, list, read, resolve};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    types::{RunnerHint, RunnerKind, ScanCtx, SourceId},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// A prefix and what is known about who runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundPrefix {
    pub path: PathBuf,
    /// The launcher that manages the prefix, if any.
    pub owner: Option<SourceId>,
    pub runner: Option<RunnerHint>,
}

fn runner(kind: RunnerKind, name: Option<String>) -> Option<RunnerHint> {
    name.map(|name| RunnerHint {
        kind,
        name: Some(name),
        path: None,
    })
}

fn wine_or_proton(name: &str) -> RunnerKind {
    if name.to_ascii_lowercase().contains("proton") {
        RunnerKind::Proton
    } else {
        RunnerKind::Wine
    }
}

/// A Wine prefix: `system.reg` next to `drive_c`.
pub fn is_prefix(env: &HostEnv, path: &Path) -> bool {
    is_file(env, &join(path, "system.reg")) && is_dir(env, &join(path, "drive_c"))
}

/// Every prefix on the machine, owned ones first; each path once.
pub(crate) fn prefixes(env: &HostEnv, ctx: &ScanCtx) -> Vec<FoundPrefix> {
    let mut found = Vec::new();
    lutris(env, ctx, &mut found);
    heroic(env, ctx, &mut found);
    bottles(env, &mut found);
    for (path, tool) in super::super::steam::shortcut_prefixes(env, ctx) {
        let runner = tool.map(|tool| RunnerHint {
            kind: wine_or_proton(&tool),
            name: Some(tool),
            path: None,
        });
        found.push(FoundPrefix {
            path,
            owner: Some(crate::sources::steam::shortcuts::ID),
            runner,
        });
    }
    if env.os() == OperatingSystem::Macos {
        mac_bottles(env, &mut found);
    }
    if let Some(path) = env
        .var("WINEPREFIX")
        .and_then(|path| path.into_string().ok())
        .and_then(|path| env.expand_path(&path).ok())
    {
        found.push(FoundPrefix {
            path,
            owner: None,
            runner: None,
        });
    }
    found.push(FoundPrefix {
        path: join(env.home(), ".wine"),
        owner: None,
        runner: None,
    });
    let mut seen = BTreeSet::new();
    found
        .into_iter()
        .filter(|prefix| is_prefix(env, &prefix.path))
        .filter(|prefix| seen.insert(canonical(env, &prefix.path)))
        .collect()
}

/// Lutris game YAML: `game.prefix`, runner from `wine.version`.
fn lutris(env: &HostEnv, ctx: &ScanCtx, found: &mut Vec<FoundPrefix>) {
    let home = env.home();
    let config = env
        .known_folder(KnownFolder::ConfigHome)
        .map_or_else(|| join(home, ".config"), Path::to_path_buf);
    let data = env
        .known_folder(KnownFolder::DataHome)
        .map_or_else(|| join(home, ".local/share"), Path::to_path_buf);
    let dirs = [
        join(&config, "lutris/games"),
        join(&data, "lutris/games"),
        join(home, ".var/app/net.lutris.Lutris/config/lutris/games"),
        join(home, ".var/app/net.lutris.Lutris/data/lutris/games"),
    ];
    for dir in dirs {
        for name in list(env, &dir).unwrap_or_default() {
            if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
                continue;
            }
            let file = join(&dir, &name);
            let parsed = read(env, &file, 1 << 20)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&bytes)
                        .map_err(|error| error.to_string())
                });
            let value = match parsed {
                Ok(value) => value,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", file.display()));
                    continue;
                }
            };
            let text = |section: &str, key: &str| {
                value
                    .get(section)?
                    .get(key)?
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            let Some(path) = text("game", "prefix").and_then(|path| env.expand_path(&path).ok())
            else {
                continue;
            };
            let version = text("wine", "version");
            found.push(FoundPrefix {
                path,
                owner: Some(SourceId::new("lutris")),
                runner: version
                    .clone()
                    .and_then(|name| runner(wine_or_proton(&name), Some(name))),
            });
        }
    }
}

/// Heroic `GamesConfig/<app>.json` `winePrefix` and `wineVersion`.
fn heroic(env: &HostEnv, ctx: &ScanCtx, found: &mut Vec<FoundPrefix>) {
    let home = env.home();
    let config = env
        .known_folder(KnownFolder::ConfigHome)
        .map_or_else(|| join(home, ".config"), Path::to_path_buf);
    for dir in [
        join(&config, "heroic/GamesConfig"),
        join(
            home,
            ".var/app/com.heroicgameslauncher.hgl/config/heroic/GamesConfig",
        ),
    ] {
        for name in list(env, &dir).unwrap_or_default() {
            if !name.ends_with(".json") {
                continue;
            }
            let file = join(&dir, &name);
            let Ok(value) = json(env, &file, 4 << 20).map_err(|error| ctx.warn(error.to_string()))
            else {
                continue;
            };
            for settings in value
                .as_object()
                .into_iter()
                .flat_map(|object| object.values())
            {
                let Some(path) = settings
                    .get("winePrefix")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|path| resolve(env, path))
                else {
                    continue;
                };
                let wine = settings.get("wineVersion");
                let name = wine
                    .and_then(|wine| wine.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let kind = match wine
                    .and_then(|wine| wine.get("type"))
                    .and_then(serde_json::Value::as_str)
                {
                    Some("proton") => RunnerKind::Proton,
                    Some("crossover") => RunnerKind::CrossOver,
                    _ => RunnerKind::Wine,
                };
                found.push(FoundPrefix {
                    path,
                    owner: Some(crate::sources::heroic::ID),
                    runner: runner(kind, name),
                });
            }
        }
    }
}

/// Bottles keeps one prefix per bottle directory.
fn bottles(env: &HostEnv, found: &mut Vec<FoundPrefix>) {
    for bottle in super::super::bottles::bottles(env, None) {
        found.push(FoundPrefix {
            path: bottle.path,
            owner: Some(crate::sources::bottles::ID),
            runner: bottle.runner.map(|name| RunnerHint {
                kind: wine_or_proton(&name),
                name: Some(name),
                path: None,
            }),
        });
    }
}

/// CrossOver and Whisky bottles on macOS.
fn mac_bottles(env: &HostEnv, found: &mut Vec<FoundPrefix>) {
    let home = env.home();
    for (dir, kind) in [
        (
            "Library/Application Support/CrossOver/Bottles",
            RunnerKind::CrossOver,
        ),
        (
            "Library/Containers/com.isaacmarovitz.Whisky/Bottles",
            RunnerKind::Wine,
        ),
    ] {
        let dir = join(home, dir);
        for name in list(env, &dir).unwrap_or_default() {
            found.push(FoundPrefix {
                path: join(&dir, &name),
                owner: None,
                runner: Some(RunnerHint {
                    kind,
                    name: None,
                    path: None,
                }),
            });
        }
    }
}
