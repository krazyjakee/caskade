//! Launch actions (#177): which of a release's actions a launch starts, and
//! what it changes about the release's own launch. An action runs through
//! the same pipeline as the release: its target stands in for the entry's
//! `launch`, so managed paths, runners, emulator profiles, hooks and session
//! tracking stay one path.
use super::{LaunchChoice, LaunchError, LaunchPrompt, PreparedLaunch, Result};
use crate::{
    db::LibraryStore,
    model::{
        ActionKind, ActionTracking, EntryActionSettings, GameAction, GameEntry, InstallLocation,
        LaunchSpec, MANUAL_SOURCE, built_in_action_id,
    },
    tracking::TrackingMode,
};
use caskade_discovery::GameRuntime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

/// A play action Play can ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionOption {
    pub id: String,
    pub name: String,
    /// The release's default.
    pub default: bool,
}

/// The release's built-in action: its own launch, named and tracked as the
/// user set. `None` when the release has no launch of its own.
pub fn built_in_action(entry: &GameEntry, settings: &EntryActionSettings) -> Option<GameAction> {
    let target = entry.launch.clone()?;
    Some(GameAction {
        id: built_in_action_id(&entry.id),
        entry_id: entry.id.clone(),
        name: settings
            .built_in
            .name
            .clone()
            .unwrap_or_else(|| crate::tr!("Play")),
        kind: ActionKind::Play,
        target,
        tracking: settings.built_in.tracking.clone().unwrap_or_default(),
    })
}

/// Every action of a release in order: its built-in action, then the ones
/// the user added.
pub fn entry_actions(
    entry: &GameEntry,
    added: Vec<GameAction>,
    settings: &EntryActionSettings,
) -> Vec<GameAction> {
    built_in_action(entry, settings)
        .into_iter()
        .chain(added)
        .collect()
}

/// The play action a launch starts: the one `choice` names, else the
/// release's default (its built-in action when the default is gone). Asks
/// when the release says so and has more than one.
pub(crate) fn select(
    entry: &GameEntry,
    actions: Vec<GameAction>,
    settings: &EntryActionSettings,
    choice: &LaunchChoice,
) -> Result<GameAction> {
    if let Some(id) = &choice.action {
        let action = actions
            .into_iter()
            .find(|action| action.id == *id)
            .ok_or_else(|| {
                LaunchError::Invalid(crate::tr!("that action no longer exists; choose another"))
            })?;
        if action.kind == ActionKind::Utility {
            return Err(LaunchError::Invalid(crate::tr!(
                "“{name}” is a tool, not a way to play: run it from the game's menu",
                name = action.name
            )));
        }
        return Ok(action);
    }
    let play: Vec<GameAction> = actions
        .into_iter()
        .filter(|action| action.kind == ActionKind::Play)
        .collect();
    let default = settings
        .default_action
        .as_deref()
        .and_then(|id| play.iter().position(|action| action.id == id))
        .unwrap_or(0);
    if settings.ask && play.len() > 1 {
        return Err(LaunchError::NeedsChoice(Box::new(LaunchPrompt {
            entry_id: entry.id.clone(),
            platform_id: String::new(),
            profiles: vec![],
            discs: vec![],
            actions: play
                .iter()
                .enumerate()
                .map(|(index, action)| ActionOption {
                    id: action.id.clone(),
                    name: action.name.clone(),
                    default: index == default,
                })
                .collect(),
        })));
    }
    play.into_iter().nth(default).ok_or_else(|| {
        LaunchError::Invalid(crate::tr!(
            "this game has nothing to play: add a play action in its editor"
        ))
    })
}

/// The entry as the launch pipeline sees it for `action`: its target in
/// place of the entry's own launch. A native tool for a Windows game (a
/// script, a Linux mod manager) runs without Wine.
pub(crate) fn apply(entry: &GameEntry, action: &GameAction) -> GameEntry {
    let mut entry = entry.clone();
    if action.id == built_in_action_id(&entry.id) {
        return entry;
    }
    if matches!(entry.runtime, GameRuntime::Windows { .. })
        && !matches!(&action.target, LaunchSpec::Executable { path, .. } if windows_program(path))
    {
        entry.runtime = GameRuntime::Native;
    }
    entry.launch = Some(action.target.clone());
    entry
}

fn windows_program(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["exe", "bat", "cmd", "com", "lnk", "msi"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Whether `target` can run as an action of `entry`, with the reason it
/// can't. It doesn't look for the program of a managed install's relative
/// path: the launch does, and names it when it is missing.
pub fn validate_target(entry: &GameEntry, target: &LaunchSpec) -> Result<()> {
    let managed = matches!(entry.location, Some(InstallLocation::Managed { .. }));
    match target {
        LaunchSpec::Uri { uri } => {
            super::uri_command(uri)?;
        }
        LaunchSpec::Executable { path, cwd, .. } => {
            if path.as_os_str().is_empty() {
                return Err(LaunchError::Invalid(crate::tr!(
                    "choose the program the action starts"
                )));
            }
            if path.is_absolute() {
                if !path.is_file() {
                    return Err(LaunchError::Invalid(crate::tr!(
                        "“{path}” isn't a file that can be started; check the path",
                        path = path.display()
                    )));
                }
            } else if !managed && !matches!(entry.location, Some(InstallLocation::External { .. }))
            {
                return Err(LaunchError::Invalid(crate::tr!(
                    "“{path}” is relative, but this release has no install folder: choose the program's full path",
                    path = path.display()
                )));
            }
            if let Some(cwd) = cwd
                && cwd.is_absolute()
                && !cwd.is_dir()
            {
                return Err(LaunchError::Invalid(crate::tr!(
                    "the working folder “{path}” doesn't exist",
                    path = cwd.display()
                )));
            }
        }
        LaunchSpec::Flatpak { app_id, .. } => {
            if !super::command::flatpak_id(app_id) {
                return Err(LaunchError::Invalid(crate::tr!(
                    "“{id}” isn't a Flatpak application id (like org.example.Game)",
                    id = app_id
                )));
            }
        }
        LaunchSpec::Emulator { roms, .. } => {
            if roms.is_empty() {
                return Err(LaunchError::Invalid(crate::tr!(
                    "an emulator action needs the game's ROMs"
                )));
            }
        }
        LaunchSpec::Plugin { .. } | LaunchSpec::Web { .. } => {}
        // Streamed games come from the device that offers them.
        LaunchSpec::Stream { .. } => {
            return Err(LaunchError::Invalid(crate::tr!(
                "an action can't stream a game: streamed games come from the device that offers them"
            )));
        }
    }
    Ok(())
}

/// Whether `action`'s tracking can work for its target on this OS, with
/// what to choose instead. A tool's tracking is never used.
pub fn validate_tracking(entry: &GameEntry, action: &GameAction) -> Result<()> {
    if action.kind == ActionKind::Utility {
        return Ok(());
    }
    let tracking = &action.tracking;
    let mode = tracking.mode;
    let unsupported = |why: String| Err(LaunchError::Invalid(why));
    match (&action.target, mode) {
        (_, TrackingMode::Automatic) => {}
        (LaunchSpec::Emulator { .. }, _) => {
            return unsupported(crate::tr!(
                "an emulator action is tracked as its emulator profile says; keep Automatic"
            ));
        }
        (LaunchSpec::Web { .. }, _) => {
            return unsupported(crate::tr!(
                "a web game is tracked by its game window; keep Automatic"
            ));
        }
        (LaunchSpec::Stream { .. }, _) => {
            return unsupported(crate::tr!(
                "a streamed game is followed by its stream; keep Automatic"
            ));
        }
        (LaunchSpec::Uri { .. }, TrackingMode::OriginalProcess | TrackingMode::ProcessTree) => {
            return unsupported(crate::tr!(
                "a link opens through the system, which hands it on at once: track the game by its folder or its process names"
            ));
        }
        (LaunchSpec::Flatpak { .. }, TrackingMode::Folder) => {
            return unsupported(crate::tr!(
                "a Flatpak runs from inside its sandbox, so its folder can't be followed: track it by process tree or process names"
            ));
        }
        (LaunchSpec::Executable { path, .. }, TrackingMode::OriginalProcess)
            if !cfg!(windows)
                && matches!(entry.runtime, GameRuntime::Windows { .. })
                && windows_program(path) =>
        {
            return unsupported(crate::tr!(
                "a Windows program runs through Wine or Proton here, so the process Caskade starts isn't the game: track it by process tree, folder or process names"
            ));
        }
        _ => {}
    }
    match mode {
        TrackingMode::ProcessName if tracking.names.is_empty() => unsupported(crate::tr!(
            "add the game's process names (like game.exe) to track it by name"
        )),
        TrackingMode::Folder if tracking.folder.is_none() && entry.location.is_none() => {
            unsupported(crate::tr!(
                "choose the folder the game runs from: this release has no install folder"
            ))
        }
        _ => Ok(()),
    }
}

/// Follows the action's tracking: which processes are the game, and
/// whether the started process is one of them.
pub(crate) fn track(prepared: &mut PreparedLaunch, tracking: &ActionTracking) {
    let target = &mut prepared.target;
    match tracking.mode {
        TrackingMode::Automatic => return,
        TrackingMode::OriginalProcess | TrackingMode::ProcessTree => prepared.direct = true,
        TrackingMode::Folder => {
            prepared.direct = false;
            if let Some(folder) = &tracking.folder {
                let folder = match &target.real_dir {
                    Some(install) if !folder.is_absolute() => install.join(folder),
                    _ => folder.clone(),
                };
                target.real_dir = Some(folder);
                target.alias_dir = None;
            }
        }
        TrackingMode::ProcessName => {
            prepared.direct = false;
            target.exe_names = tracking.names.clone();
        }
    }
    target.mode = tracking.mode;
}

/// A release's actions as the game editor shows them (#178).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseActions {
    pub entry_id: String,
    /// Its built-in action's target is the user's (a manual entry, #174),
    /// so it can be changed; otherwise its source keeps it current.
    pub editable_built_in: bool,
    /// Its own folder (a managed install's), which relative paths are in.
    pub has_folder: bool,
    /// Its built-in action first, then the ones the user added.
    pub actions: Vec<GameAction>,
    pub settings: EntryActionSettings,
}

fn invalid(error: LaunchError) -> crate::Error {
    crate::Error::Invalid(match error {
        LaunchError::Invalid(why) => why,
        other => other.to_string(),
    })
}

/// Each release of `game_id` with its actions.
pub async fn game_actions(
    store: &LibraryStore,
    game_id: &str,
) -> crate::Result<Vec<ReleaseActions>> {
    let mut releases = vec![];
    for entry in store.entries_for_game(game_id).await? {
        let settings = store.entry_action_settings(&entry.id).await?;
        let added = store.game_actions(&entry.id).await?;
        releases.push(ReleaseActions {
            entry_id: entry.id.clone(),
            editable_built_in: entry.source_id.as_str() == MANUAL_SOURCE,
            has_folder: entry.location.is_some(),
            actions: entry_actions(&entry, added, &settings),
            settings,
        });
    }
    Ok(releases)
}

/// Saves an action of one of `game_id`'s releases, after checking its
/// target and tracking. An action without a release goes to the game's
/// manual release, made for it when the game has none (a title-only
/// record, #174); a play action made that way becomes its built-in action.
/// The built-in action keeps its target unless the release is manual.
pub async fn save_action(
    store: &LibraryStore,
    game_id: &str,
    action: &GameAction,
) -> crate::Result<GameAction> {
    let mut action = action.clone();
    if action.entry_id.is_empty() {
        let entries = store.entries_for_game(game_id).await?;
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.source_id.as_str() == MANUAL_SOURCE)
            .or(entries.first())
        {
            action.entry_id = entry.id.clone();
        } else {
            let mut entry = GameEntry::new(
                game_id,
                caskade_discovery::SourceId::new(MANUAL_SOURCE),
                crate::model::new_id(),
            );
            entry.state = caskade_discovery::InstallState::Installed;
            validate_target(&entry, &action.target).map_err(invalid)?;
            if action.kind == ActionKind::Play {
                entry.launch = Some(action.target.clone());
                action.id = built_in_action_id(&entry.id);
            }
            action.entry_id = store.upsert_entry(&entry).await?.id;
        }
    }
    let entry = store
        .get_entry(&action.entry_id)
        .await?
        .filter(|entry| entry.game_id == game_id)
        .ok_or_else(|| crate::db::missing("entry", &action.entry_id))?;
    // The built-in target as its source left it is the source's to check.
    let built_in = action.id == built_in_action_id(&entry.id);
    let retarget = entry.launch.as_ref() != Some(&action.target);
    if built_in && retarget && entry.source_id.as_str() != MANUAL_SOURCE {
        return Err(crate::Error::Invalid(crate::tr!(
            "this release's own launch comes from where it was found, which keeps it current: duplicate it to change it"
        )));
    }
    if !built_in || retarget {
        validate_target(&entry, &action.target).map_err(invalid)?;
    }
    validate_tracking(&entry, &action).map_err(invalid)?;
    if built_in {
        return save_built_in(store, entry, action).await;
    }
    if action.id.is_empty() {
        action.id = crate::model::new_id();
    }
    store.save_game_action(&action).await
}

async fn save_built_in(
    store: &LibraryStore,
    mut entry: GameEntry,
    action: GameAction,
) -> crate::Result<GameAction> {
    if action.kind != ActionKind::Play {
        return Err(crate::Error::Invalid(crate::tr!(
            "the release's own launch plays the game; add a tool as a new action"
        )));
    }
    if entry.launch.as_ref() != Some(&action.target) {
        entry.launch = Some(action.target.clone());
        entry = store.upsert_entry(&entry).await?;
    }
    let mut settings = store.entry_action_settings(&entry.id).await?;
    settings.built_in = crate::model::BuiltInAction {
        name: Some(action.name.clone()).filter(|name| *name != crate::tr!("Play")),
        tracking: Some(action.tracking.clone())
            .filter(|tracking| *tracking != ActionTracking::default()),
    };
    let settings = store
        .save_entry_action_settings(&entry.id, &settings)
        .await?;
    built_in_action(&entry, &settings)
        .ok_or_else(|| crate::Error::Invalid(crate::tr!("the release has no launch of its own")))
}

/// Deletes an action of `game_id`; the built-in action can't be.
pub async fn delete_action(store: &LibraryStore, game_id: &str, id: &str) -> crate::Result<bool> {
    let Some(action) = store.game_action(id).await? else {
        return if id.starts_with(crate::model::BUILT_IN_PREFIX) {
            Err(crate::Error::Invalid(crate::tr!(
                "a release's own launch can't be deleted"
            )))
        } else {
            Ok(false)
        };
    };
    let owner = store.get_entry(&action.entry_id).await?;
    if owner.is_none_or(|entry| entry.game_id != game_id) {
        return Err(crate::db::missing("action", id));
    }
    store.delete_game_action(id).await
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
