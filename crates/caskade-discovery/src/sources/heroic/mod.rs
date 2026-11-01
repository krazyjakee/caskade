//! Heroic (§5, #74): Epic (Legendary), GOG, Amazon (Nile) and sideloaded
//! games from every Heroic config dir (native, Flatpak, macOS, Windows).
//!
//! Files are read from their documented layout, with the older GOG and
//! Amazon library locations as fallbacks. Heroic (GPL-3.0) was used as a
//! format reference only; no code was copied.

pub(crate) mod time;

use super::{canonical, is_dir, is_file, join, json, json_text, legendary, plain_name, resolve};
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

pub const ID: SourceId = SourceId::new("heroic");
pub const FLATPAK_ID: &str = "com.heroicgameslauncher.hgl";
const LIMIT: u64 = 32 << 20;

/// A Heroic config dir and how Heroic was installed.
#[derive(Debug, Clone)]
struct Config {
    dir: PathBuf,
    kind: LauncherKind,
}

fn configs(env: &HostEnv) -> Vec<Config> {
    let mut seen = BTreeSet::new();
    candidates(env)
        .into_iter()
        .filter(|(dir, _)| is_dir(env, dir))
        .filter(|(dir, _)| seen.insert(canonical(env, dir)))
        .map(|(dir, kind)| Config { dir, kind })
        .collect()
}

/// Every place a Heroic config dir can be, installed or not.
fn candidates(env: &HostEnv) -> Vec<(PathBuf, LauncherKind)> {
    let home = env.home().to_path_buf();
    let mut native = Vec::new();
    if let Some(config) = env.known_folder(KnownFolder::ConfigHome) {
        native.push(join(config, "heroic"));
    }
    if env.os() == OperatingSystem::Windows {
        if let Some(app_data) = env.known_folder(KnownFolder::AppData) {
            native.push(join(app_data, "heroic"));
        }
    } else {
        native.push(join(&home, ".config/heroic"));
    }
    let flatpak = join(&home, &format!(".var/app/{FLATPAK_ID}/config/heroic"));
    native
        .into_iter()
        .map(|dir| (dir, LauncherKind::Native))
        .chain([(
            flatpak,
            LauncherKind::Flatpak {
                app_id: FLATPAK_ID.into(),
            },
        )])
        .collect()
}

/// Heroic's four libraries, as they appear in `heroic://launch/<runner>/…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Runner {
    Legendary,
    Gog,
    Nile,
    Sideload,
}

impl Runner {
    fn name(self) -> &'static str {
        match self {
            Self::Legendary => "legendary",
            Self::Gog => "gog",
            Self::Nile => "nile",
            Self::Sideload => "sideload",
        }
    }
}

/// One installed game in one of the libraries.
#[derive(Debug, Clone)]
struct Install {
    runner: Runner,
    app_name: String,
    title: String,
    install_dir: Option<PathBuf>,
    executable: Option<String>,
    windows: bool,
    ids: Vec<ExternalId>,
    size: Option<u64>,
    file: PathBuf,
}

#[derive(Debug, Default)]
pub struct Heroic;

impl Source for Heroic {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let statuses: Vec<_> = configs(env)
            .into_iter()
            .map(|config| LauncherStatus::found(ID, config.kind, None, Some(config.dir)))
            .collect();
        if statuses.is_empty() {
            vec![LauncherStatus::not_found(ID)]
        } else {
            statuses
        }
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games: BTreeMap<String, DetectedGame> = BTreeMap::new();
        for config in configs(env) {
            ctx.check()?;
            let played = time::played(env, &config.dir, ctx);
            let mut installs = epic(env, &config.dir, ctx);
            installs.extend(gog(env, &config.dir, ctx));
            installs.extend(nile(env, &config.dir, ctx));
            installs.extend(sideload(env, &config.dir, ctx));
            for install in installs {
                ctx.check()?;
                let key = format!("{}:{}", install.runner.name(), install.app_name);
                if games.contains_key(&key) {
                    continue;
                }
                let mut game = game(env, &config, &install, ctx);
                if let Some(played) = played.get(&install.app_name) {
                    game.playtime_secs = played.playtime_secs;
                    game.last_played_unix = played.last_played_unix;
                }
                game.key = key.clone();
                games.insert(key, game);
            }
        }
        Ok(games.into_values().collect())
    }

    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        candidates(env).into_iter().map(|(dir, _)| dir).collect()
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        configs(env)
            .iter()
            .flat_map(|config| {
                [
                    "legendaryConfig/legendary/installed.json",
                    "gog_store/installed.json",
                    "nile_config/nile/installed.json",
                    "sideload_apps/library.json",
                    "store/timestamp.json",
                ]
                .map(|relative| join(&config.dir, relative))
            })
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}

/// Loads an optional file; a missing file is not a warning.
fn load(env: &HostEnv, path: &Path, ctx: &ScanCtx) -> Option<Value> {
    if !is_file(env, path) {
        return None;
    }
    json(env, path, LIMIT)
        .map_err(|error| ctx.warn(error.to_string()))
        .ok()
}

fn epic(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> Vec<Install> {
    legendary::installed(env, &join(dir, "legendaryConfig/legendary"), ctx)
        .into_iter()
        .map(|install| Install {
            runner: Runner::Legendary,
            windows: install.windows(),
            ids: install.id.iter().cloned().collect(),
            app_name: install.app_name,
            title: install.title,
            install_dir: install.install_dir,
            executable: install.executable,
            size: install.size,
            file: install.file,
        })
        .collect()
}

/// `app_name` → title from Heroic's store cache and older library files.
fn titles(
    env: &HostEnv,
    dir: &Path,
    files: &[(&str, &str)],
    ctx: &ScanCtx,
) -> BTreeMap<String, String> {
    let mut titles = BTreeMap::new();
    for (relative, list) in files {
        let Some(value) = load(env, &join(dir, relative), ctx) else {
            continue;
        };
        let entries = if list.is_empty() {
            value.as_array()
        } else {
            value.get(*list).and_then(Value::as_array)
        };
        for entry in entries.into_iter().flatten() {
            // Store caches use `app_name`; Nile's own library nests `product`.
            let product = entry.get("product").unwrap_or(entry);
            let id = json_text(entry, "app_name").or_else(|| json_text(product, "id"));
            if let (Some(id), Some(title)) = (
                id,
                json_text(entry, "title").or_else(|| json_text(product, "title")),
            ) {
                titles
                    .entry(id.to_owned())
                    .or_insert_with(|| title.to_owned());
            }
        }
    }
    titles
}

fn gog(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> Vec<Install> {
    let file = join(dir, "gog_store/installed.json");
    let Some(value) = load(env, &file, ctx) else {
        return Vec::new();
    };
    let titles = titles(
        env,
        dir,
        &[
            ("store_cache/gog_library.json", "games"),
            ("gog_store/library.json", "games"),
        ],
        ctx,
    );
    let records = value.get("installed").and_then(Value::as_array);
    records
        .into_iter()
        .flatten()
        .filter(|record| record.get("is_dlc").and_then(Value::as_bool) != Some(true))
        .filter_map(|record| {
            let app_name = json_text(record, "appName")?;
            if !plain_name(app_name) {
                ctx.warn(format!("{}: invalid app name {app_name:?}", file.display()));
                return None;
            }
            let install_dir = json_text(record, "install_path").and_then(|path| resolve(env, path));
            let title = titles
                .get(app_name)
                .cloned()
                .or_else(|| json_text(record, "title").map(str::to_owned))
                .or_else(|| {
                    install_dir
                        .as_ref()?
                        .file_name()?
                        .to_str()
                        .map(str::to_owned)
                })?;
            Some(Install {
                runner: Runner::Gog,
                windows: json_text(record, "platform")
                    .is_some_and(|platform| platform.eq_ignore_ascii_case("windows")),
                ids: app_name
                    .parse()
                    .ok()
                    .map(|product_id| ExternalId::Gog { product_id })
                    .into_iter()
                    .collect(),
                app_name: app_name.to_owned(),
                title,
                install_dir,
                executable: json_text(record, "executable").map(str::to_owned),
                size: record.get("install_size").and_then(Value::as_u64),
                file: file.clone(),
            })
        })
        .collect()
}

fn nile(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> Vec<Install> {
    let file = join(dir, "nile_config/nile/installed.json");
    let Some(value) = load(env, &file, ctx) else {
        return Vec::new();
    };
    let titles = titles(
        env,
        dir,
        &[
            ("store_cache/nile_library.json", "library"),
            ("nile_config/nile/library.json", ""),
        ],
        ctx,
    );
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| {
            let id = json_text(record, "id")?;
            if !plain_name(id) {
                ctx.warn(format!("{}: invalid id {id:?}", file.display()));
                return None;
            }
            let install_dir = json_text(record, "path").and_then(|path| resolve(env, path));
            let title = titles.get(id).cloned().or_else(|| {
                install_dir
                    .as_ref()?
                    .file_name()?
                    .to_str()
                    .map(str::to_owned)
            })?;
            Some(Install {
                runner: Runner::Nile,
                windows: true,
                ids: vec![ExternalId::Amazon { id: id.to_owned() }],
                app_name: id.to_owned(),
                title,
                install_dir,
                executable: None,
                size: record.get("size").and_then(Value::as_u64),
                file: file.clone(),
            })
        })
        .collect()
}

fn sideload(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> Vec<Install> {
    let file = join(dir, "sideload_apps/library.json");
    let Some(value) = load(env, &file, ctx) else {
        return Vec::new();
    };
    value
        .get("games")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|record| record.get("is_installed").and_then(Value::as_bool) != Some(false))
        .filter_map(|record| {
            let app_name = json_text(record, "app_name")?;
            if !plain_name(app_name) {
                ctx.warn(format!("{}: invalid app name {app_name:?}", file.display()));
                return None;
            }
            let install = record.get("install").unwrap_or(&Value::Null);
            if install.get("is_dlc").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let platform = json_text(install, "platform").unwrap_or("");
            if platform.eq_ignore_ascii_case("browser") {
                return None;
            }
            let executable = json_text(install, "executable").map(str::to_owned);
            let install_dir = json_text(record, "folder_name")
                .and_then(|path| resolve(env, path))
                .or_else(|| {
                    let exe = resolve(env, executable.as_deref()?)?;
                    exe.parent().map(Path::to_path_buf)
                });
            Some(Install {
                runner: Runner::Sideload,
                windows: platform.eq_ignore_ascii_case("windows"),
                ids: Vec::new(),
                title: json_text(record, "title").unwrap_or(app_name).to_owned(),
                app_name: app_name.to_owned(),
                install_dir,
                executable,
                size: None,
                file: file.clone(),
            })
        })
        .collect()
}

fn game(env: &HostEnv, config: &Config, install: &Install, ctx: &ScanCtx) -> DetectedGame {
    let mut game = DetectedGame::new(ID, install.app_name.clone(), install.title.clone());
    game.ids = install.ids.clone();
    game.install_dir = install.install_dir.clone();
    game.size = install.size.filter(|size| *size > 0);
    let uri = format!(
        "heroic://launch/{}/{}",
        install.runner.name(),
        install.app_name
    );
    game.launch = match &config.kind {
        LauncherKind::Flatpak { app_id } => vec![
            LaunchTarget::Flatpak {
                app_id: app_id.clone(),
                args: vec!["--no-gui".into(), "--no-sandbox".into(), uri.clone()],
            },
            LaunchTarget::Uri { uri },
        ],
        _ => vec![LaunchTarget::Uri { uri }],
    };
    game.runtime = runtime(env, &config.dir, install, ctx);
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

/// Windows games on Linux and macOS run in the prefix and with the Wine
/// build that `GamesConfig/<app>.json` records.
fn runtime(env: &HostEnv, dir: &Path, install: &Install, ctx: &ScanCtx) -> GameRuntime {
    if !install.windows {
        return GameRuntime::Native;
    }
    if env.os() == OperatingSystem::Windows {
        return GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
    }
    let file = join(dir, &format!("GamesConfig/{}.json", install.app_name));
    let settings = load(env, &file, ctx);
    let settings = settings
        .as_ref()
        .and_then(|value| value.get(&install.app_name));
    let prefix = settings
        .and_then(|settings| json_text(settings, "winePrefix"))
        .and_then(|path| resolve(env, path))
        .map(|path| Prefix {
            path,
            arch: None,
            owner: Some(ID),
        });
    let runner = settings
        .and_then(|settings| settings.get("wineVersion"))
        .map(|wine| RunnerHint {
            kind: match json_text(wine, "type")
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("proton") => RunnerKind::Proton,
                Some("wine") => RunnerKind::Wine,
                Some("crossover") => RunnerKind::CrossOver,
                _ => RunnerKind::Other,
            },
            name: json_text(wine, "name").map(str::to_owned),
            path: json_text(wine, "bin").and_then(|path| resolve(env, path)),
        });
    GameRuntime::Windows { prefix, runner }
}
