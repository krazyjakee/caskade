//! Named launch actions (#177): a release's ways to start the game and its
//! related tools. The entry's own `launch` is its built-in play action; users
//! add actions beside it, choose the default or ask at launch.
use super::LaunchSpec;
use crate::tracking::TrackingMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// The source of entries the user adds by hand (#174). Their built-in
/// action's target is the user's, so the action editor may change it.
pub const MANUAL_SOURCE: &str = "manual";

/// The built-in action's id for an entry: its own `launch`.
pub fn built_in_action_id(entry_id: &str) -> String {
    format!("{BUILT_IN_PREFIX}{entry_id}")
}
pub(crate) const BUILT_IN_PREFIX: &str = "source:";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Starts the game: a play session tracks it.
    #[default]
    Play,
    /// A related tool (configuration, mod manager setup, …): it runs without
    /// a play session, playtime or last played.
    Utility,
}

/// How a play action's game is found once it starts. `Automatic` is the
/// release's usual tracking; the others say exactly which processes count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionTracking {
    pub mode: TrackingMode,
    /// `Folder`: processes started from here. Relative to a managed install
    /// (so it moves with it); empty means the install folder.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// `ProcessName`: the game's process names (`game.exe`), e.g. for a mod
    /// manager that starts the game itself.
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAction {
    pub id: String,
    pub entry_id: String,
    pub name: String,
    pub kind: ActionKind,
    /// What it starts. A managed release's executable paths are relative to
    /// its install folder, as in its own launch.
    pub target: LaunchSpec,
    #[serde(default)]
    pub tracking: ActionTracking,
}

/// The built-in action as the user changed it: its name and tracking. Its
/// target stays the entry's `launch`, which rediscovery keeps current.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInAction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tracking: Option<ActionTracking>,
}

/// A release's choice of action, apart from the game's default release.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryActionSettings {
    /// The play action Play starts; `None` (or one that is gone) is the
    /// built-in action.
    #[serde(default)]
    pub default_action: Option<String>,
    /// Play asks which play action to start when there are several.
    #[serde(default)]
    pub ask: bool,
    #[serde(default)]
    pub built_in: BuiltInAction,
}
