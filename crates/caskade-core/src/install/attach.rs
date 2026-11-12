//! Installs that become another release of a game already in the library,
//! instead of a game of their own: a fan port of a game the user owns
//! (`crate::ports`), with the user's game file placed where it wants it.

use crate::model::{GameEntry, LaunchSpec};
use std::path::{Component, Path, PathBuf};

/// What an install attached to an existing game does after installing.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttachTo {
    /// The library game the install becomes a release of. When it is gone
    /// by the time the install finishes, the install gets a game of its own.
    pub game_id: String,
    /// The program to launch: a file name, or a path relative to the
    /// install folder. The shallowest match wins over the installer's own
    /// choice; without a match the installer's choice stays.
    pub executable: Option<String>,
    /// Files copied into the install folder.
    pub files: Vec<PlacedFile>,
    /// Arguments added after the launch target's own.
    pub args: Vec<String>,
}

/// A file copied into an install.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlacedFile {
    pub source: PathBuf,
    /// The folder inside the install it goes in, relative; empty for the
    /// install folder itself.
    pub into: PathBuf,
    /// The name it gets there; the source's own name when `None`.
    pub name: Option<String>,
}

/// Applies `attach` to the entry of the install in `dir`. Returns warnings
/// for what couldn't be done; the install itself stands either way.
pub(super) async fn apply(dir: &Path, attach: &AttachTo, entry: &mut GameEntry) -> Vec<String> {
    let mut warnings = Vec::new();
    entry.game_id = attach.game_id.clone();
    if let Some(executable) = &attach.executable {
        let dir = dir.to_path_buf();
        let wanted = executable.clone();
        match tokio::task::spawn_blocking(move || find_executable(&dir, &wanted)).await {
            Ok(Some(found)) => set_program(entry, found),
            _ => warnings.push(crate::tr!(
                "{program} isn't in the install; it starts what the installer chose",
                program = executable
            )),
        }
    }
    if !attach.args.is_empty()
        && let Some(LaunchSpec::Executable { args, .. }) = &mut entry.launch
    {
        args.extend(attach.args.iter().cloned());
    }
    for file in &attach.files {
        if let Err(error) = place(dir, file).await {
            warnings.push(crate::tr!(
                "{file} couldn't be copied into the install: {error}",
                file = file.source.display().to_string(),
                error = error.to_string()
            ));
        }
    }
    warnings
}

/// Points the entry's launch at `path` (relative to the install), keeping
/// its environment and running from the program's own folder.
fn set_program(entry: &mut GameEntry, path: PathBuf) {
    let cwd = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let env = match entry.launch.take() {
        Some(LaunchSpec::Executable { env, .. }) => env,
        _ => Default::default(),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path,
        args: Vec::new(),
        cwd,
        env,
    });
}

/// `wanted` relative to `dir` when it is there, otherwise the shallowest
/// file named like it (case ignored), first by name at each depth.
fn find_executable(dir: &Path, wanted: &str) -> Option<PathBuf> {
    let wanted = wanted.replace('\\', "/");
    let relative = relative_path(Path::new(wanted.trim_start_matches("./")))?;
    if dir.join(&relative).is_file() {
        return Some(relative);
    }
    let name = relative.file_name()?.to_string_lossy().to_lowercase();
    let mut level = vec![PathBuf::new()];
    for _ in 0..8 {
        let mut next = Vec::new();
        let mut found = Vec::new();
        for folder in &level {
            let Ok(entries) = std::fs::read_dir(dir.join(folder)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = folder.join(entry.file_name());
                // Caskade's own bookkeeping isn't the game.
                if path.starts_with(".caskade") || kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    next.push(path);
                } else if entry.file_name().to_string_lossy().to_lowercase() == name {
                    found.push(path);
                }
            }
        }
        if !found.is_empty() {
            found.sort();
            return found.into_iter().next();
        }
        if next.is_empty() {
            return None;
        }
        next.sort();
        level = next;
    }
    None
}

/// `path` when it stays inside the folder it is relative to.
fn relative_path(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

async fn place(dir: &Path, file: &PlacedFile) -> std::io::Result<()> {
    let invalid = |message: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
    let into = relative_path(&file.into).ok_or_else(|| invalid("the folder leaves the install"))?;
    let name = match &file.name {
        Some(name) => {
            let plain = Path::new(name);
            if plain.components().count() != 1
                || !matches!(plain.components().next(), Some(Component::Normal(_)))
            {
                return Err(invalid("the file name isn't a plain name"));
            }
            plain.as_os_str()
        }
        None => file
            .source
            .file_name()
            .ok_or_else(|| invalid("the file has no name"))?,
    };
    let folder = dir.join(into);
    tokio::fs::create_dir_all(&folder).await?;
    let target = folder.join(name);
    if tokio::fs::try_exists(&target).await? {
        return Ok(());
    }
    // A hard link costs nothing for a large disc image on the same drive.
    if tokio::fs::hard_link(&file.source, &target).await.is_err() {
        let partial = folder.join(format!(".{}.partial", name.to_string_lossy()));
        tokio::fs::copy(&file.source, &partial).await?;
        tokio::fs::rename(&partial, &target).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executables_are_found_by_path_or_shallowest_name() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for file in [
            "Game/bin/port.exe",
            "Game/Port.EXE",
            "other/deep/a/port.exe",
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
        assert_eq!(
            find_executable(root, "Game/bin/port.exe"),
            Some(PathBuf::from("Game/bin/port.exe"))
        );
        assert_eq!(
            find_executable(root, "port.exe"),
            Some(PathBuf::from("Game/Port.EXE"))
        );
        assert_eq!(find_executable(root, "missing.exe"), None);
        assert_eq!(find_executable(root, "../port.exe"), None);
    }

    #[tokio::test]
    async fn files_are_placed_inside_the_install_only() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        let rom = temp.path().join("Game (USA).z64");
        std::fs::write(&rom, b"rom").unwrap();
        place(
            &install,
            &PlacedFile {
                source: rom.clone(),
                into: "Game Files".into(),
                name: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(install.join("Game Files/Game (USA).z64")).unwrap(),
            b"rom"
        );
        assert!(
            place(
                &install,
                &PlacedFile {
                    source: rom.clone(),
                    into: "../outside".into(),
                    name: None,
                },
            )
            .await
            .is_err()
        );
        assert!(!temp.path().join("outside").exists());
        place(
            &install,
            &PlacedFile {
                source: rom.clone(),
                into: "".into(),
                name: Some("baserom.us.z64".into()),
            },
        )
        .await
        .unwrap();
        assert!(install.join("baserom.us.z64").is_file());
        assert!(
            place(
                &install,
                &PlacedFile {
                    source: rom,
                    into: "".into(),
                    name: Some("../escape.z64".into()),
                },
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn apply_attaches_points_the_launch_and_adds_arguments() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("Port")).unwrap();
        std::fs::write(temp.path().join("Port/port.x86_64"), b"").unwrap();
        let mut entry = GameEntry::new("new-game", crate::install::STORAGE_SOURCE, "id");
        entry.launch = Some(LaunchSpec::Executable {
            path: "setup.sh".into(),
            args: vec![],
            cwd: None,
            env: [("A".to_owned(), "1".to_owned())].into(),
        });
        let warnings = apply(
            temp.path(),
            &AttachTo {
                game_id: "owned".into(),
                executable: Some("port.x86_64".into()),
                files: vec![],
                args: vec!["/roms/game.z64".into()],
            },
            &mut entry,
        )
        .await;
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entry.game_id, "owned");
        assert_eq!(
            entry.launch,
            Some(LaunchSpec::Executable {
                path: "Port/port.x86_64".into(),
                args: vec!["/roms/game.z64".into()],
                cwd: Some("Port".into()),
                env: [("A".to_owned(), "1".to_owned())].into(),
            })
        );
    }
}
