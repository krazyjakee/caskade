//! Ubisoft Connect installed games (#86). Original implementation, no ported
//! code. Format references: BoilR uplay/platform.rs (Apache-2.0) and Playnite
//! UplayLibrary (MIT): registry InstallDir, Uplay Install uninstall keys, and
//! configurations protobuf/YAML. Only local installed records are enumerated.
pub mod cache;
use super::{
    REGISTRY_VIEWS, expand, is_dir, is_file, join, read, registry_string, uninstall_entries,
};
use crate::{HostEnv, env::KnownFolder, registry::RegistryKey, types::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub const ID: SourceId = SourceId::new("ubisoft");
const INSTALLS: &str = r"SOFTWARE\Ubisoft\Launcher\Installs";
#[derive(Debug, Default)]
pub struct UbisoftConnect;
fn product_id(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|id| *id > 0)
}
fn uninstall_id(key: &str) -> Option<u64> {
    let name = key.rsplit(['\\', '/']).next()?;
    let (prefix, id) = name.split_at_checked("Uplay Install ".len())?;
    prefix
        .eq_ignore_ascii_case("Uplay Install ")
        .then(|| product_id(id))
        .flatten()
}
fn clients(env: &HostEnv) -> Vec<PathBuf> {
    let mut dirs = BTreeSet::new();
    for (hive, view) in REGISTRY_VIEWS {
        for name in ["InstallDir", "InstallLocation"] {
            if let Some(raw) = registry_string(env, hive, view, r"SOFTWARE\Ubisoft\Launcher", name)
                && let Some(path) = expand(env, &raw)
            {
                dirs.insert(path);
            }
        }
    }
    for entry in uninstall_entries(env) {
        if entry.display_name.as_deref().is_some_and(|s| {
            s.eq_ignore_ascii_case("Ubisoft Connect") || s.eq_ignore_ascii_case("Uplay")
        }) && let Some(path) = entry.install_location
        {
            dirs.insert(path);
        }
    }
    for folder in [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86]
        .into_iter()
        .filter_map(|f| env.known_folder(f))
    {
        dirs.insert(join(folder, "Ubisoft/Ubisoft Game Launcher"));
    }
    let mut paths = BTreeSet::new();
    for dir in dirs {
        for file in ["UbisoftConnect.exe", "upc.exe", "Uplay.exe"] {
            let path = dir.join(file);
            if is_file(env, &path) {
                paths.insert(path);
                break;
            }
        }
    }
    paths.into_iter().collect()
}
fn cache_paths(env: &HostEnv) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(local) = env.known_folder(KnownFolder::LocalAppData) {
        result.push(local.join("Ubisoft Game Launcher/cache/configuration/configurations"));
    }
    for client in clients(env) {
        if let Some(parent) = client.parent() {
            let path = parent.join("cache/configuration/configurations");
            if !result.contains(&path) {
                result.push(path);
            }
        }
    }
    result
}
fn titles(env: &HostEnv, ctx: &ScanCtx) -> Result<BTreeMap<u64, (String, PathBuf)>, SourceError> {
    let mut result = BTreeMap::new();
    for path in cache_paths(env) {
        ctx.check()?;
        if !is_file(env, &path) {
            continue;
        }
        match read(env, &path, 8 << 20).and_then(|b| cache::parse_configurations(&b)) {
            Ok(cache) => {
                for warning in cache.warnings {
                    ctx.warn(warning);
                }
                for game in cache.games {
                    // InstallId is the registry identity; UplayId is a fallback
                    // title key only, never a reason to emit uninstalled games.
                    for id in [game.install_id, game.id] {
                        result
                            .entry(id)
                            .or_insert((game.title.clone(), path.clone()));
                    }
                }
            }
            Err(e) => ctx.warn(format!("{}: {e}", path.display())),
        }
    }
    Ok(result)
}
#[derive(Default)]
struct Installed {
    dir: Option<PathBuf>,
    evidence: Vec<Evidence>,
}
impl Source for UbisoftConnect {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let clients = clients(env);
        if clients.is_empty() {
            return vec![LauncherStatus::not_found(ID)];
        }
        clients
            .into_iter()
            .map(|path| {
                let data = path.parent().map(|p| p.to_owned());
                LauncherStatus::found(ID, LauncherKind::Native, Some(path), data)
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let cached = titles(env, ctx)?;
        let mut installed = BTreeMap::<u64, Installed>::new();
        for (hive, view) in REGISTRY_VIEWS {
            ctx.check()?;
            let root = RegistryKey::new(hive, view, INSTALLS);
            let names = match env.registry().subkeys(&root) {
                Ok(n) => n,
                Err(e) => {
                    ctx.warn(format!("Ubisoft install keys: {e}"));
                    continue;
                }
            };
            if names.len() > 10_000 {
                ctx.warn("Ubisoft registry entry budget reached");
            }
            for name in names.into_iter().take(10_000) {
                ctx.check()?;
                let Some(id) = product_id(&name) else {
                    ctx.warn("invalid Ubisoft install id");
                    continue;
                };
                let key = format!("{INSTALLS}\\{name}");
                let Some(raw) = registry_string(env, hive, view, &key, "InstallDir") else {
                    continue;
                };
                let Some(dir) = expand(env, &raw) else {
                    ctx.warn("invalid Ubisoft install directory");
                    continue;
                };
                let entry = installed.entry(id).or_default();
                if entry.dir.is_none()
                    || entry.dir.as_ref().is_some_and(|p| !is_dir(env, p)) && is_dir(env, &dir)
                {
                    entry.dir = Some(dir);
                }
                entry.evidence.push(Evidence::Registry {
                    key: format!("{hive:?}\\{key}"),
                    value: Some("InstallDir".into()),
                });
            }
        }
        let mut uninstalls = BTreeMap::new();
        for uninstall in uninstall_entries(env) {
            ctx.check()?;
            let Some(id) = uninstall_id(&uninstall.key) else {
                continue;
            };
            if let Some(dir) = &uninstall.install_location {
                let entry = installed.entry(id).or_default();
                if entry.dir.is_none() {
                    entry.dir = Some(dir.clone());
                }
            }
            uninstalls.entry(id).or_insert(uninstall);
        }
        let mut games = Vec::new();
        for (id, installed) in installed {
            ctx.check()?;
            let Some(dir) = installed.dir else {
                continue;
            };
            let uninstall = uninstalls.get(&id);
            let title = uninstall
                .and_then(|u| u.display_name.as_deref())
                .filter(|t| !t.trim().is_empty())
                .or_else(|| cached.get(&id).map(|(title, _)| title.as_str()))
                .or_else(|| dir.file_name().and_then(|n| n.to_str()))
                .unwrap_or("Ubisoft game");
            let mut game = DetectedGame::new(ID, id.to_string(), title);
            game.ids = vec![ExternalId::Ubisoft { id }];
            game.install_dir = Some(dir.clone());
            game.runtime = GameRuntime::Windows {
                prefix: None,
                runner: None,
            };
            game.launch = vec![LaunchTarget::Uri {
                uri: format!("uplay://launch/{id}/0"),
            }];
            game.evidence = installed.evidence;
            if let Some((_, path)) = cached.get(&id) {
                game.evidence.push(Evidence::File { path: path.clone() });
            }
            if let Some(uninstall) = uninstall {
                game.evidence.push(Evidence::Registry {
                    key: uninstall.key.clone(),
                    value: Some("DisplayName".into()),
                });
                game.uninstall = uninstall.info(env);
            }
            if !is_dir(env, &dir) {
                game.state = InstallState::Broken {
                    reason: "Ubisoft install folder missing".into(),
                };
                game.confidence = Confidence::Maybe;
            }
            game.tracking = Some(TrackingHint {
                steam_app_id: None,
                install_dir: Some(dir),
                exe_names: vec![],
            });
            games.push(game);
        }
        Ok(games)
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86]
            .into_iter()
            .filter_map(|f| env.known_folder(f))
            .map(|p| p.join("Ubisoft/Ubisoft Game Launcher/UbisoftConnect.exe"))
            .collect()
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        cache_paths(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}
