//! A ROM collection set up in one go (first-run setup): every subfolder
//! named after a platform (`snes`, `psx`, `Nintendo 64`, the ES-DE and
//! RetroArch names) gets a scanner, with an emulator that runs it. A folder
//! that is itself named after a platform is set up on its own.

use super::ScannerService;
use crate::{
    Result,
    emulation::{Catalog, EmulatorTarget, Platform, PlatformSettings, ProfileRef, RomScanner},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// What setting up a ROM collection did (`ScannerService::setup_folder`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RomFolderSetup {
    /// The scanners made, one per platform folder.
    pub added: Vec<RomScanner>,
    /// Platform folders a scanner already covers.
    pub existing: u32,
    /// Names of platforms with a folder but no emulator to run them.
    pub no_emulator: Vec<String>,
    /// Subfolders that name no platform.
    pub unknown: Vec<String>,
}

/// Folder names collections use that aren't a platform's own names.
const ALIASES: [(&str, &str); 12] = [
    ("famicom", "nintendo_nes"),
    ("sfc", "nintendo_snes"),
    ("superfamicom", "nintendo_snes"),
    ("ds", "nintendo_ds"),
    ("gamecube", "nintendo_gamecube"),
    ("ngc", "nintendo_gamecube"),
    ("genesis", "sega_genesis"),
    ("ps", "sony_playstation"),
    ("ps1", "sony_playstation"),
    ("playstation", "sony_playstation"),
    ("mame", "arcade"),
    ("fbneo", "arcade"),
];

impl ScannerService {
    /// Gives each platform folder in `folder` a scanner that imports with
    /// library updates. The profile is the platform's default, or else the
    /// first available one, which then becomes the default. Nothing is
    /// scanned yet.
    pub async fn setup_folder(&self, folder: &Path) -> Result<RomFolderSetup> {
        let catalog = self.emulation.catalog();
        let mut setup = RomFolderSetup::default();
        let own = folder
            .file_name()
            .and_then(|name| platform_named(&catalog, &name.to_string_lossy()));
        let found: Vec<(PathBuf, &Platform)> = match own {
            Some(platform) => vec![(folder.to_owned(), platform)],
            None => {
                let mut dirs: Vec<_> = std::fs::read_dir(folder)?
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| entry.path())
                    .collect();
                dirs.sort();
                let mut found = Vec::new();
                for dir in dirs {
                    let name = dir.file_name().unwrap_or_default().to_string_lossy();
                    if name.starts_with('.') {
                        continue;
                    }
                    match platform_named(&catalog, &name) {
                        Some(platform) => found.push((dir, platform)),
                        None => setup.unknown.push(name.into_owned()),
                    }
                }
                found
            }
        };
        let covered: Vec<PathBuf> = self
            .store
            .scanners()
            .await?
            .into_iter()
            .map(|scanner| scanner.folder)
            .collect();
        let views = self.emulation.platforms().await?;
        for (dir, platform) in found {
            if covered.contains(&dir) {
                setup.existing += 1;
                continue;
            }
            let Some(view) = views.iter().find(|view| view.platform.id == platform.id) else {
                continue;
            };
            let profile = match &view.settings.default {
                Some(default) => default.clone(),
                None => match view.compatible.iter().find(|profile| profile.available) {
                    Some(profile) => {
                        let chosen = ProfileRef {
                            emulator_id: profile.emulator_id.clone(),
                            profile_id: profile.profile_id.clone(),
                        };
                        self.emulation
                            .set_platform_settings(PlatformSettings {
                                default: Some(chosen.clone()),
                                ..view.settings.clone()
                            })
                            .await?;
                        chosen
                    }
                    None => {
                        setup.no_emulator.push(platform.name.clone());
                        continue;
                    }
                },
            };
            let scanner = self
                .save(RomScanner {
                    id: String::new(),
                    name: platform.name.clone(),
                    folder: dir,
                    platform: Some(platform.id.clone()),
                    profile,
                    launch_with: EmulatorTarget::PlatformDefault,
                    recursive: true,
                    scan_archives: false,
                    skip_cloud_placeholders: true,
                    exclusions: Vec::new(),
                    in_library_update: true,
                })
                .await?;
            setup.added.push(scanner);
        }
        Ok(setup)
    }
}

/// The platform a folder name stands for: its id, name, short name, its
/// names in ES-DE, RetroArch, Playnite and Hasheous, or a common alias.
/// Case, spaces and punctuation don't count.
fn platform_named<'a>(catalog: &'a Catalog, name: &str) -> Option<&'a Platform> {
    let wanted = key(name);
    if wanted.is_empty() {
        return None;
    }
    catalog
        .platforms()
        .find(|platform| {
            [
                Some(platform.id.as_str()),
                Some(platform.name.as_str()),
                Some(platform.short.as_str()),
                platform.ids.esde.as_deref(),
                platform.ids.libretro.as_deref(),
                platform.ids.playnite.as_deref(),
                platform.ids.hasheous.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|known| key(known) == wanted)
        })
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(alias, _)| *alias == wanted)
                .and_then(|(_, id)| catalog.platform(id))
        })
}

fn key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
#[path = "rom_folders_tests.rs"]
mod tests;
