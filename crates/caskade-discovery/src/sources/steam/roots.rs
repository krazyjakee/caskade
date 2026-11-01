//! Steam installs: native, Flatpak and Snap on Linux, the registry and
//! default folders on Windows, Application Support on macOS. In live mode
//! `steamlocate` adds whatever it finds.

use super::{
    super::{canonical, is_dir, join, list, registry_string, resolve},
    vdf,
};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    registry::{RegistryHive, RegistryView},
    types::LauncherKind,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) const FLATPAK_ID: &str = "com.valvesoftware.Steam";

#[derive(Debug, Clone)]
pub(super) struct Root {
    /// As the machine reports it.
    pub path: PathBuf,
    pub kind: LauncherKind,
}

/// Every distinct Steam install, native first, then Flatpak, then Snap.
pub(super) fn roots(env: &HostEnv) -> Vec<Root> {
    let mut seen = BTreeSet::new();
    let mut roots: Vec<Root> = candidates(env)
        .into_iter()
        .filter(|path| is_dir(env, &join(path, "steamapps")))
        .filter(|path| seen.insert(canonical(env, path)))
        .map(|path| Root {
            kind: kind(&path),
            path,
        })
        .collect();
    roots.sort_by_key(|root| match root.kind {
        LauncherKind::Native => 0,
        LauncherKind::Flatpak { .. } => 1,
        LauncherKind::Snap { .. } => 2,
        LauncherKind::Wine { .. } => 3,
    });
    roots
}

fn kind(path: &Path) -> LauncherKind {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.contains(&format!("/.var/app/{FLATPAK_ID}/")) {
        LauncherKind::Flatpak {
            app_id: FLATPAK_ID.into(),
        }
    } else if text.contains("/snap/steam/") {
        LauncherKind::Snap {
            name: "steam".into(),
        }
    } else {
        LauncherKind::Native
    }
}

pub(super) fn candidates(env: &HostEnv) -> Vec<PathBuf> {
    let home = env.home().to_path_buf();
    let mut paths = Vec::new();
    match env.os() {
        OperatingSystem::Linux => {
            let data_home = env
                .known_folder(KnownFolder::DataHome)
                .map_or_else(|| join(&home, ".local/share"), Path::to_path_buf);
            paths.push(join(&data_home, "Steam"));
            for relative in [
                ".local/share/Steam",
                ".steam/steam",
                ".steam/root",
                ".steam/debian-installation",
            ] {
                paths.push(join(&home, relative));
            }
            let flatpak = join(&home, &format!(".var/app/{FLATPAK_ID}"));
            paths.push(join(&flatpak, ".local/share/Steam"));
            paths.push(join(&flatpak, ".steam/steam"));
            let snap = join(&home, "snap/steam/common");
            paths.push(join(&snap, ".local/share/Steam"));
            paths.push(join(&snap, ".steam/steam"));
        }
        OperatingSystem::Macos => {
            paths.push(join(&home, "Library/Application Support/Steam"));
        }
        OperatingSystem::Windows => {
            paths.extend(registry_paths(env));
            for folder in [KnownFolder::ProgramFilesX86, KnownFolder::ProgramFiles] {
                if let Some(folder) = env.known_folder(folder) {
                    paths.push(join(folder, "Steam"));
                }
            }
            paths.extend(resolve(env, r"C:\Program Files (x86)\Steam"));
        }
    }
    if !env.is_fixture() {
        paths.extend(
            steamlocate::locate_all()
                .into_iter()
                .flatten()
                .map(|dir| dir.path().to_path_buf()),
        );
    }
    paths
}

/// `SteamPath` (current user) and `InstallPath` (machine, 32-bit view).
fn registry_paths(env: &HostEnv) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for (hive, view, key, name) in [
        (
            RegistryHive::CurrentUser,
            RegistryView::Native,
            r"Software\Valve\Steam",
            "SteamPath",
        ),
        (
            RegistryHive::LocalMachine,
            RegistryView::Registry32,
            r"SOFTWARE\Valve\Steam",
            "InstallPath",
        ),
        (
            RegistryHive::LocalMachine,
            RegistryView::Registry64,
            r"SOFTWARE\Valve\Steam",
            "InstallPath",
        ),
    ] {
        if let Some(path) = registry_string(env, hive, view, key, name) {
            paths.extend(resolve(env, &path.replace('/', "\\")));
        }
    }
    paths
}

/// The client version from `package/steam_client_*.manifest`.
pub(super) fn version(env: &HostEnv, root: &Path) -> Option<String> {
    let package = join(root, "package");
    let name = list(env, &package)
        .ok()?
        .into_iter()
        .find(|name| name.starts_with("steam_client_") && name.ends_with(".manifest"))?;
    let (manifest, _) = vdf::load(env, &join(&package, &name), &vdf::small()).ok()?;
    let (_, body) = vdf::entries(&manifest).first()?;
    vdf::text(body, "version").map(str::to_owned)
}
