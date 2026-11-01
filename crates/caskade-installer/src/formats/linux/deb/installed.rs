//! A deb or rpm the system package manager installed: the files it owns, as
//! `dpkg-query -L` or `rpm -ql` list them, and the command that starts it.
use super::{PackageError, Result, common::package_name, desktop};
use crate::{Format, host::HostCaps};
use std::{
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Where packages put their commands.
const BIN_DIRS: [&str; 4] = [
    "/usr/games",
    "/usr/bin",
    "/usr/local/games",
    "/usr/local/bin",
];
const APPLICATIONS: &str = "/usr/share/applications";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLaunch {
    /// The desktop entry's name.
    pub title: Option<String>,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// The files the package database lists for `name`.
pub fn installed_files(host: &HostCaps, format: Format, name: &str) -> Result<Vec<PathBuf>> {
    package_name(name)?;
    let (query, list) = if format == Format::Deb {
        ("dpkg-query", "-L")
    } else {
        ("rpm", "-ql")
    };
    let program = host
        .packages
        .managers
        .get(query)
        .ok_or_else(|| PackageError::Unsupported(format!("{query} is not configured")))?;
    let output = Command::new(program)
        .args([list, "--", name])
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(PackageError::Invalid(format!(
            "{query} lists no files for {name}"
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/'))
        .map(PathBuf::from)
        .collect())
}

/// What a package's desktop entry runs (the one named after the package
/// first), else its binary named after it, else its only binary. Only a
/// command that exists and is executable counts. The listed paths are read
/// below `system` (`/` but in tests).
pub fn launch(system: &Path, files: &[PathBuf], name: &str) -> Option<PackageLaunch> {
    let mut entries: Vec<_> = files
        .iter()
        .filter(|path| {
            path.parent() == Some(Path::new(APPLICATIONS))
                && path.extension().is_some_and(|ext| ext == "desktop")
        })
        .collect();
    entries.sort_by_key(|path| path.file_stem() != Some(OsStr::new(name)));
    if let Some(launch) = entries
        .into_iter()
        .find_map(|entry| desktop_launch(system, entry, files))
    {
        return Some(launch);
    }
    let binaries: Vec<_> = files
        .iter()
        .filter(|path| in_bin_dir(path) && executable(system, path))
        .collect();
    binaries
        .iter()
        .find(|path| path.file_name() == Some(OsStr::new(name)))
        .or(binaries.first().filter(|_| binaries.len() == 1))
        .map(|program| PackageLaunch {
            title: None,
            program: program.to_path_buf(),
            args: vec![],
        })
}

fn desktop_launch(system: &Path, path: &Path, files: &[PathBuf]) -> Option<PackageLaunch> {
    let mut bytes = Vec::new();
    std::fs::File::open(below(system, path))
        .ok()?
        .take(1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let (mut section, mut exec, mut title) = (false, None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| section) else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Exec" => exec = Some(value),
            "Name" => title = Some(value).filter(|name| !name.is_empty()),
            "Type" if value != "Application" => return None,
            "NoDisplay" | "Hidden" if value == "true" => return None,
            _ => {}
        }
    }
    let (word, suffix) = desktop::first_word(exec?).ok()?;
    let args = desktop::exec_arguments(suffix)?;
    let program = if word.starts_with('/') {
        PathBuf::from(word)
    } else if word.contains('/') {
        return None;
    } else {
        // A bare command: the package's own binary of that name.
        files
            .iter()
            .find(|path| path.file_name() == Some(OsStr::new(&word)) && in_bin_dir(path))?
            .clone()
    };
    executable(system, &program).then(|| PackageLaunch {
        title: title.map(str::to_owned),
        program,
        args,
    })
}

fn in_bin_dir(path: &Path) -> bool {
    path.parent()
        .is_some_and(|dir| BIN_DIRS.iter().any(|bin| dir == Path::new(bin)))
}

fn below(system: &Path, path: &Path) -> PathBuf {
    system.join(path.strip_prefix("/").unwrap_or(path))
}

fn executable(system: &Path, path: &Path) -> bool {
    let path = below(system, path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn file(system: &Path, path: &str, text: &str, mode: u32) -> PathBuf {
        let real = below(system, Path::new(path));
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, text).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(mode)).unwrap();
        path.into()
    }

    fn launch_of(
        system: &Path,
        files: &[PathBuf],
    ) -> Option<(PathBuf, Vec<String>, Option<String>)> {
        launch(system, files, "game").map(|launch| (launch.program, launch.args, launch.title))
    }

    #[test]
    fn the_desktop_entry_names_the_command_and_title() {
        let system = tempfile::tempdir().unwrap();
        let system = system.path();
        let files = vec![
            file(system, "/usr/games/game", "", 0o755),
            file(system, "/usr/bin/game-editor", "", 0o755),
            file(
                system,
                "/usr/share/applications/editor.desktop",
                "[Desktop Entry]\nType=Application\nName=Game Editor\nExec=game-editor\n",
                0o644,
            ),
            file(
                system,
                "/usr/share/applications/game.desktop",
                "[Desktop Entry]\nType=Application\nName=The Game\nName[de]=Das Spiel\n\
                 Exec=game --fullscreen %U\n[Desktop Action Window]\nExec=game --window\n",
                0o644,
            ),
        ];
        assert_eq!(
            launch_of(system, &files),
            Some((
                "/usr/games/game".into(),
                vec!["--fullscreen".into()],
                Some("The Game".into())
            ))
        );
    }

    #[test]
    fn hidden_or_broken_entries_fall_back_to_the_binary_named_after_the_package() {
        let system = tempfile::tempdir().unwrap();
        let system = system.path();
        let files = vec![
            file(system, "/usr/bin/game", "", 0o755),
            file(system, "/usr/bin/game-server", "", 0o755),
            file(
                system,
                "/usr/share/applications/game.desktop",
                "[Desktop Entry]\nName=Game\nExec=game\nNoDisplay=true\n",
                0o644,
            ),
            file(
                system,
                "/usr/share/applications/other.desktop",
                "[Desktop Entry]\nName=Other\nExec=/opt/missing/run\n",
                0o644,
            ),
        ];
        assert_eq!(
            launch_of(system, &files),
            Some(("/usr/bin/game".into(), vec![], None))
        );
    }

    #[test]
    fn an_only_executable_binary_runs_and_ambiguity_runs_nothing() {
        let system = tempfile::tempdir().unwrap();
        let system = system.path();
        let readme = file(system, "/usr/bin/readme", "", 0o644);
        let start = file(system, "/usr/bin/start", "", 0o755);
        let lib = file(system, "/usr/lib/game/game", "", 0o755);
        assert_eq!(
            launch_of(system, &[readme.clone(), start.clone(), lib.clone()]),
            Some(("/usr/bin/start".into(), vec![], None))
        );
        let other = file(system, "/usr/games/other", "", 0o755);
        assert_eq!(launch_of(system, &[readme, start, lib, other]), None);
    }
}
