//! An emulated release as its ROM files (#248, #279): what copies between
//! the user's devices and one-off shares send instead of the folder it's
//! in. A ROM scanner's folder holds every game of its platform, so a
//! release found by a scanner is never sent as its folder.
//!
//! The ROM files go together in a folder of their own under the sender's
//! cache (linked where the disk allows), described by a [`RomCopy`]: the
//! platform, the emulator definition and profile it plays with, and each
//! file's name and disc name. The receiving device places them as a game of
//! that platform in its chosen storage ([`place`]), pinned to its emulator
//! of the same definition and profile when it has one; otherwise emulator
//! setup installs that emulator.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::db::LibraryStore;
use crate::emulation::{EmulatorProfile, EmulatorSetup, EmulatorTarget, RomLocation};
use crate::install::Placed;
use crate::model::{GameEntry, LaunchSpec, StorageRoot};
use crate::{Error, Result};

/// The most ROM files one release brings (discs of one game).
pub(crate) const MAX_ROMS: usize = 64;

/// An emulated release, in a form another device reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RomCopy {
    pub platform: String,
    /// The emulator definition and its profile it plays with.
    pub definition: Option<String>,
    pub profile: Option<String>,
    pub roms: Vec<RomFile>,
}

/// One ROM file of a [`RomCopy`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RomFile {
    /// Its file name in the sent folder.
    pub file: String,
    /// Shown in the disc picker.
    pub name: Option<String>,
}

impl RomCopy {
    /// Whether what another device sent is within bounds: some files, not
    /// too many, each a bare file name.
    pub(crate) fn sound(&self) -> bool {
        !self.roms.is_empty()
            && self.roms.len() <= MAX_ROMS
            && self.roms.iter().all(|rom| plain(&rom.file))
    }
}

/// Installs an emulator by its definition (emulator setup).
#[async_trait]
pub trait EmulatorInstaller: Send + Sync {
    async fn install(&self, definition: &str) -> Result<()>;
}

#[async_trait]
impl EmulatorInstaller for EmulatorSetup {
    async fn install(&self, definition: &str) -> Result<()> {
        EmulatorSetup::install(self, definition).await.map(drop)
    }
}

/// A bare file name: nothing that leads out of its folder.
pub(crate) fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
}

/// Puts `from` (a file or folder) at `to`: renamed, or copied then removed
/// when they're on different disks. Blocking.
pub(crate) fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            move_path(&entry.path(), &to.join(entry.file_name()))?;
        }
        std::fs::remove_dir_all(from)
    } else {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    }
}

/// `name`, or `name (2)`, `name (3)`, …: the first that `root` hasn't got.
pub(crate) fn free_folder(root: &Path, name: &str) -> Result<PathBuf> {
    for n in 1..1000 {
        let candidate = if n == 1 {
            name.to_owned()
        } else {
            format!("{name} ({n})")
        };
        if !root.join(&candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Err(Error::Invalid(crate::tr!("no free folder name for it")))
}

/// Whether `entry` launches through an emulator.
pub(crate) fn emulated(entry: &GameEntry) -> bool {
    matches!(entry.launch, Some(LaunchSpec::Emulator { .. }))
}

/// An emulated release and its ROM files: each one's path here, the name
/// it's sent under and its disc name.
pub(crate) async fn rom_files(
    store: &LibraryStore,
    entry_id: &str,
) -> Result<(GameEntry, Vec<(PathBuf, String, Option<String>)>)> {
    let entry = store
        .get_entry(entry_id)
        .await?
        .ok_or_else(|| Error::Invalid(crate::tr!("that game isn't on this device")))?;
    let Some(LaunchSpec::Emulator { roms, .. }) = &entry.launch else {
        return Err(Error::Invalid(crate::tr!("that isn't an emulated game")));
    };
    if roms.is_empty() || roms.len() > MAX_ROMS {
        return Err(Error::Invalid(crate::tr!(
            "that game has no ROM files to share"
        )));
    }
    let scanners = store.scanners().await?;
    let mut names = HashSet::new();
    let mut files = Vec::new();
    for (index, rom) in roms.iter().enumerate() {
        let path = match &rom.location {
            RomLocation::Absolute { path } => Some(path.clone()),
            RomLocation::Scanner { scanner_id, path } => scanners
                .iter()
                .find(|scanner| scanner.id == *scanner_id)
                .and_then(|scanner| RomLocation::scanner_path(&scanner.folder, path)),
        }
        .filter(|path| path.is_file())
        .ok_or_else(|| Error::Invalid(crate::tr!("a ROM file of that game isn't there")))?;
        let base = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| plain(name))
            .unwrap_or_else(|| format!("rom{index}"));
        // Two discs with one name in different folders stay apart.
        let name = if names.insert(base.clone()) {
            base
        } else {
            let name = format!("{index} {base}");
            names.insert(name.clone());
            name
        };
        files.push((path, name, rom.name.clone()));
    }
    Ok((entry, files))
}

/// Release `entry_id` as a [`RomCopy`], with its files' paths here.
pub(crate) async fn describe(
    store: &LibraryStore,
    entry_id: &str,
) -> Result<(GameEntry, RomCopy, Vec<(PathBuf, String)>)> {
    let (entry, files) = rom_files(store, entry_id).await?;
    let Some(LaunchSpec::Emulator {
        platform_id,
        target,
        ..
    }) = &entry.launch
    else {
        return Err(Error::Invalid(crate::tr!("that isn't an emulated game")));
    };
    let (definition, profile) = portable_profile(store, platform_id, target).await?;
    let copy = RomCopy {
        platform: platform_id.clone(),
        definition,
        profile,
        roms: files
            .iter()
            .map(|(_, file, name)| RomFile {
                file: file.clone(),
                name: name.clone(),
            })
            .collect(),
    };
    let sources = files
        .into_iter()
        .map(|(path, file, _)| (path, file))
        .collect();
    Ok((entry, copy, sources))
}

/// The emulator definition and profile `target` plays `platform` with
/// here, as another device can find them.
async fn portable_profile(
    store: &LibraryStore,
    platform: &str,
    target: &EmulatorTarget,
) -> Result<(Option<String>, Option<String>)> {
    let (emulator, profile) = match target {
        EmulatorTarget::Profile {
            emulator_id,
            profile_id,
        } => (Some(emulator_id.clone()), Some(profile_id.clone())),
        EmulatorTarget::AskProfile { emulator_id } => (Some(emulator_id.clone()), None),
        EmulatorTarget::PlatformDefault => match store
            .get_platform_settings(platform)
            .await?
            .and_then(|settings| settings.default)
        {
            Some(default) => (Some(default.emulator_id), Some(default.profile_id)),
            None => (None, None),
        },
        EmulatorTarget::AskEmulator => (None, None),
    };
    let Some(emulator) = emulator else {
        return Ok((None, None));
    };
    let Some(emulator) = store.get_emulator(&emulator).await? else {
        return Ok((None, None));
    };
    let profile = profile
        .and_then(|id| emulator.profile(&id).cloned())
        .and_then(|profile| match profile {
            EmulatorProfile::BuiltIn {
                definition_profile, ..
            } => Some(definition_profile),
            EmulatorProfile::Custom { .. } => None,
        });
    Ok((emulator.definition, profile))
}

/// A folder `dir` of `sources` under their names, made afresh.
pub(crate) async fn put_together(dir: &Path, sources: Vec<(PathBuf, String)>) -> Result<PathBuf> {
    let folder = dir.to_owned();
    tokio::task::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder)?;
        for (source, name) in sources {
            let target = folder.join(name);
            if target.exists() {
                continue;
            }
            if std::fs::hard_link(&source, &target).is_err() {
                std::fs::copy(&source, &target)?;
            }
        }
        Ok::<_, Error>(())
    })
    .await
    .map_err(|error| Error::Io(std::io::Error::other(error)))??;
    Ok(dir.to_owned())
}

/// Whether this device has an emulator of `definition`, or else one set
/// for `platform`.
pub(crate) async fn emulator_here(
    store: &LibraryStore,
    platform: &str,
    definition: Option<&str>,
) -> Result<bool> {
    if let Some(definition) = definition {
        return Ok(store
            .emulators()
            .await?
            .iter()
            .any(|emulator| emulator.definition.as_deref() == Some(definition)));
    }
    Ok(store
        .get_platform_settings(platform)
        .await?
        .is_some_and(|settings| settings.default.is_some()))
}

/// This device's emulator and profile for a definition's profile, when it
/// has them.
async fn profile_here(
    store: &LibraryStore,
    definition: Option<&str>,
    profile: Option<&str>,
) -> Result<Option<EmulatorTarget>> {
    let (Some(definition), Some(profile)) = (definition, profile) else {
        return Ok(None);
    };
    for emulator in store.emulators().await? {
        if emulator.definition.as_deref() != Some(definition) {
            continue;
        }
        let found = emulator.profiles.iter().find(|candidate| {
            matches!(candidate, EmulatorProfile::BuiltIn { definition_profile, .. } if definition_profile == profile)
        });
        if let Some(found) = found {
            return Ok(Some(EmulatorTarget::Profile {
                emulator_id: emulator.id.clone(),
                profile_id: found.id().to_owned(),
            }));
        }
    }
    Ok(None)
}

/// The storage root at `path`.
pub(crate) async fn root_at(store: &LibraryStore, path: &Path) -> Result<StorageRoot> {
    store
        .roots()
        .await?
        .into_iter()
        .find(|root| root.path == path)
        .ok_or_else(|| Error::Invalid(crate::tr!("that storage location is gone")))
}

/// Places the ROM files that arrived in `folder` as a release of `into`
/// (a copy of the user's own) or else a game of its own called `title`, of
/// `rom`'s platform, in a folder of its own in the storage root at `root`,
/// and returns the game. Pinned to this device's emulator of the same
/// definition and profile when it has one; otherwise, when this device has
/// no emulator for it, `installer` installs that definition in the
/// background.
pub(crate) async fn place(
    store: &LibraryStore,
    installer: Option<Arc<dyn EmulatorInstaller>>,
    rom: &RomCopy,
    folder: &Path,
    root: &Path,
    title: &str,
    into: Option<&str>,
) -> Result<Placed> {
    if !rom.sound() {
        return Err(Error::Invalid(crate::tr!(
            "this share's description is damaged"
        )));
    }
    let root = root_at(store, root).await?;
    let (dir, files) = {
        let (folder, base) = (folder.to_owned(), root.path.clone());
        let name = super::folder_name(title);
        let names: Vec<String> = rom.roms.iter().map(|rom| rom.file.clone()).collect();
        tokio::task::spawn_blocking(move || {
            let dir = base.join(free_folder(&base, &name)?);
            std::fs::create_dir_all(&dir)?;
            for name in &names {
                move_path(&folder.join(name), &dir.join(name))?;
            }
            Ok::<_, Error>((
                dir,
                names.into_iter().map(PathBuf::from).collect::<Vec<_>>(),
            ))
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))??
    };
    let (mut game, mut entry) = crate::emulation::rom_entry(&rom.platform, &dir, &files, None);
    if !title.trim().is_empty() {
        title.trim().clone_into(&mut game.title);
    }
    game.platforms = vec![rom.platform.clone()];
    let target = profile_here(store, rom.definition.as_deref(), rom.profile.as_deref()).await?;
    if let Some(LaunchSpec::Emulator {
        target: pinned,
        roms: placed,
        ..
    }) = &mut entry.launch
    {
        if let Some(target) = target.clone() {
            *pinned = target;
        }
        // Disc names as the sender had them, in the sent order.
        for (placed, sent) in placed.iter_mut().zip(&rom.roms) {
            if sent.name.is_some() {
                placed.name.clone_from(&sent.name);
            }
        }
    }
    let existing = match into {
        Some(id) => store.game(id).await?,
        None => None,
    };
    let added = existing.is_none();
    let game = match existing {
        Some(game) => game,
        None => store.upsert_game(&game).await?,
    };
    entry.game_id = game.id.clone();
    store.upsert_entry(&entry).await?;
    if target.is_none()
        && !emulator_here(store, &rom.platform, rom.definition.as_deref()).await?
        && let (Some(definition), Some(installer)) = (rom.definition.clone(), installer)
    {
        // Emulator setup follows in Activity; the game is here whatever it
        // does.
        tokio::spawn(async move {
            if let Err(error) = installer.install(&definition).await {
                tracing::warn!(%definition, %error, "a received ROM's emulator wasn't installed");
            }
        });
    }
    Ok(Placed {
        game_id: game.id,
        added,
    })
}
