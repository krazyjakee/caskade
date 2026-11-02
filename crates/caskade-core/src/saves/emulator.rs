use super::{SaveLocation, Saves};
use crate::{
    Result,
    emulation::{
        EmulatorDefinition, EmulatorTarget, InstallKind, OperatingSystem, OsFolders, VarContext,
        emulator_dir,
    },
    model::{Game, GameEntry, LaunchSpec},
};
use caskade_discovery::env::KnownFolder;
use std::path::PathBuf;

pub(super) struct Plan {
    pub definition: EmulatorDefinition,
    os: OperatingSystem,
    kind: InstallKind,
    contexts: Vec<VarContext>,
}

pub(super) async fn plan(saves: &Saves, entry: &GameEntry, game: &Game) -> Result<Option<Plan>> {
    let Some(LaunchSpec::Emulator {
        platform_id,
        target,
        roms,
        ..
    }) = &entry.launch
    else {
        return Ok(None);
    };
    let chosen = match target {
        EmulatorTarget::Profile {
            emulator_id,
            profile_id,
        } => Some((emulator_id.clone(), profile_id.clone())),
        EmulatorTarget::PlatformDefault => saves
            .store
            .get_platform_settings(platform_id)
            .await?
            .and_then(|settings| settings.default)
            .map(|profile| (profile.emulator_id, profile.profile_id)),
        // An emulator chosen at launch is not a known save destination yet.
        EmulatorTarget::AskEmulator | EmulatorTarget::AskProfile { .. } => None,
    };
    let Some((emulator_id, profile_id)) = chosen else {
        return Ok(None);
    };
    let Some(emulator) = saves.store.get_emulator(&emulator_id).await? else {
        return Ok(None);
    };
    if emulator.profile(&profile_id).is_none() {
        return Ok(None);
    }
    let Some(definition) = emulator
        .definition
        .as_deref()
        .and_then(|id| saves.catalog.emulator(id))
    else {
        return Ok(None);
    };
    if definition.saves.files.is_empty() && definition.saves.states.is_empty() {
        return Ok(None);
    }
    let host = &saves.environment;
    let os = match host.os() {
        caskade_discovery::OperatingSystem::Windows => OperatingSystem::Windows,
        caskade_discovery::OperatingSystem::Linux => OperatingSystem::Linux,
        caskade_discovery::OperatingSystem::Macos => OperatingSystem::Macos,
    };
    let folder = |known| host.known_folder(known).map(PathBuf::from);
    let folders = OsFolders {
        user_home: Some(host.home().into()),
        xdg_config: folder(KnownFolder::ConfigHome),
        app_data: folder(KnownFolder::AppData),
        app_support: folder(KnownFolder::DataHome),
        ..Default::default()
    };
    let mut contexts = Vec::new();
    for rom in roms {
        contexts.push(VarContext {
            rom: Some(rom.path(&saves.store).await?),
            emulator_dir: emulator_dir(&emulator.install, &folders, None),
            name: Some(game.title.clone()),
            platform: Some(platform_id.clone()),
            game_id: Some(game.id.clone()),
            folders: folders.clone(),
            ..Default::default()
        });
    }
    Ok((!contexts.is_empty()).then(|| Plan {
        definition: definition.clone(),
        os,
        kind: emulator.install.kind(),
        contexts,
    }))
}

pub(super) fn resolve(plan: Plan, warnings: &mut Vec<String>) -> Vec<SaveLocation> {
    let mut found = Vec::new();
    for context in plan.contexts {
        let patterns = match plan.definition.resolve_saves(plan.os, plan.kind, &context) {
            Ok(patterns) => patterns,
            Err(error) => {
                warnings.push(error);
                continue;
            }
        };
        for (patterns, tag) in [(patterns.files, "save"), (patterns.states, "state")] {
            for pattern in patterns {
                let result = super::expand_glob(&pattern, plan.os == OperatingSystem::Linux)
                    .and_then(|paths| {
                        paths
                            .into_iter()
                            .filter_map(|path| {
                                super::scan_path(&path, vec![tag.into()]).transpose()
                            })
                            .collect::<Result<Vec<_>>>()
                    });
                match result {
                    Ok(locations) => found.extend(locations),
                    Err(error) => warnings.push(error.to_string()),
                }
            }
        }
    }
    super::service::deduplicate(found)
}
