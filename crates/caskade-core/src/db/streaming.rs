//! Games a paired streaming host offers (#262): each
//! app is an uninstalled release under source `stream:<host id>`, joined to
//! the library game with the same title, or a streaming-only game of its
//! own. Sync (#232) will join them by the game's identity instead.

use super::{
    LibraryStore,
    games::{save_entry, save_game},
};
use crate::{
    Result,
    model::{Game, GameEntry, InstallState, LaunchSpec, SourceId},
};
use std::collections::{BTreeMap, BTreeSet};

/// The source of a host's releases.
pub fn stream_source(host_id: &str) -> SourceId {
    format!("stream:{host_id}").into()
}

/// An app a host offers: its id there, and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamApp {
    pub id: String,
    pub name: String,
}

/// What a sync changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamSync {
    /// Streaming-only games made for apps no library game matched.
    pub added: Vec<String>,
    /// Library games that gained or lost a release from the host.
    pub updated: Vec<String>,
    /// Streaming-only games deleted with their last release.
    pub removed: Vec<String>,
}

/// How a host's app launches: streamed, by its name there.
fn stream_launch(host_id: &str, app: &StreamApp) -> LaunchSpec {
    LaunchSpec::Stream {
        host: host_id.to_owned(),
        app: app.name.clone(),
    }
}

/// A title as a key: case, accents' composition and punctuation aside.
fn title_key(title: &str) -> String {
    icu_normalizer::ComposingNormalizer::new_nfkc()
        .normalize(title)
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

impl LibraryStore {
    /// Makes the host's releases exactly `apps`: new ones join the library
    /// game of the same title (one with a release of its own, not hidden or
    /// kept apart), else a game of their own; ones the host no longer lists
    /// go, and a game left with no release goes with them. Joining only
    /// games with a release of their own keeps a title-only record (#174)
    /// from going with a host's app.
    pub async fn sync_stream_apps(&self, host_id: &str, apps: &[StreamApp]) -> Result<StreamSync> {
        self.sync_stream_apps_from(host_id, apps, None).await
    }

    /// [`Self::sync_stream_apps`] for a host on `device`, a device paired
    /// through sync (#267): an app joins the game of that device's own
    /// installed copy (#232), named by sync's identity rather than by a
    /// title anywhere in the library; only apps none of its copies match
    /// fall back to the title.
    pub async fn sync_stream_apps_from(
        &self,
        host_id: &str,
        apps: &[StreamApp],
        device: Option<&str>,
    ) -> Result<StreamSync> {
        let source = stream_source(host_id);
        let mut tx = self.begin_write().await?;
        let known: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id,game_id,external_id FROM game_entries WHERE source_id=?")
                .bind(source.as_str())
                .fetch_all(&mut *tx)
                .await?;
        let mut sync = StreamSync::default();
        for (entry_id, game_id, external_id) in &known {
            if let Some(app) = apps.iter().find(|app| app.id == *external_id) {
                // Each release streams its app by name (#264); releases
                // from before that, or an app renamed in place, catch up.
                let launch = serde_json::to_string(&stream_launch(host_id, app))?;
                let changed = sqlx::query(
                    "UPDATE game_entries SET launch_kind='stream',\
                     record_json=json_set(record_json,'$.launch',json(?)) \
                     WHERE id=? AND json(coalesce(json_extract(record_json,'$.launch'),'null')) \
                     IS NOT json(?)",
                )
                .bind(&launch)
                .bind(entry_id)
                .bind(&launch)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                    > 0;
                if changed {
                    sync.updated.push(game_id.clone());
                }
                continue;
            }
            sqlx::query("DELETE FROM game_entries WHERE id=?")
                .bind(entry_id)
                .execute(&mut *tx)
                .await?;
            let gone = sqlx::query(
                "DELETE FROM games WHERE id=? AND NOT EXISTS(SELECT 1 FROM game_entries e \
                 WHERE e.game_id=games.id)",
            )
            .bind(game_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0;
            if gone {
                sync.removed.push(game_id.clone());
            } else {
                sync.updated.push(game_id.clone());
            }
        }
        let have: BTreeSet<&str> = known.iter().map(|(_, _, id)| id.as_str()).collect();
        let new: Vec<&StreamApp> = apps
            .iter()
            .filter(|app| !have.contains(app.id.as_str()))
            .collect();
        if !new.is_empty() {
            let titles: Vec<(String, String)> = sqlx::query_as(
                "SELECT g.id,g.title FROM games g WHERE g.hidden=0 \
                 AND NOT EXISTS(SELECT 1 FROM kept_apart k WHERE k.game_id=g.id) \
                 AND EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id) ORDER BY g.rowid",
            )
            .fetch_all(&mut *tx)
            .await?;
            let mut by_title: BTreeMap<String, String> = BTreeMap::new();
            for (id, title) in titles {
                by_title.entry(title_key(&title)).or_insert(id);
            }
            // The games the host's device itself has installed come first.
            if let Some(device) = device {
                let copies: Vec<(String, String)> = sqlx::query_as(
                    "SELECT g.id,g.title FROM games g WHERE g.hidden=0 \
                     AND EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id \
                     AND e.source_id=?) ORDER BY g.rowid DESC",
                )
                .bind(format!("{}{device}", super::sync_copies::PEER_PREFIX))
                .fetch_all(&mut *tx)
                .await?;
                for (id, title) in copies {
                    by_title.insert(title_key(&title), id);
                }
            }
            for app in new {
                let key = title_key(&app.name);
                let game_id = match by_title.get(&key).filter(|_| !key.is_empty()) {
                    Some(game_id) => {
                        sync.updated.push(game_id.clone());
                        game_id.clone()
                    }
                    None => {
                        let game = Game::new(&app.name);
                        save_game(&mut tx, &game).await?;
                        by_title.insert(key, game.id.clone());
                        sync.added.push(game.id.clone());
                        game.id
                    }
                };
                let mut entry = GameEntry::new(&game_id, source.clone(), &app.id);
                entry.state = InstallState::Partial;
                entry.launch = Some(stream_launch(host_id, app));
                save_entry(&mut tx, &entry.normalized()?).await?;
            }
        }
        tx.commit().await?;
        sync.updated.sort();
        sync.updated.dedup();
        sync.updated.retain(|id| !sync.added.contains(id));
        Ok(sync)
    }
}
