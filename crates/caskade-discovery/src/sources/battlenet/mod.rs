//! Battle.net (§5, #87): Blizzard and Activision games from the Agent's
//! `ProgramData\Battle.net\Agent\product.db` (protobuf; products the Agent
//! marks as not installed are skipped), titled through
//! `data/launchers.toml` and the matching uninstall keys.
//!
//! The product.db layout follows game-scanner and Playnite BattleNetLibrary
//! (MIT) as documentation; Steam ROM Manager (GPL-3.0) was a format
//! reference only. The code is original.

use super::{UninstallEntry, is_dir, is_file, join, read, resolve, same_dir, uninstall_entries};
use crate::{
    OperatingSystem,
    data::Data,
    env::{HostEnv, KnownFolder},
    parsers::{Limits, protobuf},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use std::path::{Path, PathBuf};

pub const ID: SourceId = SourceId::new("battlenet");

/// The Agent and the Battle.net app are launchers, not games.
const LAUNCHER_CODES: &[&str] = &["agent", "bna", "battle.net"];

fn data_dir(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        let path = env
            .resolve_path(Path::new("/Users/Shared/Battle.net"))
            .ok()?;
        return is_dir(env, &path).then_some(path);
    }
    let program_data = env
        .known_folder(KnownFolder::ProgramData)
        .map(Path::to_path_buf)
        .or_else(|| resolve(env, r"C:\ProgramData"))?;
    Some(join(&program_data, "Battle.net")).filter(|dir| is_dir(env, dir))
}

fn client(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        return [KnownFolder::Applications, KnownFolder::UserApplications]
            .into_iter()
            .filter_map(|f| env.known_folder(f))
            .find_map(|p| super::macos_bundle_executable(env, &p.join("Battle.net.app")));
    }
    [KnownFolder::ProgramFilesX86, KnownFolder::ProgramFiles]
        .into_iter()
        .filter_map(|folder| env.known_folder(folder))
        .map(|folder| join(folder, "Battle.net/Battle.net.exe"))
        .find(|exe| is_file(env, exe))
}

#[derive(Debug, Default)]
pub struct BattleNet;

impl Source for BattleNet {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS.with(OperatingSystem::Macos)
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let (exe, data) = (client(env), data_dir(env));
        if exe.is_none() && data.is_none() {
            return vec![LauncherStatus::not_found(ID)];
        }
        vec![LauncherStatus::found(ID, LauncherKind::Native, exe, data)]
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let Some(file) = data_dir(env).map(|dir| join(&dir, "Agent/product.db")) else {
            return Ok(Vec::new());
        };
        if !is_file(env, &file) {
            return Ok(Vec::new());
        }
        let limits = Limits {
            max_input_bytes: 16 << 20,
            ..Limits::default()
        };
        let bytes = read(env, &file, limits.max_input_bytes as u64)
            .map_err(|error| SourceError::io(&file, error))?;
        let db = protobuf::parse_product_db(&bytes, &limits)
            .map_err(|error| SourceError::parse(&file, error))?;
        let uninstall = uninstall_entries(env);
        let mut games: Vec<DetectedGame> = Vec::new();
        for product in &db.products {
            ctx.check()?;
            let code = product.product_code.trim();
            if code.is_empty()
                || LAUNCHER_CODES.contains(&code)
                || LAUNCHER_CODES.contains(&product.uid.as_str())
            {
                continue;
            }
            // A product the Agent knows but has not (or no longer) installed.
            let state = product
                .cached_product_state
                .as_ref()
                .and_then(|state| state.base_product_state.as_ref());
            if state.is_some_and(|state| !state.installed) {
                continue;
            }
            let Some(path) = product
                .settings
                .as_ref()
                .map(|settings| settings.install_path.trim())
                .filter(|path| !path.is_empty())
            else {
                continue;
            };
            let Some(dir) = resolve(env, path) else {
                ctx.warn(format!(
                    "{}: unreadable install path for {code}",
                    file.display()
                ));
                continue;
            };
            if games.iter().any(|game| game.key == code) {
                continue;
            }
            games.push(game(env, &file, code, dir, &uninstall));
        }
        games.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(games)
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        data_dir(env)
            .map(|dir| WatchPath {
                path: join(&dir, "Agent/product.db"),
                recursive: false,
            })
            .into_iter()
            .collect()
    }
}

fn game(
    env: &HostEnv,
    file: &Path,
    code: &str,
    dir: PathBuf,
    uninstall: &[UninstallEntry],
) -> DetectedGame {
    let known = Data::builtin().ok().and_then(|data| {
        data.launchers
            .battlenet
            .iter()
            .find(|product| product.code == code)
    });
    let entry = uninstall.iter().find(|entry| {
        entry
            .install_location
            .as_deref()
            .is_some_and(|location| same_dir(location, &dir))
    });
    let title = known
        .map(|product| product.title.clone())
        .or_else(|| entry.and_then(|entry| entry.display_name.clone()))
        .or_else(|| dir.file_name()?.to_str().map(str::to_owned))
        .unwrap_or_else(|| code.to_owned());
    let mut game = DetectedGame::new(ID, code, title);
    game.ids = vec![ExternalId::BattleNet { code: code.into() }];
    game.launch = vec![LaunchTarget::Uri {
        uri: format!(
            "battlenet://{}",
            known.map_or(code, |product| product.launch.as_str())
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
    game.state = if is_dir(env, &dir) {
        InstallState::Installed
    } else {
        InstallState::Broken {
            reason: "install folder missing".into(),
        }
    };
    game.evidence = vec![Evidence::File {
        path: file.to_path_buf(),
    }];
    if let Some(entry) = entry {
        game.evidence.push(Evidence::Registry {
            key: entry.key.clone(),
            value: None,
        });
        game.uninstall = entry.info(env);
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: Some(dir.clone()),
        exe_names: Vec::new(),
    });
    game.install_dir = Some(dir);
    game
}
