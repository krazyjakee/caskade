//! Turning a downloaded release into an install: unpack it, lift a lone
//! top folder out, find the emulator with its definition's detect rules,
//! and carry the user's files across when it replaces an older version.

use crate::{
    Error, Result,
    emulation::{DetectRules, EmulatorInstall, OperatingSystem},
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The files a release shipped, relative to the install folder.
pub(super) const SHIPPED: &str = ".caskade/shipped.json";
/// User files a newer release also ships go here, per old version.
pub(super) const BACKUP: &str = ".caskade/update-backup";

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_archive(name: &str) -> bool {
    [
        ".zip", ".7z", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.zst", ".tar.bz2",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

/// Unpacks `file` into the empty folder `into`: archives are extracted,
/// disk images (macOS) have their app copied out, anything else (an
/// AppImage, a bare binary) is copied in and made executable.
pub(super) fn unpack(file: &Path, into: &Path) -> Result<()> {
    fs::create_dir_all(into)?;
    let name = lower_name(file);
    if is_archive(&name) {
        extract(file, into)?;
    } else if name.ends_with(".dmg") {
        dmg(file, into)?;
    } else if name.ends_with(".exe") && extract(file, into).is_ok() {
        // A self-extracting archive (MAME); a plain program is copied.
    } else {
        // What a failed extraction left.
        fs::remove_dir_all(into)?;
        fs::create_dir_all(into)?;
        let target = into.join(
            file.file_name()
                .ok_or_else(|| invalid(crate::tr!("no file name")))?,
        );
        fs::copy(file, &target)?;
        executable(&target)?;
    }
    flatten(into)
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    fs::create_dir_all(into)?;
    use caskade_installer::{
        formats::archive::{self, ArchiveOptions, NoopControl},
        safety::{Limits, SafeDir, open_staging},
    };
    let packed = fs::metadata(archive)?.len();
    let root = open_staging(into).map_err(|error| invalid(error.to_string()))?;
    let case_sensitive = !cfg!(any(windows, target_os = "macos"));
    let mut dir = SafeDir::new(root, Limits::default(), packed, case_sensitive);
    archive::extract_path(
        archive,
        &mut dir,
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .map_err(|error| {
        invalid(crate::tr!(
            "couldn't unpack {archive}: {error}",
            archive = archive.display(),
            error = error
        ))
    })?;
    Ok(())
}

/// Copies the apps out of a disk image.
#[cfg(target_os = "macos")]
fn dmg(image: &Path, into: &Path) -> Result<()> {
    use std::process::Command;
    let mut name = into.file_name().unwrap_or_default().to_owned();
    name.push(".mount");
    let mount = into.with_file_name(name);
    fs::create_dir_all(&mount)?;
    let status = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(image)
        .status()?;
    if !status.success() {
        return Err(invalid(crate::tr!("couldn't open the disk image")));
    }
    let copied = (|| {
        let mut found = false;
        for entry in fs::read_dir(&mount)? {
            let path = entry?.path();
            if lower_name(&path).ends_with(".app") {
                let status = Command::new("ditto")
                    .arg(&path)
                    .arg(into.join(path.file_name().unwrap_or_default()))
                    .status()?;
                found |= status.success();
            }
        }
        if found {
            Ok(())
        } else {
            Err(invalid(crate::tr!("the disk image has no app")))
        }
    })();
    let _ = Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mount)
        .status();
    let _ = fs::remove_dir(&mount);
    copied
}

#[cfg(not(target_os = "macos"))]
fn dmg(_image: &Path, _into: &Path) -> Result<()> {
    Err(invalid(crate::tr!("disk images open only on macOS")))
}

#[cfg(unix)]
fn executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    Ok(fs::set_permissions(path, permissions)?)
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Lifts the contents of a lone top folder (not an app bundle) into `dir`.
fn flatten(dir: &Path) -> Result<()> {
    loop {
        let entries: Vec<PathBuf> = fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        let [only] = entries.as_slice() else {
            return Ok(());
        };
        if !only.is_dir() || lower_name(only).ends_with(".app") {
            return Ok(());
        }
        // Moved aside first: it may hold an entry of its own name.
        let aside = dir.join(".caskade-flatten");
        fs::rename(only, &aside)?;
        for entry in fs::read_dir(&aside)? {
            let entry = entry?;
            fs::rename(entry.path(), dir.join(entry.file_name()))?;
        }
        fs::remove_dir(&aside)?;
    }
}

/// Every file under `dir`, relative, skipping Caskade's own folder.
pub(super) fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(dir.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            if path == Path::new(".caskade") {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The install in `dir` that `rules` describe on `os`, shallowest first:
/// an AppImage (Linux), an app bundle (macOS), or the executable.
pub(super) fn locate(
    dir: &Path,
    rules: &DetectRules,
    os: OperatingSystem,
) -> Option<EmulatorInstall> {
    let windows = os == OperatingSystem::Windows;
    let regex = |pattern: &Option<String>, insensitive: bool| {
        pattern.as_deref().and_then(|pattern| {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(insensitive)
                .build()
                .ok()
        })
    };
    let appimage = regex(&rules.appimage, true);
    let executable = regex(&rules.executable, windows);
    // Breadth first, four levels deep, so the shallowest match wins.
    let mut level = vec![dir.to_path_buf()];
    let mut bundles = Vec::new();
    let mut executables = Vec::new();
    let mut appimages = Vec::new();
    for _ in 0..4 {
        let mut next = Vec::new();
        for folder in &level {
            let Ok(entries) = fs::read_dir(folder) else {
                continue;
            };
            let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
            entries.sort();
            for path in entries {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name == ".caskade" {
                    continue;
                }
                if path.is_dir() {
                    if os == OperatingSystem::Macos && name.to_lowercase().ends_with(".app") {
                        bundles.push(path);
                    } else {
                        next.push(path);
                    }
                    continue;
                }
                if os == OperatingSystem::Linux && name.to_lowercase().ends_with(".appimage") {
                    let matched = appimage.as_ref().is_none_or(|regex| regex.is_match(&name));
                    appimages.push((matched, path));
                } else if executable
                    .as_ref()
                    .is_some_and(|regex| regex.is_match(&name))
                {
                    executables.push(path);
                }
            }
        }
        level = next;
    }
    // An AppImage the rules name, then any AppImage when it's the only one.
    let appimage = appimages
        .iter()
        .find(|(matched, _)| *matched)
        .or_else(|| (appimages.len() == 1).then(|| &appimages[0]))
        .map(|(_, path)| path.clone());
    if let Some(file) = appimage {
        let _ = executable_bit(&file);
        return Some(EmulatorInstall::AppImage { file });
    }
    if let Some(bundle) = bundles.into_iter().next() {
        return Some(EmulatorInstall::AppBundle { bundle });
    }
    let found = executables.into_iter().next()?;
    let _ = executable_bit(&found);
    Some(EmulatorInstall::Folder {
        dir: found.parent()?.to_path_buf(),
        executable: PathBuf::from(found.file_name()?),
    })
}

fn executable_bit(path: &Path) -> Result<()> {
    executable(path)
}

/// Moves `install` from `from` to `to`, keeping its relative path.
pub(super) fn rebase(install: EmulatorInstall, from: &Path, to: &Path) -> EmulatorInstall {
    // `join("")` would add a trailing separator.
    let moved = |path: PathBuf| match path.strip_prefix(from) {
        Ok(relative) if relative.as_os_str().is_empty() => to.to_path_buf(),
        Ok(relative) => to.join(relative),
        Err(_) => path,
    };
    match install {
        EmulatorInstall::Folder { dir, executable } => EmulatorInstall::Folder {
            dir: moved(dir),
            executable,
        },
        EmulatorInstall::AppImage { file } => EmulatorInstall::AppImage { file: moved(file) },
        EmulatorInstall::AppBundle { bundle } => EmulatorInstall::AppBundle {
            bundle: moved(bundle),
        },
        other => other,
    }
}

/// Puts the file named `name` from `download` at `to`: the file itself, or
/// the one of that name inside a `.zip`.
pub(super) fn take(download: &Path, name: &str, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if !lower_name(download).ends_with(".zip") {
        fs::rename(download, to)?;
        return Ok(());
    }
    let mut unpacked = download.as_os_str().to_owned();
    unpacked.push(".d");
    let unpacked = PathBuf::from(unpacked);
    extract(download, &unpacked)?;
    let found = files(&unpacked)?
        .into_iter()
        .find(|path| path.file_name().is_some_and(|file| file == name))
        .ok_or_else(|| invalid(crate::tr!("the download has no {file}", file = name)))?;
    // Moved, or copied when the cores folder is on another drive.
    if fs::rename(unpacked.join(&found), to).is_err() {
        fs::copy(unpacked.join(&found), to)?;
    }
    let _ = fs::remove_dir_all(&unpacked);
    Ok(())
}

/// Records the files a release shipped.
pub(super) fn record_shipped(dir: &Path) -> Result<()> {
    let shipped: Vec<String> = files(dir)?
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let path = dir.join(SHIPPED);
    fs::create_dir_all(path.parent().unwrap_or(dir))?;
    fs::write(path, serde_json::to_vec(&shipped)?)?;
    Ok(())
}

fn shipped(dir: &Path) -> BTreeSet<PathBuf> {
    fs::read(dir.join(SHIPPED))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Moves what the user added to the old install (configs, saves, cores,
/// BIOS files: everything the old release didn't ship) into the new one.
/// Where the new release ships the same path, its file stays and the
/// user's goes to [`BACKUP`]`/<old version>/`. Earlier backups come along.
/// Returns how many files were kept.
pub(super) fn carry(old: &Path, new: &Path, old_version: &str) -> Result<usize> {
    let shipped = shipped(old);
    let mut kept = 0;
    for relative in files(old)? {
        if shipped.contains(&relative) {
            continue;
        }
        let source = old.join(&relative);
        let mut target = new.join(&relative);
        if target.exists() {
            let version = sanitize(old_version);
            target = new.join(BACKUP).join(version).join(&relative);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&source, &target)?;
        kept += 1;
    }
    let backups = old.join(BACKUP);
    if backups.is_dir() {
        for entry in fs::read_dir(&backups)? {
            let entry = entry?;
            let target = new.join(BACKUP).join(entry.file_name());
            if !target.exists() {
                fs::create_dir_all(new.join(BACKUP))?;
                fs::rename(entry.path(), target)?;
            }
        }
    }
    Ok(kept)
}

/// A version as a folder name.
fn sanitize(version: &str) -> String {
    let name: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    // `.` and `..` would put the backup beside or above its folder (#157).
    if name.chars().all(|c| c == '.') {
        "previous".into()
    } else {
        name
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    fn touch(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// An upstream version of `..` can't steer the backup folder (#157).
    #[test]
    fn versions_name_one_backup_folder() {
        for version in ["", ".", "..", "..."] {
            assert_eq!(sanitize(version), "previous");
        }
        assert_eq!(sanitize("../../x"), ".._.._x");
        assert_eq!(sanitize("2509"), "2509");
    }

    #[test]
    fn a_lone_top_folder_is_lifted_out_but_an_app_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("a");
        touch(&dir.join("pcsx2/pcsx2/pcsx2-qt.exe"), "");
        flatten(&dir).unwrap();
        assert!(dir.join("pcsx2-qt.exe").is_file());
        let bundle = temp.path().join("b");
        touch(&bundle.join("PCSX2.app/Contents/MacOS/PCSX2"), "");
        flatten(&bundle).unwrap();
        assert!(bundle.join("PCSX2.app").is_dir());
    }

    #[test]
    fn a_program_that_isnt_an_archive_is_copied_in() {
        let temp = tempfile::tempdir().unwrap();
        let download = temp.path().join("gopher64-windows-x86_64.exe");
        fs::write(&download, b"MZ not an archive").unwrap();
        let into = temp.path().join("unpacked");
        unpack(&download, &into).unwrap();
        assert_eq!(
            files(&into).unwrap(),
            [PathBuf::from("gopher64-windows-x86_64.exe")]
        );
    }

    #[test]
    fn locate_prefers_named_appimages_bundles_then_executables() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        touch(&dir.join("docs/readme.txt"), "");
        touch(&dir.join("bin/pcsx2-qt"), "");
        let rules = DetectRules {
            executable: Some("^(?:pcsx2\\-qt|pcsx2)$".into()),
            appimage: Some("(?i)^PCSX2(?:[-_].*)?\\.AppImage$".into()),
            ..DetectRules::default()
        };
        assert_eq!(
            locate(dir, &rules, OperatingSystem::Linux),
            Some(EmulatorInstall::Folder {
                dir: dir.join("bin"),
                executable: "pcsx2-qt".into()
            })
        );
        // The only AppImage, though its name differs from the rule's.
        touch(&dir.join("melonDS-x86_64.AppImage"), "");
        assert_eq!(
            locate(dir, &rules, OperatingSystem::Linux),
            Some(EmulatorInstall::AppImage {
                file: dir.join("melonDS-x86_64.AppImage")
            })
        );
        // Windows matches executables without regard to case.
        touch(&dir.join("PCSX2-QT.EXE"), "");
        let windows = DetectRules {
            executable: Some("^pcsx2-qt\\.exe$".into()),
            ..DetectRules::default()
        };
        assert_eq!(
            locate(dir, &windows, OperatingSystem::Windows),
            Some(EmulatorInstall::Folder {
                dir: dir.to_path_buf(),
                executable: "PCSX2-QT.EXE".into()
            })
        );
    }

    #[test]
    fn updates_carry_the_users_files_and_back_up_clashes() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old"), temp.path().join("new"));
        touch(&old.join("retroarch.exe"), "1.21");
        touch(&old.join("assets/menu.png"), "1.21");
        record_shipped(&old).unwrap();
        touch(&old.join("retroarch.cfg"), "mine");
        touch(&old.join("cores/snes9x_libretro.dll"), "core");
        touch(&old.join("system/scph5501.bin"), "bios");
        touch(&old.join("assets/extra.png"), "mine");
        touch(&new.join("retroarch.exe"), "1.22");
        touch(&new.join("retroarch.cfg"), "default");
        record_shipped(&new).unwrap();

        assert_eq!(carry(&old, &new, "v1.21.0").unwrap(), 4);
        let read = |path: &str| fs::read_to_string(new.join(path)).unwrap();
        assert_eq!(read("retroarch.exe"), "1.22");
        assert_eq!(read("retroarch.cfg"), "default");
        assert_eq!(read(".caskade/update-backup/v1.21.0/retroarch.cfg"), "mine");
        assert_eq!(read("cores/snes9x_libretro.dll"), "core");
        assert_eq!(read("system/scph5501.bin"), "bios");
        assert_eq!(read("assets/extra.png"), "mine");
        assert!(!new.join("assets/menu.png").exists());
    }
}
