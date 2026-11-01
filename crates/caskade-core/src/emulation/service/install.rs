//! Where an emulator install lives on this machine and the command it runs.

use crate::emulation::{EmulatorInstall, OsFolders};
use caskade_installer::OperatingSystem;
use std::path::{Path, PathBuf};

/// The OS this build runs on.
pub fn host_os() -> OperatingSystem {
    if cfg!(windows) {
        OperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        OperatingSystem::Macos
    } else {
        OperatingSystem::Linux
    }
}

/// The program an install runs and the tokens before the profile's
/// arguments (§5.1). Launches add `--filesystem` for Flatpaks when needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub prefix: Vec<String>,
}

pub fn command(install: &EmulatorInstall) -> Command {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    match install {
        EmulatorInstall::Folder { dir, executable } => Command {
            program: text(&dir.join(executable)),
            prefix: Vec::new(),
        },
        EmulatorInstall::Path { binary } => Command {
            program: binary.clone(),
            prefix: Vec::new(),
        },
        EmulatorInstall::Flatpak { app_id } => Command {
            program: "flatpak".into(),
            prefix: vec!["run".into(), app_id.clone()],
        },
        EmulatorInstall::AppImage { file } => Command {
            program: text(file),
            prefix: Vec::new(),
        },
        // Launches read CFBundleExecutable (#121); the bundle's name is the
        // usual value and good enough for a preview.
        EmulatorInstall::AppBundle { bundle } => Command {
            program: text(
                &bundle
                    .join("Contents/MacOS")
                    .join(bundle.file_stem().unwrap_or_default()),
            ),
            prefix: Vec::new(),
        },
    }
}

/// A binary on `$PATH`, or in `search_path` when one is set (tests).
pub fn which(binary: &str, search_path: Option<&[PathBuf]>) -> Option<PathBuf> {
    let dirs = match search_path {
        Some(dirs) => dirs.to_vec(),
        None => std::env::split_paths(&std::env::var_os("PATH")?).collect(),
    };
    dirs.iter().find_map(|dir| {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        cfg!(windows)
            .then(|| dir.join(format!("{binary}.exe")))
            .filter(|candidate| candidate.is_file())
    })
}

/// Flatpak installations, user first.
fn flatpak_app(app_id: &str, folders: &OsFolders) -> Option<PathBuf> {
    let user = folders
        .user_home
        .as_ref()
        .map(|home| home.join(".local/share/flatpak/app").join(app_id));
    let system = Some(PathBuf::from("/var/lib/flatpak/app").join(app_id));
    [user, system]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
}

/// The folder `{EmulatorDir}` names (§3.3): for a Flatpak, its data dir.
pub fn emulator_dir(
    install: &EmulatorInstall,
    folders: &OsFolders,
    search_path: Option<&[PathBuf]>,
) -> Option<PathBuf> {
    match install {
        EmulatorInstall::Folder { dir, .. } => Some(dir.clone()),
        EmulatorInstall::Path { binary } => {
            which(binary, search_path).and_then(|path| path.parent().map(Path::to_path_buf))
        }
        EmulatorInstall::Flatpak { app_id } => folders
            .user_home
            .as_ref()
            .map(|home| home.join(".var/app").join(app_id)),
        EmulatorInstall::AppImage { file } => file.parent().map(Path::to_path_buf),
        EmulatorInstall::AppBundle { bundle } => Some(bundle.clone()),
    }
}

/// Whether the install is still there.
pub fn installed(
    install: &EmulatorInstall,
    folders: &OsFolders,
    search_path: Option<&[PathBuf]>,
) -> bool {
    match install {
        EmulatorInstall::Folder { dir, executable } => dir.join(executable).is_file(),
        EmulatorInstall::Path { binary } => which(binary, search_path).is_some(),
        EmulatorInstall::Flatpak { app_id } => flatpak_app(app_id, folders).is_some(),
        EmulatorInstall::AppImage { file } => file.is_file(),
        EmulatorInstall::AppBundle { bundle } => bundle.is_dir(),
    }
}
