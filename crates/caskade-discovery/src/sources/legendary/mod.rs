//! Legendary and Rare (§5, #75): Epic games installed with the Legendary
//! CLI, or with Rare, which uses Legendary's config.
//!
//! `installed.json` and `metadata/<app>.json` are read from their documented
//! layout; Legendary (GPL-3.0) was used as a format reference only. The
//! Heroic source reuses `installed` for Heroic's bundled Legendary config.

use super::{canonical, is_dir, is_file, join, json, json_text, plain_name, read, resolve};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, Prefix, RunnerHint, RunnerKind, ScanCtx, Source, SourceError,
        SourceId, TrackingHint, WatchPath,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("legendary");
/// Rare's launcher status; its games come from the `legendary` source.
pub const RARE: SourceId = SourceId::new("rare");
const RARE_FLATPAK: &str = "io.github.dummerle.rare";
const LIMIT: u64 = 16 << 20;

/// One `installed.json` record.
#[derive(Debug, Clone)]
pub(crate) struct EpicInstall {
    pub app_name: String,
    pub title: String,
    pub install_dir: Option<PathBuf>,
    pub executable: Option<String>,
    /// `Windows`, `Mac`, …
    pub platform: String,
    pub id: Option<ExternalId>,
    pub size: Option<u64>,
    pub file: PathBuf,
}

impl EpicInstall {
    pub fn windows(&self) -> bool {
        self.platform.eq_ignore_ascii_case("windows") || self.platform.eq_ignore_ascii_case("win32")
    }
}

/// The installed games of a Legendary config dir, without DLC.
pub(crate) fn installed(env: &HostEnv, config: &Path, ctx: &ScanCtx) -> Vec<EpicInstall> {
    let file = join(config, "installed.json");
    if !is_file(env, &file) {
        return Vec::new();
    }
    let value = match json(env, &file, LIMIT) {
        Ok(value) => value,
        Err(error) => {
            ctx.warn(error.to_string());
            return Vec::new();
        }
    };
    let Some(records) = value.as_object() else {
        ctx.warn(format!("{}: not an object", file.display()));
        return Vec::new();
    };
    let mut games = Vec::new();
    for (key, record) in records {
        let text = |name: &str| json_text(record, name);
        let app_name = text("app_name").unwrap_or(key);
        if !plain_name(app_name) {
            ctx.warn(format!("{}: invalid app name {app_name:?}", file.display()));
            continue;
        }
        if record.get("is_dlc").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let (namespace, item) = match (text("namespace"), text("catalog_item_id")) {
            (Some(namespace), Some(item)) => (Some(namespace.to_owned()), Some(item.to_owned())),
            _ => metadata_ids(env, config, app_name),
        };
        games.push(EpicInstall {
            app_name: app_name.to_owned(),
            title: text("title").unwrap_or(app_name).to_owned(),
            install_dir: text("install_path").and_then(|path| resolve(env, path)),
            executable: text("executable").map(str::to_owned),
            platform: text("platform").unwrap_or("Windows").to_owned(),
            id: namespace
                .zip(item)
                .map(|(namespace, catalog_item)| ExternalId::Epic {
                    namespace,
                    catalog_item,
                    app_name: app_name.to_owned(),
                }),
            size: record
                .get("install_size")
                .and_then(Value::as_u64)
                .filter(|size| *size > 0),
            file: file.clone(),
        });
    }
    games.sort_by(|a, b| a.app_name.cmp(&b.app_name));
    games
}

/// Namespace and catalog item from `metadata/<app>.json`.
fn metadata_ids(env: &HostEnv, config: &Path, app_name: &str) -> (Option<String>, Option<String>) {
    let file = join(config, &format!("metadata/{app_name}.json"));
    let Ok(value) = json(env, &file, LIMIT) else {
        return (None, None);
    };
    let text =
        |value: &Value, name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
    if let Some(meta) = value.get("metadata")
        && let (Some(namespace), Some(item)) = (text(meta, "namespace"), text(meta, "id"))
    {
        return (Some(namespace), Some(item));
    }
    let asset = value
        .get("asset_infos")
        .and_then(Value::as_object)
        .and_then(|assets| assets.get("Windows").or_else(|| assets.values().next()));
    match asset {
        Some(asset) => (text(asset, "namespace"), text(asset, "catalog_item_id")),
        None => (None, None),
    }
}

/// `wine_prefix` and `wine_executable` for an app (or `[default]`) from
/// Legendary's `config.ini`.
fn wine_settings(env: &HostEnv, config: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let Ok(bytes) = read(env, &join(config, "config.ini"), 1 << 20) else {
        return BTreeMap::new();
    };
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current = String::new();
    for line in String::from_utf8_lossy(&bytes).lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current = name.trim().to_owned();
        } else if let Some((key, value)) = line.split_once('=')
            && !line.starts_with(['#', ';'])
        {
            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    sections
}

/// A Legendary config dir and how it was installed.
struct Config {
    dir: PathBuf,
    kind: LauncherKind,
}

fn configs(env: &HostEnv) -> Vec<Config> {
    let home = env.home().to_path_buf();
    let mut native = Vec::new();
    if let Some(path) = env
        .var("LEGENDARY_CONFIG_PATH")
        .and_then(|path| path.into_string().ok())
    {
        native.extend(env.expand_path(&path).ok());
    }
    if env.os() == OperatingSystem::Linux
        && let Some(config) = env.known_folder(KnownFolder::ConfigHome)
    {
        native.push(join(config, "legendary"));
    }
    native.push(join(&home, ".config/legendary"));
    let rare = join(&home, &format!(".var/app/{RARE_FLATPAK}/config/legendary"));
    let mut seen = BTreeSet::new();
    native
        .into_iter()
        .map(|dir| (dir, LauncherKind::Native))
        .chain([(
            rare,
            LauncherKind::Flatpak {
                app_id: RARE_FLATPAK.into(),
            },
        )])
        .filter(|(dir, _)| {
            is_file(env, &join(dir, "installed.json")) || is_file(env, &join(dir, "user.json"))
        })
        .filter(|(dir, _)| seen.insert(canonical(env, dir)))
        .map(|(dir, kind)| Config { dir, kind })
        .collect()
}

/// Rare's own settings, native or Flatpak.
fn rare(env: &HostEnv) -> Vec<LauncherStatus> {
    let home = env.home();
    let mut found = Vec::new();
    let config = env
        .known_folder(KnownFolder::ConfigHome)
        .map_or_else(|| join(home, ".config"), Path::to_path_buf);
    let native = join(&config, "Rare");
    if is_file(env, &join(&native, "Rare.conf")) {
        found.push((native, LauncherKind::Native));
    }
    let flatpak = join(home, &format!(".var/app/{RARE_FLATPAK}"));
    if is_dir(env, &flatpak) {
        found.push((
            flatpak,
            LauncherKind::Flatpak {
                app_id: RARE_FLATPAK.into(),
            },
        ));
    }
    found
        .into_iter()
        .map(|(dir, kind)| LauncherStatus::found(RARE, kind, None, Some(dir)))
        .collect()
}

#[derive(Debug, Default)]
pub struct Legendary;

impl Source for Legendary {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let mut statuses: Vec<_> = configs(env)
            .into_iter()
            .map(|config| LauncherStatus::found(ID, config.kind, None, Some(config.dir)))
            .collect();
        if statuses.is_empty() {
            statuses.push(LauncherStatus::not_found(ID));
        }
        statuses.extend(rare(env));
        statuses
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games: BTreeMap<String, DetectedGame> = BTreeMap::new();
        for config in configs(env) {
            ctx.check()?;
            let wine = wine_settings(env, &config.dir);
            for install in installed(env, &config.dir, ctx) {
                if games.contains_key(&install.app_name) {
                    continue;
                }
                let mut game = game(ID, &install);
                let launch = match &config.kind {
                    LauncherKind::Flatpak { app_id } => LaunchTarget::Flatpak {
                        app_id: app_id.clone(),
                        args: vec!["launch".into(), install.app_name.clone()],
                    },
                    _ => LaunchTarget::Exec {
                        program: PathBuf::from("legendary"),
                        args: vec!["launch".into(), install.app_name.clone()],
                        cwd: None,
                        env: BTreeMap::new(),
                    },
                };
                game.launch = vec![launch];
                game.runtime = runtime(env, &install, &wine);
                games.insert(install.app_name.clone(), game);
            }
        }
        Ok(games.into_values().collect())
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        configs(env)
            .into_iter()
            .flat_map(|config| ["installed.json", "config.ini"].map(|name| join(&config.dir, name)))
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}

/// The parts of an Epic install that Heroic and Legendary report alike.
pub(crate) fn game(source: SourceId, install: &EpicInstall) -> DetectedGame {
    let mut game = DetectedGame::new(source, install.app_name.clone(), install.title.clone());
    game.ids = install.id.iter().cloned().collect();
    game.install_dir = install.install_dir.clone();
    game.size = install.size;
    game.evidence = vec![Evidence::File {
        path: install.file.clone(),
    }];
    let exe = install
        .executable
        .as_deref()
        .and_then(|exe| exe.rsplit(['/', '\\']).next())
        .filter(|name| !name.is_empty());
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: game.install_dir.clone(),
        exe_names: exe.map(str::to_owned).into_iter().collect(),
    });
    game
}

fn runtime(
    env: &HostEnv,
    install: &EpicInstall,
    wine: &BTreeMap<String, BTreeMap<String, String>>,
) -> GameRuntime {
    if !install.windows() {
        return GameRuntime::Native;
    }
    if env.os() == OperatingSystem::Windows {
        return GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
    }
    let setting = |key: &str| {
        wine.get(&install.app_name)
            .and_then(|section| section.get(key))
            .or_else(|| wine.get("default").and_then(|section| section.get(key)))
            .filter(|value| !value.is_empty())
    };
    GameRuntime::Windows {
        prefix: setting("wine_prefix")
            .and_then(|path| env.expand_path(path).ok())
            .map(|path| Prefix {
                path,
                arch: None,
                owner: Some(ID),
            }),
        runner: setting("wine_executable").map(|bin| RunnerHint {
            kind: if bin.to_ascii_lowercase().contains("proton") {
                RunnerKind::Proton
            } else {
                RunnerKind::Wine
            },
            name: None,
            path: resolve(env, bin),
        }),
    }
}
