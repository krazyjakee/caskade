//! An explicit metadata allowlist. Login caches, saves and game bytes are absent.
use super::{DumpError, Result};
use crate::env::{HostEnv, KnownFolder};
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
enum Kind {
    Steam,
    Heroic,
    Legendary,
    Lutris,
    Epic,
    BattleNet,
    Amazon,
    Wine,
    OsMetadata,
    AppBundle,
}
fn roots(env: &HostEnv) -> Vec<(PathBuf, Kind)> {
    let mut roots = vec![];
    let home = env.home();
    if let Some(data) = env.known_folder(KnownFolder::DataHome) {
        roots.extend([
            (data.join("Steam"), Kind::Steam),
            (data.join("lutris"), Kind::Lutris),
        ]);
    }
    if let Some(config) = env.known_folder(KnownFolder::ConfigHome) {
        roots.extend([
            (config.join("heroic"), Kind::Heroic),
            (config.join("legendary"), Kind::Legendary),
            (config.join("lutris"), Kind::Lutris),
        ]);
    }
    for path in [
        ".steam/steam",
        "Library/Application Support/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
    ] {
        roots.push((home.join(path), Kind::Steam));
    }
    for path in [
        ".var/app/com.heroicgameslauncher.hgl/config/heroic",
        "Library/Application Support/heroic",
    ] {
        roots.push((home.join(path), Kind::Heroic));
    }
    roots.extend([
        (
            home.join(".var/app/net.lutris.Lutris/data/lutris"),
            Kind::Lutris,
        ),
        (home.join(".wine"), Kind::Wine),
    ]);
    if let Some(data) = env.known_folder(KnownFolder::DataHome) {
        roots.push((data.join("applications"), Kind::OsMetadata));
    }
    for folder in [
        KnownFolder::Desktop,
        KnownFolder::StartMenu,
        KnownFolder::CommonStartMenu,
    ] {
        if let Some(path) = env.known_folder(folder) {
            roots.push((path.to_owned(), Kind::OsMetadata));
        }
    }
    for folder in [KnownFolder::Applications, KnownFolder::UserApplications] {
        if let Some(path) = env.known_folder(folder) {
            roots.push((path.to_owned(), Kind::AppBundle));
        }
    }
    for folder in [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86] {
        if let Some(path) = env.known_folder(folder) {
            roots.push((path.join("Steam"), Kind::Steam));
        }
    }
    if let Some(data) = env.known_folder(KnownFolder::ProgramData) {
        roots.extend([
            (
                data.join("Epic/EpicGamesLauncher/Data/Manifests"),
                Kind::Epic,
            ),
            (data.join("Battle.net/Agent"), Kind::BattleNet),
            (data.join("Amazon Games/Data/Games/Sql"), Kind::Amazon),
        ]);
    }
    roots
}
fn allows(kind: Kind, relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or_default();
    match kind {
        Kind::Steam => {
            name == "libraryfolders.vdf"
                || matches!(relative, "config/config.vdf" | "config/loginusers.vdf")
                || relative.starts_with("steamapps/appmanifest_") && name.ends_with(".acf")
                || relative.starts_with("userdata/")
                    && matches!(name, "shortcuts.vdf" | "localconfig.vdf")
                || relative == "appcache/appinfo.vdf"
        }
        Kind::Heroic => {
            matches!(
                relative,
                "legendaryConfig/legendary/installed.json"
                    | "gog_store/installed.json"
                    | "nile_config/nile/installed.json"
                    | "sideload_apps/library.json"
            ) || relative.starts_with("GamesConfig/") && name.ends_with(".json")
        }
        Kind::Legendary => relative == "installed.json",
        Kind::Lutris => {
            name == "pga.db"
                || relative.starts_with("games/")
                    && (name.ends_with(".yml") || name.ends_with(".yaml"))
        }
        Kind::Epic => name.ends_with(".item"),
        Kind::BattleNet => relative == "product.db",
        Kind::Amazon => relative == "GameInstallInfo.sqlite",
        Kind::Wine => matches!(relative, "user.reg" | "system.reg"),
        Kind::OsMetadata => [".desktop", ".lnk", ".url"]
            .iter()
            .any(|suffix| name.ends_with(suffix)),
        Kind::AppBundle => relative.ends_with(".app/Contents/Info.plist"),
    }
}
fn descend(kind: Kind, relative: &str) -> bool {
    match kind {
        Kind::Steam => {
            matches!(relative, "steamapps" | "config" | "appcache" | "userdata")
                || relative.starts_with("userdata/") && relative.split('/').count() <= 3
        }
        Kind::Heroic => [
            "legendaryConfig",
            "gog_store",
            "nile_config",
            "sideload_apps",
            "GamesConfig",
        ]
        .iter()
        .any(|prefix| relative.starts_with(prefix)),
        Kind::Lutris => relative == "games",
        Kind::OsMetadata => true,
        Kind::AppBundle => relative.ends_with(".app") || relative.ends_with(".app/Contents"),
        _ => false,
    }
}
pub(super) fn files(env: &HostEnv) -> Result<Vec<PathBuf>> {
    let mut selected = BTreeSet::new();
    let mut visited = 0usize;
    let mut roots: VecDeque<_> = roots(env).into();
    let mut seen = BTreeSet::new();
    while let Some((base, kind)) = roots.pop_front() {
        let base = env.resolve_path(&base)?;
        if !seen.insert(base.clone()) {
            continue;
        }
        if seen.len() > 64 {
            return Err(DumpError::Limit("launcher/library roots"));
        }
        if !base.is_dir() {
            continue;
        }
        if matches!(kind, Kind::Steam) {
            for name in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
                let path = base.join(name);
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(meta) => meta,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                if metadata.is_symlink() {
                    continue;
                }
                if metadata.len() > super::MAX_FILE {
                    return Err(DumpError::Limit("libraryfolders bytes"));
                }
                let parsed = crate::parsers::text_vdf::parse(
                    &fs::read(path)?,
                    &crate::parsers::Limits::default(),
                )
                .map_err(|e| DumpError::Invalid(e.to_string()))?
                .value;
                use crate::parsers::binary_vdf::Value;
                if let Value::Object(top) = parsed {
                    for (_, value) in top {
                        if let Value::Object(entries) = value {
                            for (key, value) in entries {
                                if !key.bytes().all(|c| c.is_ascii_digit()) {
                                    continue;
                                }
                                let path = match &value {
                                    Value::String(path) => Some(path.as_str()),
                                    Value::Object(fields) => fields
                                        .iter()
                                        .find(|(key, _)| key.eq_ignore_ascii_case("path"))
                                        .and_then(|(_, value)| value.as_str()),
                                    _ => None,
                                };
                                if let Some(path) = path {
                                    roots.push_back((
                                        env.resolve_path(Path::new(path))?,
                                        Kind::Steam,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        walk(&base, &base, kind, 0, &mut visited, &mut selected)?;
    }
    Ok(selected.into_iter().collect())
}
fn walk(
    base: &Path,
    directory: &Path,
    kind: Kind,
    depth: usize,
    visited: &mut usize,
    files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if depth > 8 {
        return Err(DumpError::Limit("metadata directory depth"));
    }
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *visited += 1;
        if *visited > 20_000 {
            return Err(DumpError::Limit("metadata directory entries"));
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_symlink() {
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .expect("walk below base")
            .to_string_lossy()
            .replace('\\', "/");
        if metadata.is_dir() && descend(kind, &relative) {
            walk(base, &path, kind, depth + 1, visited, files)?;
        } else if metadata.is_file() && allows(kind, &relative) {
            files.insert(path);
        }
    }
    Ok(())
}
