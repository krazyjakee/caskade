//! What an update keeps of the version it replaces (the user's files).
//!
//! The old install's manifest says what it shipped, so everything else in
//! its folder (saves, configs, mods) is the user's. Each file is decided
//! three ways, against what the old version shipped and the new one ships:
//!
//! - Upstream didn't change it (or neither version ships it): the old
//!   folder's copy is kept, edits and all.
//! - Upstream changed or removed it, and the user didn't touch it: the new
//!   version's stands.
//! - Both changed it: the new version's stands, and the user's copy goes
//!   to `.caskade/update-backup/<old version>/` in the new folder.
//!
//! Inside `.caskade/`, a Windows game's prefix comes across whole when the
//! new install has none yet (it is made at first launch); when the new
//! installer made one, the old prefix's `drive_c/users` (AppData, Documents,
//! Saved Games) is copied into it. Earlier update backups come across too.
//!
//! Files are moved, not copied: the old folder is deleted afterwards.

use caskade_installer::manifest::InstallManifest;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Where conflicting user files go, inside the game folder.
pub(crate) const BACKUP_DIR: &str = ".caskade/update-backup";

/// What the carry-over did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Carried {
    /// User files kept.
    pub kept: usize,
    /// User files the new version replaced, now under [`BACKUP_DIR`].
    pub backed_up: Vec<PathBuf>,
}

/// A shipped file: its size and digest.
type Shipped = HashMap<PathBuf, (u64, Option<String>)>;

fn shipped(manifest: &InstallManifest) -> Shipped {
    manifest
        .record()
        .files
        .iter()
        .map(|file| {
            (
                file.path.as_path().to_path_buf(),
                (
                    file.size,
                    file.sha256.as_ref().map(|d| d.as_str().to_owned()),
                ),
            )
        })
        .collect()
}

/// Moves the user's files from `old` (the replaced version, installed per
/// `old_manifest`) into `new` (installed per `new_manifest`). `label`
/// names the backup folder of files both sides changed.
pub(crate) fn carry_over(
    old: &Path,
    old_manifest: &InstallManifest,
    new: &Path,
    new_manifest: &InstallManifest,
    label: &str,
) -> io::Result<Carried> {
    let mut carry = Carry {
        base: shipped(old_manifest),
        theirs: shipped(new_manifest),
        backup: Path::new(BACKUP_DIR).join(safe_label(label)),
        carried: Carried::default(),
    };
    carry.dir(old, new, Path::new(""))?;
    prefix(old, new)?;
    merge(&old.join(BACKUP_DIR), &new.join(BACKUP_DIR))?;
    Ok(carry.carried)
}

/// A release name as one folder name.
fn safe_label(label: &str) -> String {
    let label: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    match label.trim_matches('.') {
        "" => "previous".into(),
        label => label.into(),
    }
}

struct Carry {
    base: Shipped,
    theirs: Shipped,
    backup: PathBuf,
    carried: Carried,
}

impl Carry {
    fn dir(&mut self, old: &Path, new: &Path, relative: &Path) -> io::Result<()> {
        for entry in fs::read_dir(old.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            if relative.as_os_str().is_empty() && entry.file_name() == ".caskade" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                let target = new.join(&path);
                match fs::symlink_metadata(&target) {
                    Ok(meta) if meta.is_dir() => {}
                    // The new version has a file there: the folder is the
                    // user's, and it can't stay.
                    Ok(_) => {
                        self.back_up(old, new, &path)?;
                        continue;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        fs::create_dir_all(&target)?;
                    }
                    Err(error) => return Err(error),
                }
                self.dir(old, new, &path)?;
            } else {
                self.file(old, new, &path)?;
            }
        }
        Ok(())
    }

    fn file(&mut self, old: &Path, new: &Path, path: &Path) -> io::Result<()> {
        let base = self.base.get(path);
        let theirs = self.theirs.get(path);
        let upstream_changed = match (base, theirs) {
            (None, None) => false,
            (Some(base), Some(theirs)) => !same(base, theirs),
            _ => true,
        };
        if !upstream_changed {
            return keep(&old.join(path), &new.join(path)).map(|kept| {
                // An untouched shipped file isn't the user's.
                if kept && base.is_none() {
                    self.carried.kept += 1;
                }
            });
        }
        if let Some(base) = base
            && unchanged(&old.join(path), base)?
        {
            return Ok(());
        }
        self.back_up(old, new, path)
    }

    fn back_up(&mut self, old: &Path, new: &Path, path: &Path) -> io::Result<()> {
        let target = new.join(&self.backup).join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(old.join(path), target)?;
        self.carried.backed_up.push(path.to_path_buf());
        Ok(())
    }
}

/// Whether two shipped files are the same: by digest, or by size where a
/// manifest has none.
fn same(a: &(u64, Option<String>), b: &(u64, Option<String>)) -> bool {
    match (&a.1, &b.1) {
        (Some(x), Some(y)) => x == y,
        _ => a.0 == b.0,
    }
}

/// Whether the file at `path` is still what was shipped.
fn unchanged(path: &Path, shipped: &(u64, Option<String>)) -> io::Result<bool> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() != shipped.0 {
        return Ok(false);
    }
    let Some(digest) = &shipped.1 else {
        return Ok(true);
    };
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()) == *digest)
}

/// Moves `from` over `to`, unless `to` is a folder. Whether it moved.
fn keep(from: &Path, to: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(to) {
        Ok(meta) if meta.is_dir() => return Ok(false),
        Ok(_) => fs::remove_file(to)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
        }
        Err(error) => return Err(error),
    }
    fs::rename(from, to)?;
    Ok(true)
}

/// A Windows game's prefix, made by Wine or Proton in the game folder.
fn prefix(old: &Path, new: &Path) -> io::Result<()> {
    let from = old.join(super::MANAGED_PREFIX);
    let to = new.join(super::MANAGED_PREFIX);
    if !from.is_dir() {
        return Ok(());
    }
    if fs::symlink_metadata(&to).is_err() {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        return fs::rename(from, to);
    }
    merge(&from.join("drive_c/users"), &to.join("drive_c/users"))
}

/// Moves everything under `from` into `to`, replacing files, keeping
/// folders. Links move as links.
fn merge(from: &Path, to: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(from) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !meta.is_dir() {
        keep(from, to)?;
        return Ok(());
    }
    match fs::symlink_metadata(to) {
        Ok(target) if target.is_dir() => {
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                merge(&entry.path(), &to.join(entry.file_name()))?;
            }
            Ok(())
        }
        // A link or file where the old side has a folder: the new side's
        // stays.
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(from, to)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caskade_installer::manifest::{InstalledFile, ManifestPath, Sha256Digest};

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    /// Writes `files` into `dir` and returns a manifest that shipped them.
    fn install(dir: &Path, files: &[(&str, &[u8])]) -> InstallManifest {
        let mut record = InstallManifest::from_json(include_bytes!(
            "../../../caskade-installer/tests/manifests/install.v1.posix.json"
        ))
        .unwrap()
        .into_record();
        record.files.clear();
        for (path, bytes) in files {
            let file = dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, bytes).unwrap();
            record.files.push(InstalledFile {
                path: ManifestPath::new(path).unwrap(),
                size: bytes.len() as u64,
                sha256: Some(Sha256Digest::new(digest(bytes)).unwrap()),
            });
        }
        InstallManifest::new(record).unwrap()
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn user_files_stay_and_conflicts_are_backed_up() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old"), temp.path().join("new"));
        let old_manifest = install(
            &old,
            &[
                ("game.exe", b"v1"),
                ("data/levels.pak", b"levels"),
                ("config.ini", b"default"),
                ("readme.txt", b"v1 notes"),
                ("removed.dll", b"old"),
            ],
        );
        let new_manifest = install(
            &new,
            &[
                ("game.exe", b"v2"),
                ("data/levels.pak", b"levels"),
                ("config.ini", b"new default"),
                ("readme.txt", b"v2 notes"),
                ("saves/slot1.sav", b"shipped"),
            ],
        );
        // The user played v1: a save, an edited config, a mod over a pak
        // upstream left alone, and a settings file in a new folder.
        fs::write(old.join("config.ini"), b"fullscreen=1").unwrap();
        fs::write(old.join("data/levels.pak"), b"modded").unwrap();
        fs::create_dir_all(old.join("saves")).unwrap();
        fs::write(old.join("saves/slot1.sav"), b"progress").unwrap();
        fs::create_dir_all(old.join("user/settings")).unwrap();
        fs::write(old.join("user/settings/keys.cfg"), b"wasd").unwrap();
        fs::create_dir_all(old.join(".caskade")).unwrap();
        fs::write(old.join(".caskade/install.json"), b"{}").unwrap();

        let carried = carry_over(&old, &old_manifest, &new, &new_manifest, "v1.0").unwrap();

        // New binaries and notes; the untouched file upstream removed is gone.
        assert_eq!(read(&new.join("game.exe")), "v2");
        assert_eq!(read(&new.join("readme.txt")), "v2 notes");
        assert!(!new.join("removed.dll").exists());
        // The mod over an unchanged file, and the user's own files, stay.
        assert_eq!(read(&new.join("data/levels.pak")), "modded");
        assert_eq!(read(&new.join("user/settings/keys.cfg")), "wasd");
        // Both sides changed these: upstream wins, the user's are kept aside.
        assert_eq!(read(&new.join("config.ini")), "new default");
        assert_eq!(read(&new.join("saves/slot1.sav")), "shipped");
        let backup = new.join(".caskade/update-backup/v1.0");
        assert_eq!(read(&backup.join("config.ini")), "fullscreen=1");
        assert_eq!(read(&backup.join("saves/slot1.sav")), "progress");
        assert!(!new.join(".caskade/install.json").exists());
        assert_eq!(carried.kept, 1);
        let mut backed_up = carried.backed_up;
        backed_up.sort();
        assert_eq!(
            backed_up,
            [
                PathBuf::from("config.ini"),
                PathBuf::from("saves/slot1.sav")
            ]
        );
    }

    #[test]
    fn prefixes_and_earlier_backups_come_across() {
        let temp = tempfile::tempdir().unwrap();
        let (old, new) = (temp.path().join("old"), temp.path().join("new"));
        let old_manifest = install(&old, &[("game.exe", b"v1")]);
        let new_manifest = install(&new, &[("game.exe", b"v2")]);
        let users = "drive_c/users/me/AppData/Roaming/Game";
        fs::create_dir_all(old.join(MANAGED).join(users)).unwrap();
        fs::write(old.join(MANAGED).join(users).join("save.dat"), b"save").unwrap();
        fs::write(old.join(MANAGED).join("system.reg"), b"old registry").unwrap();
        fs::create_dir_all(old.join(BACKUP_DIR).join("v0.9")).unwrap();
        fs::write(old.join(BACKUP_DIR).join("v0.9/config.ini"), b"older").unwrap();

        // The new version has no prefix yet: the old one comes across whole.
        carry_over(&old, &old_manifest, &new, &new_manifest, "v1").unwrap();
        assert_eq!(read(&new.join(MANAGED).join("system.reg")), "old registry");
        assert_eq!(
            read(&new.join(MANAGED).join(users).join("save.dat")),
            "save"
        );
        assert_eq!(read(&new.join(BACKUP_DIR).join("v0.9/config.ini")), "older");

        // An installer made a new prefix: only the users' folders come.
        let newer = temp.path().join("newer");
        let newer_manifest = install(&newer, &[("game.exe", b"v3")]);
        fs::create_dir_all(newer.join(MANAGED).join("drive_c/users/me")).unwrap();
        fs::write(newer.join(MANAGED).join("system.reg"), b"new registry").unwrap();
        carry_over(&new, &new_manifest, &newer, &newer_manifest, "v2").unwrap();
        assert_eq!(
            read(&newer.join(MANAGED).join("system.reg")),
            "new registry"
        );
        assert_eq!(
            read(&newer.join(MANAGED).join(users).join("save.dat")),
            "save"
        );
    }

    const MANAGED: &str = super::super::MANAGED_PREFIX;

    #[test]
    fn labels_are_one_safe_folder_name() {
        assert_eq!(safe_label("v1.2.0"), "v1.2.0");
        assert_eq!(safe_label("release/1.2"), "release_1.2");
        assert_eq!(safe_label(".."), "previous");
    }
}
