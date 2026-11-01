//! Riot Client (§5, #88): `%ProgramData%\Riot Games\RiotClientInstalls.json`
//! for the client, and `Metadata\<product>.<patchline>\
//! <product>.<patchline>.product_settings.yaml` for each installed game.
//! Layout from game-scanner (MIT) as documentation; the code is original.

use super::{is_dir, is_file, join, json, list_or_warn, read, resolve};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("riot");

/// Product ids and their display titles.
const TITLES: &[(&str, &str)] = &[
    ("valorant", "VALORANT"),
    ("league_of_legends", "League of Legends"),
    ("bacon", "Legends of Runeterra"),
    ("lion", "2XKO"),
];

fn root(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        let path = env
            .resolve_path(Path::new("/Users/Shared/Riot Games"))
            .ok()?;
        return is_dir(env, &path).then_some(path);
    }
    let program_data = env.known_folder(KnownFolder::ProgramData)?;
    Some(join(program_data, "Riot Games")).filter(|dir| is_dir(env, dir))
}

/// `RiotClientServices.exe` from `RiotClientInstalls.json`.
fn client(env: &HostEnv, root: &Path, ctx: &ScanCtx) -> Option<PathBuf> {
    let file = join(root, "RiotClientInstalls.json");
    if !is_file(env, &file) {
        return None;
    }
    let value = json(env, &file, 1 << 20)
        .map_err(|error| ctx.warn(error.to_string()))
        .ok()?;
    ["rc_default", "rc_live", "rc_beta"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .filter_map(|path| resolve(env, path))
        .find(|exe| is_file(env, exe))
}

#[derive(Debug, Default)]
pub struct RiotClient;

impl Source for RiotClient {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS.with(OperatingSystem::Macos)
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let Some(root) = root(env) else {
            return vec![LauncherStatus::not_found(ID)];
        };
        let client = client(env, &root, &ScanCtx::default());
        vec![LauncherStatus::found(
            ID,
            LauncherKind::Native,
            client,
            Some(root),
        )]
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let Some(root) = root(env) else {
            return Ok(Vec::new());
        };
        let client = client(env, &root, ctx);
        let metadata = join(&root, "Metadata");
        let mut games = BTreeMap::new();
        for folder in list_or_warn(env, &metadata, Some(ctx)) {
            ctx.check()?;
            let Some((product, patchline)) = folder.split_once('.') else {
                continue;
            };
            if product == "riot_client"
                || product.is_empty()
                || patchline.is_empty()
                || games.contains_key(product)
            {
                continue;
            }
            let file = join(
                &metadata,
                &format!("{folder}/{folder}.product_settings.yaml"),
            );
            if !is_file(env, &file) {
                continue;
            }
            let settings = match read(env, &file, 1 << 20)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&bytes)
                        .map_err(|error| error.to_string())
                }) {
                Ok(settings) => settings,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", file.display()));
                    continue;
                }
            };
            let text = |name: &str| {
                settings
                    .get(name)
                    .and_then(serde_yaml_ng::Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            };
            let Some(dir) = text("product_install_full_path").and_then(|path| resolve(env, path))
            else {
                ctx.warn(format!("{}: no install path", file.display()));
                continue;
            };
            let title = TITLES
                .iter()
                .find(|(id, _)| *id == product)
                .map(|(_, title)| (*title).to_owned())
                .or_else(|| {
                    text("shortcut_name").map(|name| name.trim_end_matches(".lnk").to_owned())
                })
                .unwrap_or_else(|| product.to_owned());
            let mut game = DetectedGame::new(ID, product, title);
            game.ids = vec![ExternalId::Riot {
                product: product.into(),
            }];
            game.launch = client
                .iter()
                .map(|client| LaunchTarget::Exec {
                    program: client.clone(),
                    args: vec![
                        format!("--launch-product={product}"),
                        format!("--launch-patchline={patchline}"),
                    ],
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
            game.evidence = vec![Evidence::File { path: file }];
            game.tracking = Some(TrackingHint {
                steam_app_id: None,
                install_dir: Some(dir.clone()),
                exe_names: Vec::new(),
            });
            game.install_dir = Some(dir);
            games.insert(product.to_owned(), game);
        }
        Ok(games.into_values().collect())
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        root(env)
            .map(|root| WatchPath {
                path: join(&root, "Metadata"),
                recursive: true,
            })
            .into_iter()
            .collect()
    }
}
