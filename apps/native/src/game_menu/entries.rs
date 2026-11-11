//! A game's entries as its menu and quick look read them, as the webview
//! maps them (`api/mapping.ts`'s `mapGame`, `GameMenu.tsx`'s derived
//! values): where each lives, whether it can launch, and which the menu's
//! Play starts.

use caskade_core::model::{GameDetails, GameEntry, InstallLocation, LaunchSpec};
use caskade_ui_i18n::t;
use std::collections::HashSet;

/// One entry, as the menu sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryView {
    pub id: String,
    /// Its source's id: `caskade-storage`, `web`, `manual`, `steam`…
    pub source: String,
    pub installed: bool,
    /// Where its files are, as people read it.
    pub location: Option<String>,
    pub can_launch: bool,
    pub emulated: bool,
    pub stream: bool,
}

impl EntryView {
    pub fn of(entry: &GameEntry) -> Self {
        let installed = matches!(entry.state, caskade_core::model::InstallState::Installed);
        let stream = matches!(entry.launch, Some(LaunchSpec::Stream { .. }));
        Self {
            id: entry.id.clone(),
            source: entry.source_id.as_str().to_owned(),
            installed,
            location: entry.location.as_ref().map(|location| match location {
                InstallLocation::External { path } => path.display().to_string(),
                InstallLocation::Managed { rel_dir, .. } => t!(
                    "{folder} in game storage",
                    folder = rel_dir.display().to_string()
                ),
            }),
            // Streamed from another device (#264): nothing is installed here.
            can_launch: stream
                || (installed
                    && entry.launch.is_some()
                    && !matches!(entry.launch, Some(LaunchSpec::Plugin { .. }))),
            emulated: matches!(entry.launch, Some(LaunchSpec::Emulator { .. })),
            stream,
        }
    }

    pub fn managed(&self) -> bool {
        self.source == "caskade-storage"
    }

    pub fn web(&self) -> bool {
        self.source == "web"
    }
}

/// The entries of `game`.
pub fn entries(game: &GameDetails) -> Vec<EntryView> {
    game.entries.iter().map(EntryView::of).collect()
}

/// The entry Play starts: the game's default when it can launch and is
/// reachable, else the first one that can (`playableEntry`).
pub fn playable<'a>(
    entries: &'a [EntryView],
    unavailable: &HashSet<String>,
    preferred: Option<&str>,
) -> Option<&'a EntryView> {
    let can = |entry: &&EntryView| entry.can_launch && !unavailable.contains(&entry.id);
    entries
        .iter()
        .filter(can)
        .find(|entry| Some(entry.id.as_str()) == preferred)
        .or_else(|| entries.iter().find(can))
}

/// The entries with files somewhere, one per folder.
pub fn located(entries: &[EntryView]) -> Vec<&EntryView> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .filter(|entry| entry.location.is_some())
        .filter(|entry| seen.insert(entry.location.clone()))
        .collect()
}

/// Whether Caskade added every entry (it can remove the game).
pub fn added_by_caskade(entries: &[EntryView]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(
                entry.source.as_str(),
                "caskade-storage" | "caskade-handoff" | "manual"
            )
        })
}
