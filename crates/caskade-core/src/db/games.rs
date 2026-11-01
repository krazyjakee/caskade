use super::{
    LibraryStore, decode,
    game_edit::{Overrides, automatic, load_layers, protect},
    missing,
};
use crate::{
    Error, Result,
    model::{validation as v, *},
};
use sqlx::{Sqlite, SqliteConnection, Transaction};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Saves `game` from a writer that doesn't know which fields the user set
/// (#171): the user's values stay, and where `game` differs from them it
/// gives the automatic value beneath. Returns the game as saved.
pub(super) async fn save_game(connection: &mut SqliteConnection, game: &Game) -> Result<Game> {
    game.validate()?;
    match load_layers(connection, &game.id).await? {
        Some((shown, mut overrides)) if !overrides.is_empty() => {
            let next = protect(&shown, &mut overrides, game)?;
            write_game(connection, &next, Some(&overrides)).await?;
            Ok(next)
        }
        _ => {
            write_game(connection, game, None).await?;
            Ok(game.clone())
        }
    }
}
/// Writes `game` as the library shows it, and `overrides` when given; a new
/// game without them has no fields set by hand.
pub(super) async fn write_game(
    connection: &mut SqliteConnection,
    game: &Game,
    overrides: Option<&Overrides>,
) -> Result<()> {
    game.validate()?;
    let overrides = overrides.map(v::json).transpose()?;
    sqlx::query("INSERT INTO games(id,title,sort_title,aliases_json,tags_json,favorite,hidden,record_json,overrides_json) VALUES(?,?,?,?,?,?,?,?,coalesce(?,'{}')) ON CONFLICT(id) DO UPDATE SET title=excluded.title,sort_title=excluded.sort_title,aliases_json=excluded.aliases_json,tags_json=excluded.tags_json,favorite=excluded.favorite,hidden=excluded.hidden,record_json=excluded.record_json,overrides_json=CASE WHEN ? IS NULL THEN games.overrides_json ELSE excluded.overrides_json END")
        .bind(&game.id).bind(&game.title).bind(&game.sort_title).bind(v::json(&game.aliases)?).bind(v::json(&game.tags)?).bind(game.favorite).bind(game.hidden).bind(v::json(game)?).bind(&overrides).bind(&overrides).execute(connection).await?;
    Ok(())
}
impl LibraryStore {
    /// Saves a game from an automatic source, keeping the fields the user
    /// set (#171); the game as saved.
    pub async fn upsert_game(&self, game: &Game) -> Result<Game> {
        let mut tx = self.pool.begin().await?;
        let game = save_game(&mut tx, game).await?;
        tx.commit().await?;
        Ok(game)
    }
    /// A single transaction for imports, including the search-index triggers.
    pub async fn upsert_games(&self, games: &[Game]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for game in games {
            save_game(&mut tx, game).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn game(&self, id: &str) -> Result<Option<Game>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM games WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }
    /// Hides a game from every library view, or shows it again. The game
    /// keeps its entries, so re-imports find it and it stays hidden.
    pub async fn set_game_hidden(&self, id: &str, hidden: bool) -> Result<Game> {
        let mut tx = self.pool.begin().await?;
        let mut game = load_game(&mut tx, id).await?;
        game.hidden = hidden;
        save_game(&mut tx, &game).await?;
        tx.commit().await?;
        Ok(game)
    }
    /// Marks a game as a favourite, or not. Re-imports keep the mark, and a
    /// merge keeps it when either game had it.
    pub async fn set_game_favorite(&self, id: &str, favorite: bool) -> Result<Game> {
        let mut tx = self.pool.begin().await?;
        let mut game = load_game(&mut tx, id).await?;
        game.favorite = favorite;
        save_game(&mut tx, &game).await?;
        tx.commit().await?;
        Ok(game)
    }
    /// Chooses the entry the game's Play starts, or goes back to the first
    /// that can launch (`None`).
    pub async fn set_game_default_entry(&self, id: &str, entry: Option<&str>) -> Result<Game> {
        let mut tx = self.pool.begin().await?;
        let mut game = load_game(&mut tx, id).await?;
        if let Some(entry) = entry {
            let owner: Option<String> =
                sqlx::query_scalar("SELECT game_id FROM game_entries WHERE id=?")
                    .bind(entry)
                    .fetch_optional(&mut *tx)
                    .await?;
            if owner.as_deref() != Some(id) {
                return Err(Error::Invalid(crate::tr!(
                    "entry {entry} is not one of game {id}'s",
                    entry = entry,
                    id = id
                )));
            }
        }
        game.default_entry = entry.map(str::to_owned);
        save_game(&mut tx, &game).await?;
        tx.commit().await?;
        Ok(game)
    }
    pub async fn delete_game(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM games WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
    pub async fn upsert_entry(&self, entry: &GameEntry) -> Result<GameEntry> {
        let mut entry = entry.normalized()?;
        let mut tx = self.pool.begin().await?;
        if let Some((id, game_id)) = sqlx::query_as::<_, (String, String)>(
            "SELECT id,game_id FROM game_entries WHERE source_id=? AND external_id=?",
        )
        .bind(entry.source_id.as_str())
        .bind(&entry.external_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if game_id != entry.game_id {
                return Err(Error::Invalid(crate::tr!(
                    "source entry belongs to another game; merge the games first"
                )));
            }
            entry.id = id;
        }
        save_entry(&mut tx, &entry).await?;
        tx.commit().await?;
        Ok(entry)
    }
    pub async fn get_entry(&self, id: &str) -> Result<Option<GameEntry>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM game_entries WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }
    /// The folder an entry's files are in, to show in the file manager:
    /// its managed folder under its storage root, its external folder, or
    /// the folder holding its external file.
    pub async fn entry_folder(&self, id: &str) -> Result<PathBuf> {
        let entry = self
            .get_entry(id)
            .await?
            .ok_or_else(|| missing("entry", id))?;
        let folder = match entry.location {
            Some(InstallLocation::Managed { root_id, rel_dir }) => {
                let root = self
                    .get_root(&root_id)
                    .await?
                    .ok_or_else(|| missing("storage root", &root_id))?;
                root.path.join(rel_dir)
            }
            Some(InstallLocation::External { path }) if path.is_file() => {
                path.parent().map(Path::to_path_buf).ok_or_else(|| {
                    Error::Invalid(crate::tr!("{path} has no folder", path = path.display()))
                })?
            }
            Some(InstallLocation::External { path }) => path,
            None => {
                return Err(Error::Invalid(crate::tr!(
                    "entry {id} has no install location",
                    id = id
                )));
            }
        };
        if !folder.is_dir() {
            return Err(Error::Invalid(crate::tr!(
                "{folder} is not there",
                folder = folder.display()
            )));
        }
        Ok(folder)
    }
    pub async fn entry_by_source(
        &self,
        source_id: &str,
        external_id: &str,
    ) -> Result<Option<GameEntry>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_entries WHERE source_id=? AND external_id=?",
        )
        .bind(source_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?
        .map(decode)
        .transpose()
    }
    pub async fn entries_for_game(&self, game_id: &str) -> Result<Vec<GameEntry>> {
        sqlx::query_scalar::<_,String>("SELECT record_json FROM game_entries WHERE game_id=? ORDER BY source_id,external_id,id").bind(game_id).fetch_all(&self.pool).await?.into_iter().map(decode).collect()
    }
    /// The runtimes (`native`, `windows`) of the games that aren't emulated.
    pub async fn pc_runtimes(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT json_extract(record_json,'$.runtime.type') FROM game_entries \
             WHERE coalesce(json_extract(record_json,'$.launch.type'),'')<>'emulator' \
             AND json_extract(record_json,'$.runtime.type') IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?)
    }
    pub async fn entries_for_root(&self, root_id: &str) -> Result<Vec<GameEntry>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_entries WHERE root_id=? ORDER BY rel_dir,id",
        )
        .bind(root_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }
    /// [`entries_for_root`](Self::entries_for_root) with each entry's game
    /// title (`None` for an orphaned entry), in one query.
    pub async fn entries_with_titles_for_root(
        &self,
        root_id: &str,
    ) -> Result<Vec<(GameEntry, Option<String>)>> {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT e.record_json,g.title FROM game_entries e LEFT JOIN games g ON g.id=e.game_id \
             WHERE e.root_id=? ORDER BY e.rel_dir,e.id",
        )
        .bind(root_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(record, title)| Ok((decode(record)?, title)))
        .collect()
    }

    /// The filesystem transaction has already moved/reconciled the game.
    /// Update only its root-relative location, retaining source IDs, launch
    /// contract, runtime, alias, tracking and all session references.
    pub async fn update_managed_location(
        &self,
        entry_id: &str,
        root_id: &str,
        relative_dir: &std::path::Path,
    ) -> Result<GameEntry> {
        let mut tx = self.pool.begin().await?;
        let mut entry = load_entry(&mut tx, entry_id).await?;
        if !matches!(entry.location, Some(InstallLocation::Managed { .. })) {
            return Err(Error::Invalid(crate::tr!("entry is not a managed install")));
        }
        entry.location = Some(InstallLocation::Managed {
            root_id: root_id.into(),
            rel_dir: relative_dir.into(),
        });
        let entry = entry.normalized()?;
        save_entry(&mut tx, &entry).await?;
        tx.commit().await?;
        Ok(entry)
    }
    /// Explicit deletion also removes sessions for this source entry. Discovery
    /// refreshes should update its state rather than delete historical entries.
    pub async fn delete_entry(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM game_entries WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    /// Atomically retain `keep` and transfer every source entry, session,
    /// artwork association and manual collection membership from `remove`.
    pub async fn merge_games(&self, keep: &str, remove: &str) -> Result<Game> {
        if keep == remove {
            return self.game(keep).await?.ok_or_else(|| missing("game", keep));
        }
        let mut tx = self.pool.begin().await?;
        let target = merge_in(&mut tx, keep, remove).await?;
        tx.commit().await?;
        Ok(target)
    }
}
pub(super) async fn merge_in(
    tx: &mut Transaction<'_, Sqlite>,
    keep: &str,
    remove: &str,
) -> Result<Game> {
    let (target, target_overrides) = load_game_layers(tx, keep).await?;
    let (donor, donor_overrides) = load_game_layers(tx, remove).await?;
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM play_sessions WHERE game_id IN (?,?) AND ended_at_ms IS NULL",
    )
    .bind(keep)
    .bind(remove)
    .fetch_one(&mut **tx)
    .await?;
    if open > 1 {
        return Err(Error::Invalid(crate::tr!(
            "cannot merge two games with open sessions"
        )));
    }
    let target = merge_layers((target, target_overrides), (donor, donor_overrides), tx).await?;
    sqlx::query("UPDATE game_entries SET game_id=?,record_json=json_set(record_json,'$.gameId',?) WHERE game_id=?").bind(keep).bind(keep).bind(remove).execute(&mut **tx).await?;
    sqlx::query("UPDATE romm_roms SET record_json=json_set(record_json,'$.gameId',?) WHERE record_json->>'$.gameId'=?").bind(keep).bind(remove).execute(&mut **tx).await?;
    // Session game IDs follow the composite entry FK's ON UPDATE CASCADE.
    // The kept game's artwork choices stand; the donor's join where it has
    // none (#175).
    super::artwork_choice::merge_choices(tx, keep, remove).await?;
    sqlx::query("UPDATE artwork SET game_id=?,record_json=json_set(record_json,'$.gameId',?) WHERE game_id=?").bind(keep).bind(keep).bind(remove).execute(&mut **tx).await?;
    // The game's lifecycle scripts (#179) run for the merged game, after its own.
    sqlx::query("UPDATE lifecycle_scripts SET game_id=?1,position=position+(SELECT coalesce(max(position)+1,0) FROM lifecycle_scripts WHERE game_id=?1),record_json=json_set(record_json,'$.scope.gameId',?1) WHERE game_id=?2").bind(keep).bind(remove).execute(&mut **tx).await?;
    sqlx::query("INSERT OR IGNORE INTO collection_members(collection_id,game_id,position) SELECT collection_id,?,position FROM collection_members WHERE game_id=?").bind(keep).bind(remove).execute(&mut **tx).await?;
    // The donor's IGDB match carries over when the kept game has none, and
    // either way the merged game is applied again (its artwork moved too).
    // The kept game's record of what was applied stays: it is the one the
    // next apply replaces.
    sqlx::query("INSERT INTO igdb_matches(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json) SELECT ?,igdb_id,source,1,checked_at_ms,dumps,'{}' FROM igdb_matches WHERE game_id=? AND true ON CONFLICT(game_id) DO UPDATE SET igdb_id=excluded.igdb_id,source=excluded.source,checked_at_ms=excluded.checked_at_ms,dumps=excluded.dumps WHERE igdb_matches.source='none' AND excluded.source<>'none'").bind(keep).bind(remove).execute(&mut **tx).await?;
    sqlx::query("UPDATE igdb_matches SET pending=1 WHERE game_id=?")
        .bind(keep)
        .execute(&mut **tx)
        .await?;
    super::igdb::merge_age_ratings(tx, keep, remove).await?;
    // Reported play time of releases now in one game counts once (#169).
    super::playtime::recount(tx, keep).await?;
    sqlx::query("DELETE FROM games WHERE id=?")
        .bind(remove)
        .execute(&mut **tx)
        .await?;
    Ok(target)
}
pub(super) async fn load_game(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Game> {
    decode(
        sqlx::query_scalar::<_, String>("SELECT record_json FROM games WHERE id=?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| missing("game", id))?,
    )
}
pub(super) async fn load_entry(connection: &mut SqliteConnection, id: &str) -> Result<GameEntry> {
    decode(
        sqlx::query_scalar::<_, String>("SELECT record_json FROM game_entries WHERE id=?")
            .bind(id)
            .fetch_optional(connection)
            .await?
            .ok_or_else(|| missing("entry", id))?,
    )
}
async fn load_game_layers(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<(Game, Overrides)> {
    load_layers(tx, id)
        .await?
        .ok_or_else(|| missing("game", id))
}
/// Saves the merge of `donor` into `target`, each as shown with the fields
/// the user set (#171). What the library shows merges as it always has
/// (lists joined, notes appended, the kept game's values first), except
/// that a description or release date the user set on the donor beats an
/// automatic one. The automatic values merge the same way beneath. A field
/// the user set on either stays theirs, but the donor's title and sort
/// title don't: the kept game's name stands, and the donor's names, its
/// own and the automatic one, join its aliases.
async fn merge_layers(
    (target, target_overrides): (Game, Overrides),
    (donor, donor_overrides): (Game, Overrides),
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<Game> {
    let donor_automatic = automatic(&donor, &donor_overrides)?;
    // A donor the user renamed keeps its automatic name findable too.
    let donor_name = donor_automatic.title.clone();
    let mut merged_automatic = automatic(&target, &target_overrides)?;
    merge_fields(&mut merged_automatic, donor_automatic);
    let mut merged = target.clone();
    merge_fields(&mut merged, donor.clone());
    if donor_name != merged.title && !merged.aliases.contains(&donor_name) {
        merged.aliases.push(donor_name);
    }
    let set = |overrides: &Overrides, field: GameField| overrides.contains_key(field.key());
    if set(&donor_overrides, GameField::Description)
        && !set(&target_overrides, GameField::Description)
    {
        merged.description.clone_from(&donor.description);
    }
    if set(&donor_overrides, GameField::ReleaseDate)
        && !set(&target_overrides, GameField::ReleaseDate)
    {
        merged.release_date.clone_from(&donor.release_date);
    }
    let automatic_values = serde_json::to_value(&merged_automatic)?;
    let mut overrides = Overrides::new();
    for field in GameField::ALL {
        let donors = set(&donor_overrides, field)
            && !matches!(field, GameField::Title | GameField::SortTitle);
        if set(&target_overrides, field) || donors {
            let key = field.key();
            overrides.insert(
                key.to_owned(),
                automatic_values
                    .get(key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            );
        }
    }
    write_game(tx, &merged, Some(&overrides)).await?;
    Ok(merged)
}
/// `donor`'s fields merged into `target`'s: lists joined, the donor's
/// title an alias, favourites and hiding kept, notes never dropped.
fn merge_fields(target: &mut Game, donor: Game) {
    target.aliases = union(&target.aliases, &donor.aliases);
    if donor.title != target.title && !target.aliases.contains(&donor.title) {
        target.aliases.push(donor.title.clone());
    }
    target.tags = union(&target.tags, &donor.tags);
    target.genres = union(&target.genres, &donor.genres);
    target.platforms = union(&target.platforms, &donor.platforms);
    target.favorite |= donor.favorite;
    // Hiding a game is a choice about the game, so a duplicate found later
    // doesn't bring it back.
    target.hidden |= donor.hidden;
    target.rating = target.rating.or(donor.rating);
    target.default_entry = target.default_entry.take().or(donor.default_entry);
    target.release_date = target.release_date.take().or(donor.release_date);
    if target.description.is_empty() {
        target.description = donor.description;
    }
    if target.notes.is_empty() {
        target.notes = donor.notes;
    } else if !donor.notes.is_empty() && donor.notes != target.notes {
        target
            .notes
            .push_str(&format!("\n\n{}:\n{}", donor.title, donor.notes));
    }
}
fn union(a: &[String], b: &[String]) -> Vec<String> {
    a.iter()
        .chain(b)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) async fn save_entry(conn: &mut SqliteConnection, entry: &GameEntry) -> Result<()> {
    let (kind, root, relative, external) = match &entry.location {
        None => ("none", None, None, None),
        Some(InstallLocation::Managed { root_id, rel_dir }) => (
            "managed",
            Some(root_id.as_str()),
            Some(v::path_text(rel_dir)?.replace('\\', "/")),
            None,
        ),
        Some(InstallLocation::External { path }) => {
            ("external", None, None, Some(v::path_text(path)?.to_owned()))
        }
    };
    let state = match entry.state {
        InstallState::Installed => "installed",
        InstallState::Updating => "updating",
        InstallState::Partial => "partial",
        InstallState::Broken { .. } => "broken",
    };
    let launch = entry.launch.as_ref().map(|l| match l {
        LaunchSpec::Uri { .. } => "uri",
        LaunchSpec::Executable { .. } => "executable",
        LaunchSpec::Flatpak { .. } => "flatpak",
        LaunchSpec::Emulator { .. } => "emulator",
        LaunchSpec::Plugin { .. } => "plugin",
        LaunchSpec::Web { .. } => "web",
    });
    sqlx::query("INSERT INTO game_entries(id,game_id,source_id,external_id,state_kind,location_kind,root_id,rel_dir,external_path,launch_kind,size,record_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET game_id=excluded.game_id,source_id=excluded.source_id,external_id=excluded.external_id,state_kind=excluded.state_kind,location_kind=excluded.location_kind,root_id=excluded.root_id,rel_dir=excluded.rel_dir,external_path=excluded.external_path,launch_kind=excluded.launch_kind,size=excluded.size,record_json=excluded.record_json")
        .bind(&entry.id).bind(&entry.game_id).bind(entry.source_id.as_str()).bind(&entry.external_id).bind(state).bind(kind).bind(root).bind(relative).bind(external).bind(launch).bind(entry.size.map(v::integer).transpose()?).bind(v::json(entry)?).execute(conn).await?;
    Ok(())
}
