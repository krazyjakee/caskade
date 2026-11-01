//! Artwork the user chose (#175). A row in `artwork_choices` gives the user
//! a kind of a game's artwork: the game then shows its `user`-source image
//! of that kind, or none. Choosing an automatic image copies its record as
//! the user's (the cache bytes are shared), so a refresh that replaces the
//! automatic one leaves the choice alone; Reset drops the user's records
//! and automatic precedence applies again. Deleting a record never deletes
//! cache bytes another record may use.

use super::{LibraryStore, decode, missing};
use crate::{
    Error, Result,
    artwork::{ArtworkCache, CachedArtwork, MAX_INPUT_BYTES},
    model::{validation as v, *},
};
use sqlx::SqliteConnection;
use std::path::Path;

/// SQL: whether artwork row `a` is one the game shows, by the choices.
macro_rules! shown_sql {
    () => {
        "NOT EXISTS(SELECT 1 FROM artwork_choices c WHERE c.game_id=a.game_id AND c.kind=a.kind AND a.record_json->>'$.source'<>'user')"
    };
}
pub(super) const SHOWN: &str = shown_sql!();

fn kind_text(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Cover => "cover",
        ArtworkKind::Hero => "hero",
        ArtworkKind::Logo => "logo",
        ArtworkKind::Icon => "icon",
        ArtworkKind::Screenshot => "screenshot",
    }
}

fn editable(kind: ArtworkKind) -> Result<()> {
    if EDITABLE_ARTWORK.contains(&kind) {
        Ok(())
    } else {
        Err(Error::Invalid(crate::tr!(
            "this kind of artwork can't be chosen"
        )))
    }
}

/// Automatic precedence: an IGDB icon (`igdb:` source, as the library's
/// query has it) before others, then the oldest.
fn automatic_order(kind: ArtworkKind, images: &mut [Artwork]) {
    images.sort_by(|a, b| {
        let igdb = |art: &Artwork| kind == ArtworkKind::Icon && art.source.starts_with("igdb:");
        igdb(b).cmp(&igdb(a)).then_with(|| a.id.cmp(&b.id))
    });
}

async fn exists(conn: &mut SqliteConnection, game_id: &str) -> Result<()> {
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id=?)")
        .bind(game_id)
        .fetch_one(conn)
        .await?;
    if found {
        Ok(())
    } else {
        Err(missing("game", game_id))
    }
}

/// Gives the user `kind` of the game's artwork, with `image` (already a
/// `user` record, or none) its only image of that kind.
async fn take_over(
    conn: &mut SqliteConnection,
    game_id: &str,
    kind: ArtworkKind,
    image: Option<&Artwork>,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO artwork_choices(game_id,kind) VALUES(?,?)")
        .bind(game_id)
        .bind(kind_text(kind))
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "DELETE FROM artwork WHERE game_id=? AND kind=? AND record_json->>'$.source'='user'",
    )
    .bind(game_id)
    .bind(kind_text(kind))
    .execute(&mut *conn)
    .await?;
    if let Some(image) = image {
        image.validate()?;
        sqlx::query(
            "INSERT INTO artwork(id,game_id,kind,content_hash,record_json) VALUES(?,?,?,?,?)",
        )
        .bind(&image.id)
        .bind(game_id)
        .bind(kind_text(kind))
        .bind(&image.content_hash)
        .bind(v::json(image)?)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

impl LibraryStore {
    /// The game's artwork as it shows: under a choice only the user's
    /// image of that kind (or none), otherwise every automatic one.
    pub async fn shown_artwork(&self, game_id: &str) -> Result<Vec<Artwork>> {
        sqlx::query_scalar::<_, String>(concat!(
            "SELECT a.record_json FROM artwork a WHERE a.game_id=? AND ",
            shown_sql!(),
            " ORDER BY a.kind,a.record_json->>'$.source'='user' DESC,a.id"
        ))
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    /// Each kind of artwork the editor changes, as the game has it; `None`
    /// when the game is gone.
    pub async fn artwork_slots(&self, game_id: &str) -> Result<Option<Vec<ArtworkSlot>>> {
        let mut conn = self.pool.acquire().await?;
        if exists(&mut conn, game_id).await.is_err() {
            return Ok(None);
        }
        let chosen: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM artwork_choices WHERE game_id=?")
                .bind(game_id)
                .fetch_all(&mut *conn)
                .await?;
        let all: Vec<Artwork> = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM artwork WHERE game_id=? ORDER BY kind,id",
        )
        .bind(game_id)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(decode)
        .collect::<Result<_>>()?;
        Ok(Some(
            EDITABLE_ARTWORK
                .into_iter()
                .map(|kind| {
                    let (user, mut automatic): (Vec<_>, Vec<_>) = all
                        .iter()
                        .filter(|art| art.kind == kind)
                        .cloned()
                        .partition(|art| art.source == USER_ARTWORK_SOURCE);
                    automatic_order(kind, &mut automatic);
                    let controlled = chosen.iter().any(|text| text == kind_text(kind));
                    let (choice, shown) = match (controlled, user.into_iter().next()) {
                        (false, _) => (ArtworkChoice::Automatic, automatic.first().cloned()),
                        (true, Some(image)) => (ArtworkChoice::Chosen, Some(image)),
                        (true, None) => (ArtworkChoice::Removed, None),
                    };
                    ArtworkSlot {
                        kind,
                        choice,
                        shown,
                        automatic,
                    }
                })
                .collect(),
        ))
    }

    /// Shows `artwork_id` (one of the game's images of `kind`) as the
    /// game's `kind`, whatever refreshes bring later.
    pub async fn choose_artwork(
        &self,
        game_id: &str,
        kind: ArtworkKind,
        artwork_id: &str,
    ) -> Result<()> {
        editable(kind)?;
        let mut tx = self.pool.begin().await?;
        exists(&mut tx, game_id).await?;
        let image: Artwork = decode(
            sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM artwork WHERE id=? AND game_id=? AND kind=?",
            )
            .bind(artwork_id)
            .bind(game_id)
            .bind(kind_text(kind))
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| missing("artwork", artwork_id))?,
        )?;
        if image.source != USER_ARTWORK_SOURCE {
            let mut copy = image;
            copy.id = new_id();
            USER_ARTWORK_SOURCE.clone_into(&mut copy.source);
            take_over(&mut tx, game_id, kind, Some(&copy)).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Shows `image`, cached already, as the game's `kind`.
    pub async fn set_user_artwork(
        &self,
        game_id: &str,
        kind: ArtworkKind,
        image: &CachedArtwork,
    ) -> Result<Artwork> {
        editable(kind)?;
        let record = image.record(game_id, kind, USER_ARTWORK_SOURCE);
        let mut tx = self.pool.begin().await?;
        exists(&mut tx, game_id).await?;
        take_over(&mut tx, game_id, kind, Some(&record)).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Imports the image at `path` into the artwork cache and shows it as
    /// the game's `kind`. A file that is too large or not a supported image
    /// fails before anything changes, so the game keeps what it showed.
    pub async fn import_user_artwork(
        &self,
        cache: &ArtworkCache,
        game_id: &str,
        kind: ArtworkKind,
        path: &Path,
    ) -> Result<Artwork> {
        editable(kind)?;
        let size = tokio::fs::metadata(path).await?.len();
        if size > MAX_INPUT_BYTES as u64 {
            return Err(Error::Invalid(crate::tr!(
                "the image is larger than {mib} MiB",
                mib = MAX_INPUT_BYTES / (1024 * 1024)
            )));
        }
        let bytes = tokio::fs::read(path).await?;
        let cached = cache
            .import_bytes(bytes)
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        self.set_user_artwork(game_id, kind, &cached).await
    }

    /// Shows no `kind` artwork for the game, whatever refreshes bring.
    pub async fn remove_artwork(&self, game_id: &str, kind: ArtworkKind) -> Result<()> {
        editable(kind)?;
        let mut tx = self.pool.begin().await?;
        exists(&mut tx, game_id).await?;
        take_over(&mut tx, game_id, kind, None).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Back to automatic precedence for the game's `kind`: the user's
    /// records go (their cache bytes stay for others).
    pub async fn reset_artwork(&self, game_id: &str, kind: ArtworkKind) -> Result<()> {
        editable(kind)?;
        let mut tx = self.pool.begin().await?;
        exists(&mut tx, game_id).await?;
        sqlx::query("DELETE FROM artwork_choices WHERE game_id=? AND kind=?")
            .bind(game_id)
            .bind(kind_text(kind))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM artwork WHERE game_id=? AND kind=? AND record_json->>'$.source'='user'",
        )
        .bind(game_id)
        .bind(kind_text(kind))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// `remove`'s artwork choices into `keep`'s, before its artwork moves: a
/// kind both chose keeps `keep`'s image (`remove`'s user images of it go),
/// and a kind only `remove` chose stays chosen.
pub(super) async fn merge_choices(
    conn: &mut SqliteConnection,
    keep: &str,
    remove: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM artwork WHERE game_id=? AND record_json->>'$.source'='user' \
         AND kind IN (SELECT kind FROM artwork_choices WHERE game_id=?)",
    )
    .bind(remove)
    .bind(keep)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO artwork_choices(game_id,kind) SELECT ?,kind FROM artwork_choices WHERE game_id=?",
    )
    .bind(keep)
    .bind(remove)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Copies `from`'s artwork choices to `to` (a separated entry's game).
pub(super) async fn copy_choices(conn: &mut SqliteConnection, from: &str, to: &str) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO artwork_choices(game_id,kind) SELECT ?,kind FROM artwork_choices WHERE game_id=?",
    )
    .bind(to)
    .bind(from)
    .execute(conn)
    .await?;
    Ok(())
}
