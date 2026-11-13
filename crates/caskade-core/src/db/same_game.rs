//! One library game per IGDB game: the games matching found to be the same
//! IGDB game (a ROM on each of several platforms), and entries the user
//! separates from them again (`kept_apart`). The service lives in
//! `crate::metadata`.

use super::{LibraryStore, games::write_game, missing, sync_identity::normal_title};
use crate::{
    Result,
    model::{validation as v, *},
};
use std::collections::HashMap;

/// Games of the same title released this many years apart or less are
/// one game: a port, or the same game on another console.
pub const SAME_TITLE_YEARS: i32 = 5;

impl LibraryStore {
    /// The visible games matching has looked at that are one game, a group
    /// each: the same IGDB id, or (IGDB gives many ports ids of their own,
    /// and RomM leaves some releases without one) the same title released
    /// within [`SAME_TITLE_YEARS`] of another. A game without a match joins
    /// only by title and era; one the user said is on no IGDB game, or
    /// whose date is unknown, joins nothing by title. Each group starts
    /// with the game the others merge into: one matched by hand, then a
    /// matched one, then the one with the most entries, then the oldest.
    /// Hidden games and those kept apart stay out.
    pub async fn same_games(&self) -> Result<Vec<Vec<String>>> {
        let rows: Vec<(String, Option<i64>, String, Option<String>)> = sqlx::query_as(
            "SELECT g.id,m.igdb_id,g.title,g.record_json->>'$.releaseDate' \
             FROM igdb_matches m JOIN games g ON g.id=m.game_id \
             WHERE (m.igdb_id IS NOT NULL OR m.source='none') AND g.hidden=0 \
             AND NOT EXISTS(SELECT 1 FROM kept_apart k WHERE k.game_id=g.id) \
             ORDER BY m.source='manual' DESC,m.igdb_id IS NOT NULL DESC,\
             (SELECT count(*) FROM game_entries e WHERE e.game_id=g.id) DESC,g.rowid",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut sets = Sets((0..rows.len()).collect());
        let mut by_id = HashMap::new();
        // Each title's releases: the year, when known.
        let mut by_title: HashMap<String, Vec<(Option<i32>, usize)>> = HashMap::new();
        for (index, (_, igdb_id, title, released)) in rows.iter().enumerate() {
            if let Some(igdb_id) = igdb_id
                && let Some(first) = by_id.insert(*igdb_id, index)
            {
                sets.join(first, index);
            }
            let year = released
                .as_deref()
                .and_then(|date| date.get(..4)?.parse().ok());
            by_title
                .entry(normal_title(title))
                .or_default()
                .push((year, index));
        }
        for (title, releases) in &mut by_title {
            releases.sort_unstable();
            for pair in releases.windows(2) {
                if let (Some(a), Some(b)) = (pair[0].0, pair[1].0)
                    && b - a <= SAME_TITLE_YEARS
                {
                    sets.join(pair[0].1, pair[1].1);
                }
            }
            explain_apart(title, releases, &mut sets);
        }
        // Rows are in keep order, and so is each group.
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut group_of = HashMap::new();
        for (index, (game_id, ..)) in rows.into_iter().enumerate() {
            let root = sets.root(index);
            let group = *group_of.entry(root).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[group].push(game_id);
        }
        groups.retain(|group| group.len() > 1);
        Ok(groups)
    }

    /// Moves an entry, with its play sessions, out of its game into a game
    /// of its own that IGDB matching never merges again. The new game is a
    /// copy of the old one (its match, artwork and collections too) on
    /// `platforms`; the old one keeps `left`.
    pub async fn separate_entry(
        &self,
        entry_id: &str,
        platforms: &[String],
        left: &[String],
    ) -> Result<Game> {
        let mut tx = self.begin_write().await?;
        let game_id: String = sqlx::query_scalar("SELECT game_id FROM game_entries WHERE id=?")
            .bind(entry_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| missing("entry", entry_id))?;
        let others: i64 =
            sqlx::query_scalar("SELECT count(*) FROM game_entries WHERE game_id=? AND id<>?")
                .bind(&game_id)
                .bind(entry_id)
                .fetch_one(&mut *tx)
                .await?;
        if others == 0 {
            return Err(crate::Error::Invalid(crate::tr!(
                "entry {entry} is its game's only entry",
                entry = entry_id
            )));
        }
        let (mut old, mut overrides) = super::game_edit::load_layers(&mut tx, &game_id)
            .await?
            .ok_or_else(|| missing("game", &game_id))?;
        // The split is the user's: both games show the platforms they were
        // given, beneath the user's fields as well (#171), and the new one
        // keeps the fields the user set.
        let platforms_key = GameField::Platforms.key();
        let mut game = old.clone();
        game.id = new_id();
        game.platforms = platforms.to_vec();
        game.default_entry = None;
        let mut game_overrides = overrides.clone();
        if let Some(value) = game_overrides.get_mut(platforms_key) {
            *value = serde_json::to_value(platforms)?;
        }
        // Split off a game already counted: not an addition (#198).
        super::notifications::not_an_addition(&mut tx, &game.id).await?;
        write_game(&mut tx, &game, Some(&game_overrides)).await?;
        old.platforms = left.to_vec();
        if old.default_entry.as_deref() == Some(entry_id) {
            old.default_entry = None;
        }
        if let Some(value) = overrides.get_mut(platforms_key) {
            *value = serde_json::to_value(left)?;
        }
        write_game(&mut tx, &old, Some(&overrides)).await?;
        // Its sessions follow through the composite key's ON UPDATE CASCADE.
        sqlx::query("UPDATE game_entries SET game_id=?,record_json=json_set(record_json,'$.gameId',?) WHERE id=?")
            .bind(&game.id).bind(&game.id).bind(entry_id).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE romm_roms SET record_json=json_set(record_json,'$.gameId',?) WHERE entry_id=?",
        )
        .bind(&game.id)
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
        let artwork: Vec<String> =
            sqlx::query_scalar("SELECT record_json FROM artwork WHERE game_id=?")
                .bind(&game_id)
                .fetch_all(&mut *tx)
                .await?;
        for record in artwork {
            let mut art: Artwork = super::decode(record)?;
            let from = std::mem::replace(&mut art.id, new_id());
            art.game_id = game.id.clone();
            sqlx::query("INSERT INTO artwork(id,game_id,kind,content_hash,record_json) SELECT ?,?,kind,content_hash,? FROM artwork WHERE id=?")
                .bind(&art.id).bind(&game.id).bind(v::json(&art)?).bind(from).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO igdb_matches(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json) SELECT ?,igdb_id,source,pending,checked_at_ms,dumps,applied_json FROM igdb_matches WHERE game_id=?")
            .bind(&game.id).bind(&game_id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO collection_members(collection_id,game_id,position) SELECT collection_id,?,position FROM collection_members WHERE game_id=?")
            .bind(&game.id).bind(&game_id).execute(&mut *tx).await?;
        super::artwork_choice::copy_choices(&mut tx, &game_id, &game.id).await?;
        sqlx::query("INSERT INTO kept_apart(game_id) VALUES(?)")
            .bind(&game.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(game)
    }
}

/// Says why games sharing `title` stay more than one game, for the logs:
/// a release date is unknown, or the releases are too far apart.
fn explain_apart(title: &str, releases: &[(Option<i32>, usize)], sets: &mut Sets) {
    let Some(((_, first), rest)) = releases.split_first() else {
        return;
    };
    let root = sets.root(*first);
    if rest.iter().all(|(_, index)| sets.root(*index) == root) {
        return;
    }
    let undated = releases.iter().filter(|(year, _)| year.is_none()).count();
    let years: Vec<i32> = releases.iter().filter_map(|(year, _)| *year).collect();
    tracing::debug!(
        title,
        games = releases.len(),
        undated,
        ?years,
        max_years_apart = SAME_TITLE_YEARS,
        "same title kept as separate games: an unknown release date, or releases too far apart"
    );
}

/// Disjoint sets of row indices.
struct Sets(Vec<usize>);

impl Sets {
    fn root(&mut self, mut index: usize) -> usize {
        while self.0[index] != index {
            self.0[index] = self.0[self.0[index]];
            index = self.0[index];
        }
        index
    }

    /// Joins the sets of `a` and `b`; the one first in keep order leads.
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        let (first, second) = (a.min(b), a.max(b));
        self.0[second] = first;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        db::LibraryStore,
        metadata::{IgdbMatch, MatchSource},
        model::Game,
    };

    async fn add(
        store: &LibraryStore,
        title: &str,
        released: Option<&str>,
        igdb_id: u64,
    ) -> String {
        add_as(store, title, released, Some(igdb_id), MatchSource::Romm).await
    }

    /// A game matching has looked at: matched to `igdb_id`, or not.
    async fn add_as(
        store: &LibraryStore,
        title: &str,
        released: Option<&str>,
        igdb_id: Option<u64>,
        source: MatchSource,
    ) -> String {
        let mut game = Game::new(title);
        game.release_date = released.map(str::to_owned);
        store.upsert_game(&game).await.unwrap();
        let row = IgdbMatch {
            game_id: game.id.clone(),
            igdb_id,
            source,
            pending: false,
            checked_at_ms: 0,
            dumps: String::new(),
            applied: Default::default(),
        };
        store.save_igdb_match(&row).await.unwrap();
        game.id
    }

    async fn unmatched(store: &LibraryStore, title: &str, released: Option<&str>) -> String {
        add_as(store, title, released, None, MatchSource::None).await
    }

    #[tokio::test]
    async fn ports_of_one_title_and_era_are_one_game() {
        let store = LibraryStore::in_memory().await.unwrap();
        let snes = add(&store, "Mortal Kombat 3", Some("1995-04-15"), 1620).await;
        let ps = add(&store, "Mortal Kombat 3", Some("1995-10-01"), 1620).await;
        // IGDB's ports have ids of their own.
        let gb = add(&store, "MORTAL KOMBAT 3", Some("1995-11-01"), 242251).await;
        let sms = add(&store, "Mortal Kombat 3", Some("1996-01-01"), 242253).await;
        // Same title, another era; a title without a date; another title.
        let reboot = add(&store, "Mortal Kombat", Some("2011-04-19"), 1).await;
        let arcade = add(&store, "Mortal Kombat", Some("1992-10-08"), 2).await;
        let genesis = add(&store, "Mortal Kombat", Some("1993-09-13"), 3).await;
        add(&store, "Mortal Kombat 3", None, 4).await;
        add(&store, "Mortal Kombat 4", Some("1997-10-15"), 5).await;
        let hidden = add(&store, "Mortal Kombat 3", Some("1995-04-15"), 1620).await;
        store.set_game_hidden(&hidden, true).await.unwrap();
        let mut groups = store.same_games().await.unwrap();
        groups.sort();
        let mut expected = vec![vec![snes, ps, gb, sms], vec![arcade, genesis]];
        expected.sort();
        assert_eq!(groups, expected);
        assert!(!groups.concat().contains(&reboot));
    }

    #[tokio::test]
    async fn releases_left_without_a_match_join_by_title_and_era() {
        let store = LibraryStore::in_memory().await.unwrap();
        // RomM matched the N64 ROM, not the PlayStation ones (two regions).
        let psx = unmatched(&store, "A Bug's Life", Some("1998-11-18")).await;
        let n64 = add(&store, "A Bug's Life", Some("1998-11-01"), 2847).await;
        let europe = unmatched(&store, "A BUG'S LIFE", Some("1999-02-01")).await;
        // Releases nobody matched are one game too.
        let disc1 = unmatched(&store, "Beatmania", Some("1998-10-01")).await;
        let disc2 = unmatched(&store, "Beatmania", Some("1998-10-01")).await;
        // Not the same game: a sequel, another era, an unknown date, one
        // the user said is on no IGDB game, one kept apart, a hidden one.
        let sequel = unmatched(&store, "A Bug's Life 2", Some("1999-06-01")).await;
        let remake = unmatched(&store, "A Bug's Life", Some("2024-01-01")).await;
        let undated = unmatched(&store, "A Bug's Life", None).await;
        let manual = add_as(
            &store,
            "A Bug's Life",
            Some("1998-11-18"),
            None,
            MatchSource::Manual,
        )
        .await;
        let apart = unmatched(&store, "A Bug's Life", Some("1998-11-18")).await;
        sqlx::query("INSERT INTO kept_apart(game_id) VALUES(?)")
            .bind(&apart)
            .execute(&store.pool)
            .await
            .unwrap();
        let hidden = unmatched(&store, "A Bug's Life", Some("1998-11-18")).await;
        store.set_game_hidden(&hidden, true).await.unwrap();

        let mut groups = store.same_games().await.unwrap();
        groups.sort();
        // The matched game leads its group, though the others are older.
        let mut expected = vec![vec![n64, psx, europe], vec![disc1, disc2]];
        expected.sort();
        assert_eq!(groups, expected);
        let grouped = groups.concat();
        for game in [sequel, remake, undated, manual, apart, hidden] {
            assert!(!grouped.contains(&game));
        }
    }
}
