//! Games added by hand (#174): duplicates of one being added, and adding
//! it. What a target starts is worked out in `crate::manual`.

use super::{LibraryStore, sync_identity::normal_title};
use crate::{
    Error, Result,
    launch::validate_target,
    manual::{DuplicateGame, DuplicateReason, ManualGame, ManualTarget, entry_for},
    model::*,
};

impl LibraryStore {
    /// Games already in the library that may be the one being added: one
    /// that starts the same program or URI, or has the same title.
    pub async fn manual_duplicates(
        &self,
        title: &str,
        target: Option<&ManualTarget>,
    ) -> Result<Vec<DuplicateGame>> {
        let mut found: Vec<DuplicateGame> = Vec::new();
        if let Some(target) = target {
            // The same program, or the same URI.
            let (kind, key, value) = match &target.launch {
                LaunchSpec::Executable { path, .. } => (
                    "executable",
                    "$.launch.path",
                    path.to_string_lossy().into_owned(),
                ),
                LaunchSpec::Uri { uri } => ("uri", "$.launch.uri", uri.clone()),
                _ => ("", "", String::new()),
            };
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT game_id FROM game_entries \
                 WHERE record_json->>'$.launch.type'=? AND record_json->>?=?",
            )
            .bind(kind)
            .bind(key)
            .bind(&value)
            .fetch_all(&self.pool)
            .await?;
            for id in ids {
                if let Some(game) = self.game(&id).await? {
                    found.push(DuplicateGame {
                        game_id: id,
                        title: game.title,
                        reason: DuplicateReason::SameTarget,
                    });
                }
            }
        }
        let wanted = normal_title(title);
        if !wanted.is_empty() {
            let page = self
                .list_games(&ListGames {
                    // Words alone: "PORTAL: two" finds "Portal Two".
                    query: Some(wanted.chars().take(512).collect()),
                    limit: 100,
                    ..Default::default()
                })
                .await?;
            for summary in page.items {
                if found.iter().any(|known| known.game_id == summary.id) {
                    continue;
                }
                let Some(game) = self.game(&summary.id).await? else {
                    continue;
                };
                let same = std::iter::once(&game.title)
                    .chain(&game.aliases)
                    .any(|name| normal_title(name) == wanted);
                if same {
                    found.push(DuplicateGame {
                        game_id: game.id,
                        title: game.title,
                        reason: DuplicateReason::SameTitle,
                    });
                }
            }
        }
        Ok(found)
    }

    /// Adds a game by hand and returns its id: a new game (title-only, or
    /// with its target as a `manual` release), or the target as a release
    /// of `link_to`. Nothing on disk is touched.
    pub async fn add_manual_game(&self, request: &ManualGame) -> Result<String> {
        let title = request.title.trim();
        if let Some(target) = &request.target {
            validate_target(&entry_for("check", target), &target.launch)
                .map_err(|error| Error::Invalid(error.to_string()))?;
        }
        if let Some(existing) = &request.link_to {
            let game = self
                .game(existing)
                .await?
                .ok_or_else(|| super::missing("game", existing))?;
            let Some(target) = &request.target else {
                return Err(Error::Invalid(crate::tr!(
                    "only a game with something to start joins another"
                )));
            };
            self.upsert_entry(&entry_for(&game.id, target)).await?;
            return Ok(game.id);
        }
        let game = Game::new(title);
        game.validate()?;
        let id = match &request.target {
            Some(target) => {
                let entry = entry_for(&game.id, target);
                self.import_group(&game, &[entry], None).await?.game_id
            }
            None => self.upsert_game(&game).await?.id,
        };
        if request.keep_apart {
            sqlx::query("INSERT OR IGNORE INTO kept_apart(game_id) VALUES(?)")
                .bind(&id)
                .execute(&self.pool)
                .await?;
        }
        Ok(id)
    }
}
