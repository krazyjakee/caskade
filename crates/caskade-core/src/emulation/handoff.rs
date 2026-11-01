//! ROMs handed off by the installer (#120, EMULATION-PLAN §9). The
//! installer names a platform with its own ids; the files become one game
//! whose ROMs keep their absolute paths and launch with the platform's
//! default emulator.

use super::{ArgsOverride, Catalog, EmulatorTarget, GameRom, scan::parse_name};
use crate::{
    install::HANDOFF_SOURCE,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec},
};
use std::path::{Path, PathBuf};

/// The installer's platform ids (`data/rom-platforms.toml`) that Caskade's
/// definitions name differently.
const INSTALLER_PLATFORMS: [(&str, &str); 5] = [
    ("nintendo_game_boy", "nintendo_gb"),
    ("nintendo_game_boy_color", "nintendo_gbc"),
    ("nintendo_game_boy_advance", "nintendo_gba"),
    ("nintendo_64", "nintendo_n64"),
    ("sega_megadrive", "sega_genesis"),
];

/// The catalog platform of an installer hand-off: the same id, a renamed
/// one, or the only platform that claims the files' extension.
pub fn platform_for(catalog: &Catalog, installer: &str, files: &[PathBuf]) -> Option<String> {
    if catalog.platform(installer).is_some() {
        return Some(installer.to_owned());
    }
    if let Some((_, id)) = INSTALLER_PLATFORMS
        .iter()
        .find(|(from, to)| *from == installer && catalog.platform(to).is_some())
    {
        return Some((*id).to_owned());
    }
    let extension = files.first()?.extension()?.to_str()?.to_ascii_lowercase();
    let mut claims = catalog
        .platforms()
        .filter(|platform| platform.extensions.contains(&extension));
    let only = claims.next()?;
    claims.next().is_none().then(|| only.id.clone())
}

/// The game and entry for ROMs imported where they are (§9 step 2). `files`
/// are relative to `base`; several files are the discs of one game. The
/// entry is keyed on the first ROM's path, so installing it again updates it.
pub fn rom_entry(
    platform: &str,
    base: &Path,
    files: &[PathBuf],
    catalog: Option<&Catalog>,
) -> (Game, GameEntry) {
    let regions: Vec<_> = catalog
        .map(|catalog| catalog.regions().cloned().collect())
        .unwrap_or_default();
    let paths: Vec<PathBuf> = files.iter().map(|file| base.join(file)).collect();
    let first = paths.first().cloned().unwrap_or_else(|| base.to_path_buf());
    let parsed = parse_name(&first, &regions);
    let title = if parsed.title.trim().is_empty() {
        first
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ROM".into())
    } else {
        parsed.title
    };
    let mut roms: Vec<(u32, GameRom)> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let part = parse_name(path, &regions).part;
            let order = part.as_ref().map_or(index as u32, |part| part.number);
            let rom = GameRom {
                location: super::RomLocation::Absolute { path: path.clone() },
                name: part.map(|part| part.label),
                size: std::fs::metadata(path).ok().map(|metadata| metadata.len()),
            };
            (order, rom)
        })
        .collect();
    roms.sort_by_key(|(order, _)| *order);
    let game = Game::new(title);
    let mut entry = GameEntry::new(
        &game.id,
        HANDOFF_SOURCE,
        first.to_string_lossy().into_owned(),
    );
    entry.location = Some(InstallLocation::External {
        path: base.to_path_buf(),
    });
    entry.size = roms
        .iter()
        .map(|(_, rom)| rom.size)
        .try_fold(0u64, |total, size| total.checked_add(size?));
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: platform.to_owned(),
        target: EmulatorTarget::PlatformDefault,
        roms: roms.into_iter().map(|(_, rom)| rom).collect(),
        args: ArgsOverride::Default,
    });
    entry.state = InstallState::Installed;
    (game, entry)
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
