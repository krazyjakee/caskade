//! Manual game edits (#171). A game row holds what the library shows
//! (`record_json` and its columns), the user's values included, and in
//! `overrides_json` each field the user set with the value automatic
//! sources would give it. Automatic writers work on that layer beneath:
//! through [`LibraryStore::update_automatic_game`], or [`save_game`]'s
//! protection for those that write a whole game. Either way the user's
//! values stay, intentional clears included, until Reset.
//!
//! [`save_game`]: super::games::save_game

use super::{LibraryStore, decode, games::write_game, missing};
use crate::{Error, Result, model::*};
use serde_json::{Map, Value};
use sqlx::SqliteConnection;

/// The fields the user set, each with its automatic value.
pub(super) type Overrides = Map<String, Value>;

/// A game as stored, and the automatic values of the fields the user set.
pub(super) async fn load_layers(
    conn: &mut SqliteConnection,
    id: &str,
) -> Result<Option<(Game, Overrides)>> {
    let Some((record, overrides)) = sqlx::query_as::<_, (String, String)>(
        "SELECT record_json,overrides_json FROM games WHERE id=?",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some((decode(record)?, serde_json::from_str(&overrides)?)))
}

fn object(game: &Game) -> Result<Map<String, Value>> {
    match serde_json::to_value(game)? {
        Value::Object(map) => Ok(map),
        _ => Err(Error::Invalid(crate::tr!("game record is not an object"))),
    }
}

/// The game as automatic sources have it: `shown` with each field the user
/// set back at its automatic value.
pub(super) fn automatic(shown: &Game, overrides: &Overrides) -> Result<Game> {
    if overrides.is_empty() {
        return Ok(shown.clone());
    }
    let mut map = object(shown)?;
    for (key, value) in overrides {
        map.insert(key.clone(), value.clone());
    }
    Ok(serde_json::from_value(Value::Object(map))?)
}

/// `automatic` (a new automatic layer) as the library shows it: the user's
/// fields keep `shown`'s values and take `automatic`'s as their automatic
/// values.
pub(super) fn overlay(shown: &Game, overrides: &mut Overrides, automatic: &Game) -> Result<Game> {
    if overrides.is_empty() {
        return Ok(automatic.clone());
    }
    let shown = object(shown)?;
    let mut next = object(automatic)?;
    for (key, value) in overrides.iter_mut() {
        let user = shown.get(key).cloned().unwrap_or(Value::Null);
        *value = next.insert(key.clone(), user).unwrap_or(Value::Null);
    }
    Ok(serde_json::from_value(Value::Object(next))?)
}

/// `incoming`, written by something that doesn't know the user's fields,
/// as the library shows it: where it differs from `shown` in a field the
/// user set, the difference is the new automatic value and the user's
/// stays. Where it is `shown`'s value, the writer left the field alone.
pub(super) fn protect(shown: &Game, overrides: &mut Overrides, incoming: &Game) -> Result<Game> {
    if overrides.is_empty() {
        return Ok(incoming.clone());
    }
    let shown = object(shown)?;
    let mut next = object(incoming)?;
    for (key, value) in overrides.iter_mut() {
        let user = shown.get(key).cloned().unwrap_or(Value::Null);
        let written = next
            .insert(key.clone(), user.clone())
            .unwrap_or(Value::Null);
        if written != user {
            *value = written;
        }
    }
    Ok(serde_json::from_value(Value::Object(next))?)
}

fn state(shown: Game, overrides: &Overrides) -> Result<GameEditState> {
    let automatic = automatic(&shown, overrides)?;
    let manual = GameField::ALL
        .into_iter()
        .filter(|field| overrides.contains_key(field.key()))
        .collect();
    Ok(GameEditState {
        game: shown,
        automatic,
        manual,
    })
}

/// How a patch changes one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// To the user's value, which the field had to have when the editor
    /// loaded it.
    Set,
    /// Values added or removed, whatever else the list holds.
    Combine,
    /// Back to the automatic value.
    Reset,
}

/// Trimmed values without empties or repeats, in their order.
fn clean(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !out.iter().any(|kept| kept == value) {
            out.push(value.to_owned());
        }
    }
    out
}

fn text(edit: &TextEdit, slot: &mut String, automatic: &str, trim: bool) -> Op {
    match edit {
        TextEdit::Set { value } => {
            *slot = if trim { value.trim() } else { value.as_str() }.to_owned();
            Op::Set
        }
        TextEdit::Reset => {
            automatic.clone_into(slot);
            Op::Reset
        }
    }
}

fn list(edit: &ListEdit, slot: &mut Vec<String>, automatic: &[String]) -> Op {
    match edit {
        ListEdit::Set { values } => {
            *slot = clean(values);
            Op::Set
        }
        ListEdit::Add { values } => {
            for value in clean(values) {
                if !slot.contains(&value) {
                    slot.push(value);
                }
            }
            Op::Combine
        }
        ListEdit::Remove { values } => {
            let values = clean(values);
            slot.retain(|value| !values.contains(value));
            Op::Combine
        }
        ListEdit::Reset => {
            *slot = automatic.to_vec();
            Op::Reset
        }
    }
}

/// `edit` applied to `next` (a copy of `shown`), with each changed field
/// and how; a rating out of range is reported in `errors`.
fn apply_edit(
    edit: &GameEdit,
    next: &mut Game,
    automatic: &Game,
    errors: &mut Vec<FieldError>,
) -> Vec<(GameField, Op)> {
    let mut ops = Vec::new();
    if let Some(edit) = &edit.title {
        ops.push((
            GameField::Title,
            text(edit, &mut next.title, &automatic.title, true),
        ));
    }
    if let Some(edit) = &edit.sort_title {
        ops.push((
            GameField::SortTitle,
            text(edit, &mut next.sort_title, &automatic.sort_title, true),
        ));
    }
    if let Some(edit) = &edit.description {
        ops.push((
            GameField::Description,
            text(edit, &mut next.description, &automatic.description, false),
        ));
    }
    for (field, edit, slot, automatic) in [
        (
            GameField::Aliases,
            &edit.aliases,
            &mut next.aliases,
            &automatic.aliases,
        ),
        (
            GameField::Genres,
            &edit.genres,
            &mut next.genres,
            &automatic.genres,
        ),
        (GameField::Tags, &edit.tags, &mut next.tags, &automatic.tags),
        (
            GameField::Platforms,
            &edit.platforms,
            &mut next.platforms,
            &automatic.platforms,
        ),
    ] {
        if let Some(edit) = edit {
            ops.push((field, list(edit, slot, automatic)));
        }
    }
    if let Some(edit) = &edit.release_date {
        ops.push((
            GameField::ReleaseDate,
            match edit {
                DateEdit::Set { value } => {
                    next.release_date = value
                        .as_deref()
                        .map(str::trim)
                        .filter(|date| !date.is_empty())
                        .map(str::to_owned);
                    Op::Set
                }
                DateEdit::Reset => {
                    next.release_date.clone_from(&automatic.release_date);
                    Op::Reset
                }
            },
        ));
    }
    if let Some(edit) = &edit.rating {
        match edit.value.map(u8::try_from) {
            None => next.rating = None,
            Some(Ok(rating)) if rating <= 100 => next.rating = Some(rating),
            Some(_) => errors.push(FieldError {
                field: GameField::Rating,
                problem: FieldProblem::Invalid,
                message: crate::tr!("rating must be 0 through 100"),
            }),
        }
        ops.push((GameField::Rating, Op::Set));
    }
    if let Some(edit) = &edit.notes {
        next.notes.clone_from(&edit.value);
        ops.push((GameField::Notes, Op::Set));
    }
    ops
}

/// The most games one bulk edit takes, and how many go between yields.
pub const BULK_LIMIT: usize = 10_000;
const BULK_BATCH: usize = 50;

/// A field's value as text, for a bulk edit's preview.
fn show(game: &Game, field: GameField) -> String {
    match field {
        GameField::Title => game.title.clone(),
        GameField::SortTitle => game.sort_title.clone(),
        GameField::Description => game.description.clone(),
        GameField::Notes => game.notes.clone(),
        GameField::Aliases => game.aliases.join(", "),
        GameField::Genres => game.genres.join(", "),
        GameField::Tags => game.tags.join(", "),
        GameField::Platforms => game.platforms.join(", "),
        GameField::ReleaseDate => game.release_date.clone().unwrap_or_default(),
        GameField::Rating => game.rating.map(|r| r.to_string()).unwrap_or_default(),
    }
}

fn message(error: Error) -> String {
    match error {
        Error::Invalid(message) => message,
        other => other.to_string(),
    }
}

impl LibraryStore {
    /// The game as the editor shows it; `None` when it is gone.
    pub async fn game_edit_state(&self, id: &str) -> Result<Option<GameEditState>> {
        let mut conn = self.pool.acquire().await?;
        load_layers(&mut conn, id)
            .await?
            .map(|(shown, overrides)| state(shown, &overrides))
            .transpose()
    }

    /// Saves the user's changes to a game's fields, all or none. A field
    /// set by hand stays the user's until reset; `base`, the game as the
    /// editor loaded it, makes a set field that changed since (and not to
    /// the same value) a conflict rather than overwriting it. Fields left
    /// out of `edit` are never written, so a refresh meanwhile keeps
    /// whatever it wrote to them.
    pub async fn edit_game(
        &self,
        id: &str,
        edit: &GameEdit,
        base: Option<&Game>,
    ) -> Result<GameEditOutcome> {
        Ok(self
            .edit_one(id, edit, base, true)
            .await?
            .ok_or_else(|| missing("game", id))?
            .0)
    }

    /// `edit` on one game, saved when `apply`; with the game as it was.
    /// `None` when the game is gone.
    async fn edit_one(
        &self,
        id: &str,
        edit: &GameEdit,
        base: Option<&Game>,
        apply: bool,
    ) -> Result<Option<(GameEditOutcome, Game)>> {
        let mut tx = self.begin_write().await?;
        let Some((shown, mut overrides)) = load_layers(&mut tx, id).await? else {
            return Ok(None);
        };
        let automatic = automatic(&shown, &overrides)?;
        let mut next = shown.clone();
        let mut errors = Vec::new();
        let ops = apply_edit(edit, &mut next, &automatic, &mut errors);
        let before = object(&shown)?;
        let after = object(&next)?;
        let base = base.map(object).transpose()?;
        let automatic_values = object(&automatic)?;
        let mut changed = Vec::new();
        for (field, op) in ops {
            let key = field.key();
            if errors.iter().any(|error| error.field == field) {
                continue;
            }
            if let Err(error) = next.check(field) {
                errors.push(FieldError {
                    field,
                    problem: FieldProblem::Invalid,
                    message: message(error),
                });
                continue;
            }
            if op == Op::Set
                && let Some(base) = &base
                && base.get(key) != before.get(key)
                && before.get(key) != after.get(key)
            {
                errors.push(FieldError {
                    field,
                    problem: FieldProblem::Conflict,
                    message: crate::tr!("changed since the editor opened"),
                });
                continue;
            }
            // Adding values a list has, or removing ones it lacks, leaves it
            // to whoever controls it.
            if op == Op::Combine && before.get(key) == after.get(key) {
                continue;
            }
            let was_manual = overrides.contains_key(key);
            let manual = if op == Op::Reset {
                overrides.remove(key);
                false
            } else if field.automatic() {
                if !was_manual {
                    let value = automatic_values.get(key).cloned().unwrap_or(Value::Null);
                    overrides.insert(key.to_owned(), value);
                }
                true
            } else {
                false
            };
            if was_manual != manual || before.get(key) != after.get(key) {
                changed.push(field);
            }
        }
        if !errors.is_empty() {
            let (current, overrides) = load_layers(&mut tx, id)
                .await?
                .ok_or_else(|| missing("game", id))?;
            return Ok(Some((
                GameEditOutcome::Rejected {
                    state: state(current, &overrides)?,
                    errors,
                },
                shown,
            )));
        }
        if !changed.is_empty() {
            next.validate()?;
            if apply {
                write_game(&mut tx, &next, Some(&overrides)).await?;
                tx.commit().await?;
            }
        }
        Ok(Some((
            GameEditOutcome::Saved {
                state: state(next, &overrides)?,
                changed,
            },
            shown,
        )))
    }

    /// One edit on many games (#173): previewed (`apply` false, nothing
    /// saved) or applied, each game in its own transaction so a failure
    /// leaves the others' results standing. A field left out of `edit`
    /// stays as each game has it, so mixed values stay mixed; adds and
    /// removes keep the rest of each list. Applying again is harmless:
    /// games that already have the result come back unchanged.
    pub async fn bulk_edit_games(
        &self,
        ids: &[String],
        edit: &GameEdit,
        apply: bool,
    ) -> Result<Vec<BulkGameResult>> {
        if ids.len() > BULK_LIMIT {
            return Err(Error::Invalid(crate::tr!(
                "a bulk edit takes at most {limit} games",
                limit = BULK_LIMIT
            )));
        }
        let mut results = Vec::with_capacity(ids.len());
        for (index, id) in ids.iter().enumerate() {
            // Bounded batches: other work gets the pool between them.
            if index > 0 && index % BULK_BATCH == 0 {
                tokio::task::yield_now().await;
            }
            let result = match self.edit_one(id, edit, None, apply).await? {
                None => BulkGameResult {
                    game_id: id.clone(),
                    title: String::new(),
                    status: BulkStatus::Missing,
                    changes: Vec::new(),
                    errors: Vec::new(),
                },
                Some((GameEditOutcome::Rejected { errors, .. }, before)) => BulkGameResult {
                    game_id: id.clone(),
                    title: before.title,
                    status: BulkStatus::Rejected,
                    changes: Vec::new(),
                    errors,
                },
                Some((GameEditOutcome::Saved { state, changed }, before)) => {
                    let changes: Vec<BulkChange> = changed
                        .iter()
                        .filter_map(|field| {
                            let (before, after) =
                                (show(&before, *field), show(&state.game, *field));
                            (before != after).then_some(BulkChange {
                                field: *field,
                                before,
                                after,
                            })
                        })
                        .collect();
                    BulkGameResult {
                        game_id: id.clone(),
                        title: before.title,
                        status: if changed.is_empty() {
                            BulkStatus::Unchanged
                        } else {
                            BulkStatus::Changed
                        },
                        changes,
                        errors: Vec::new(),
                    }
                }
            };
            results.push(result);
        }
        Ok(results)
    }

    /// Every game `query` finds, in its order (a bulk edit's "select all").
    pub async fn game_ids(&self, query: &ListGames) -> Result<Vec<String>> {
        self.game_ids_for_account(query, None).await
    }

    pub async fn game_ids_for_account(
        &self,
        query: &ListGames,
        account: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut request = query.clone();
        request.limit = 1000;
        request.offset = 0;
        let mut ids = Vec::new();
        loop {
            let page = self.list_games_for_account(&request, account).await?;
            let count = page.items.len();
            ids.extend(page.items.into_iter().map(|game| game.id));
            if count == 0 || ids.len() as u64 >= page.total || ids.len() >= BULK_LIMIT {
                break;
            }
            request.offset += count as u32;
        }
        ids.truncate(BULK_LIMIT);
        Ok(ids)
    }

    /// Changes the game as automatic sources have it: `update` sees the
    /// automatic values of the fields the user set and may change them,
    /// which changes what Reset restores but not what the library shows.
    /// `None` when the game is gone; otherwise what `update` returned and
    /// whether what the library shows changed.
    pub async fn update_automatic_game<R>(
        &self,
        id: &str,
        update: impl FnOnce(&mut Game) -> R,
    ) -> Result<Option<(R, bool)>> {
        let mut tx = self.begin_write().await?;
        let Some((shown, mut overrides)) = load_layers(&mut tx, id).await? else {
            return Ok(None);
        };
        let before = automatic(&shown, &overrides)?;
        let mut next = before.clone();
        let out = update(&mut next);
        if next == before {
            return Ok(Some((out, false)));
        }
        next.id.clone_from(&shown.id);
        next.validate()?;
        let next = overlay(&shown, &mut overrides, &next)?;
        write_game(&mut tx, &next, Some(&overrides)).await?;
        tx.commit().await?;
        Ok(Some((out, next != shown)))
    }
}
