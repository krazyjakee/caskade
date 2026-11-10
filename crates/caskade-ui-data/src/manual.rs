//! Games added by hand (#174, #108): what a program, shortcut or URI
//! starts, games that may be the same, and adding the record, through the
//! app layer's commands as the webview's `api/manualGames.ts` does.

use caskade_app::commands::manual_games;
use caskade_core::{
    error::ErrorReport,
    manual::{DuplicateGame, ManualGame, ManualTarget},
};

use crate::Library;

type Result<T> = std::result::Result<T, ErrorReport>;

impl Library {
    /// What `input` (a path, a shortcut or a URI) starts, with the title it
    /// suggests; refused with the reason when it can't start a game.
    pub async fn manual_target(&self, input: String) -> Result<ManualTarget> {
        manual_games::manual_game_target(input).await
    }

    /// Games already in the library that may be the one being added.
    pub async fn manual_duplicates(
        &self,
        title: String,
        target: Option<ManualTarget>,
    ) -> Result<Vec<DuplicateGame>> {
        manual_games::manual_game_duplicates(self.store(), &title, target.as_ref()).await
    }

    /// Adds the game; its id (the linked game's, when linked). The library
    /// hears it added or updated.
    pub async fn add_manual_game(&self, game: ManualGame) -> Result<String> {
        manual_games::manual_game_add(self.store(), self.bus(), game).await
    }
}
