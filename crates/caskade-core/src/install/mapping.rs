//! Install outcomes and manifests as library records (#51, #54).

use crate::{
    Error, Result,
    model::{Game, GameEntry, InstallLocation, LaunchSpec},
};
use caskade_discovery::{ExternalId, GameRuntime, InstallState, Prefix, SourceId};
use caskade_installer::{
    HandOff, LaunchTarget, RuntimeSpec, manifest::InstallManifest, pipeline::INSTALL_ID,
};
use std::path::{Path, PathBuf};

/// The library source of games Caskade installed into a storage root.
pub const STORAGE_SOURCE: SourceId = SourceId::new("caskade-storage");
/// The library source of installs handed to another launcher or emulator.
pub const HANDOFF_SOURCE: SourceId = SourceId::new("caskade-handoff");
/// The prefix of a managed Windows game, relative to its install folder.
pub use caskade_installer::runtime::proton::PREFIX as MANAGED_PREFIX;

/// The library game and entry for a managed install. The entry is keyed on
/// the install's stable `caskade-install` id, so moves and renames keep it.
pub fn from_manifest(
    root_id: &str,
    rel_dir: &Path,
    manifest: &InstallManifest,
) -> Result<(Game, GameEntry)> {
    let record = manifest.record();
    let install_id = record
        .ids
        .get(INSTALL_ID)
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "install manifest has no valid {key} id",
                key = INSTALL_ID
            ))
        })?;
    let game = Game::new(record.title.clone());
    let mut entry = GameEntry::new(&game.id, STORAGE_SOURCE, install_id.clone());
    entry.ids = external_ids(record.ids.iter());
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: root_id.to_owned(),
        rel_dir: rel_dir.to_path_buf(),
    });
    entry.launch = record.launch_targets.first().map(launch_spec);
    entry.runtime = runtime(&record.runtime);
    entry.alias_id = record.alias.clone();
    entry.size = record
        .files
        .iter()
        .try_fold(0u64, |total, file| total.checked_add(file.size));
    Ok((game, entry))
}

/// Store ids the installer found (`gog`, `steam`, …) that discovery knows.
fn external_ids<'a>(ids: impl Iterator<Item = (&'a String, &'a String)>) -> Vec<ExternalId> {
    ids.filter(|(key, _)| key.as_str() != INSTALL_ID)
        .filter_map(|(key, value)| format!("{key}:{value}").parse().ok())
        .collect()
}

/// A launch target relative to the install folder.
fn launch_spec(target: &LaunchTarget) -> LaunchSpec {
    LaunchSpec::Executable {
        path: target.path.clone(),
        args: target.args.clone(),
        cwd: Some(target.working_dir.clone()).filter(|cwd| !cwd.as_os_str().is_empty()),
        env: target.env.clone(),
    }
}

/// Wine and Proton games keep their prefix in the install folder; the
/// runner is chosen at launch from the app's runner settings.
fn runtime(runtime: &RuntimeSpec) -> GameRuntime {
    match runtime {
        RuntimeSpec::Native => GameRuntime::Native,
        RuntimeSpec::ProtonPrefix { .. } | RuntimeSpec::WinePrefix { .. } => GameRuntime::Windows {
            prefix: Some(Prefix {
                path: PathBuf::from(MANAGED_PREFIX),
                arch: None,
                owner: None,
            }),
            runner: Some(caskade_discovery::RunnerHint {
                kind: if matches!(runtime, RuntimeSpec::WinePrefix { .. }) {
                    caskade_discovery::RunnerKind::Wine
                } else {
                    caskade_discovery::RunnerKind::Umu
                },
                name: None,
                path: None,
            }),
        },
    }
}

/// The library records of a hand-off: ROMs become an emulated game where
/// they are (`emulation::rom_entry`), a delegated install launches through
/// its owner's URI, and a system package through the command it installed.
pub fn from_handoff(handoff: &HandOff, title: &str, input: &Path) -> (Game, GameEntry) {
    let game = Game::new(title);
    let mut entry = GameEntry::new(&game.id, HANDOFF_SOURCE, crate::model::new_id());
    match handoff {
        // Without the emulation catalog: the installer's platform id as is.
        HandOff::Rom { platform, files } => {
            let base = if input.is_dir() {
                input
            } else {
                input.parent().unwrap_or(input)
            };
            return crate::emulation::rom_entry(platform, base, files, None);
        }
        HandOff::Delegated { launch_uri, .. } => {
            entry.launch = Some(LaunchSpec::Uri {
                uri: launch_uri.clone(),
            });
            entry.state = InstallState::Installed;
        }
        // The package manager owns the files; the game runs the command the
        // package installed, by its absolute path.
        HandOff::Package {
            title,
            program,
            args,
            ..
        } => {
            let game = Game::new(title);
            let mut entry = GameEntry::new(&game.id, HANDOFF_SOURCE, crate::model::new_id());
            entry.launch = program.as_ref().map(|program| LaunchSpec::Executable {
                path: program.clone(),
                args: args.clone(),
                cwd: None,
                env: Default::default(),
            });
            entry.state = InstallState::Installed;
            return (game, entry);
        }
    }
    (game, entry)
}

/// The state reason ROM hand-offs had before emulation existed (#51);
/// migration 0003 turns those entries into emulated games.
pub const NEEDS_EMULATOR: &str = "needs emulator setup";

#[cfg(test)]
mod runtime_tests {
    use super::*;
    #[test]
    fn managed_prefix_mapping_retains_family_without_machine_paths() {
        for (spec, kind) in [
            (
                RuntimeSpec::WinePrefix { id: "game".into() },
                caskade_discovery::RunnerKind::Wine,
            ),
            (
                RuntimeSpec::ProtonPrefix { id: "game".into() },
                caskade_discovery::RunnerKind::Umu,
            ),
        ] {
            let GameRuntime::Windows {
                prefix: Some(prefix),
                runner: Some(runner),
            } = runtime(&spec)
            else {
                panic!("missing prefix runner");
            };
            assert_eq!(prefix.path, Path::new(MANAGED_PREFIX));
            assert_eq!(runner.kind, kind);
            assert!(runner.path.is_none());
        }
    }
}
