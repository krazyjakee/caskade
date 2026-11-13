use super::{Game, StoreId, manifest};
use crate::Result;
use crate::db::title_key;
use sqlx::{
    ConnectOptions, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE games(title TEXT PRIMARY KEY, data TEXT NOT NULL);
CREATE TABLE names(name TEXT NOT NULL, title TEXT NOT NULL, PRIMARY KEY(name,title));
CREATE TABLE ids(store TEXT NOT NULL, id TEXT NOT NULL, title TEXT NOT NULL, PRIMARY KEY(store,id,title));
";

/// The built index, read-only, opened as patiently as the library.
pub(super) async fn open(path: &Path) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_lazy_with(
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .disable_statement_logging(),
        );
    crate::db::Opening::PATIENT
        .open(&pool, std::future::ready(()), std::future::ready(Ok(())))
        .await?;
    Ok(pool)
}

pub(super) async fn build(dir: &Path, bytes: Vec<u8>) -> Result<u32> {
    tokio::fs::create_dir_all(dir).await?;
    use tokio::io::AsyncWriteExt;
    let mut yaml = tokio::fs::File::create(dir.join("manifest.yaml")).await?;
    yaml.write_all(&bytes).await?;
    yaml.sync_all().await?;
    drop(yaml);
    let mut db = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(dir.join("index.sqlite"))
            .create_if_missing(true)
            .disable_statement_logging(),
    )
    .await?;
    // This database is private until publication. DELETE journaling leaves
    // one self-contained file when the connection closes.
    use sqlx::Connection;
    let mut tx = db.begin().await?;
    sqlx::raw_sql(SCHEMA).execute(&mut *tx).await?;
    // Read a game at a time, each written as it comes (#280's memory: the
    // whole manifest parsed at once took 600 MB).
    let (sender, mut games) = tokio::sync::mpsc::channel::<(String, Game)>(64);
    let reading = tokio::task::spawn_blocking(move || {
        manifest::each_game(&bytes, |title, game| {
            sender
                .blocking_send((title, game))
                .map_err(|_| super::invalid("the save manifest's index stopped"))
        })
    });
    // Every title and its alias, for the names: games themselves aren't kept.
    let mut aliases = std::collections::BTreeMap::new();
    let mut count = 0;
    while let Some((title, game)) = games.recv().await {
        aliases.insert(title.clone(), game.alias.clone());
        if game.alias.is_some() {
            continue;
        }
        sqlx::query("INSERT INTO games(title,data) VALUES(?,?)")
            .bind(&title)
            .bind(serde_json::to_string(&game)?)
            .execute(&mut *tx)
            .await?;
        count += 1;
        for (store, primary, extra) in [
            ("steam", game.steam.id, &game.id.steam_extra),
            ("gog", game.gog.id, &game.id.gog_extra),
        ] {
            for id in primary.iter().chain(extra) {
                sqlx::query("INSERT OR IGNORE INTO ids(store,id,title) VALUES(?,?,?)")
                    .bind(store)
                    .bind(id.to_string())
                    .bind(&title)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    reading
        .await
        .map_err(|error| super::invalid(error.to_string()))??;
    for title in aliases.keys() {
        // Ignore unresolved/cyclic aliases, as Ludusavi's loose schema does.
        if let Some(target) = manifest::target(&aliases, title) {
            sqlx::query("INSERT OR IGNORE INTO names(name,title) VALUES(?,?)")
                .bind(title_key(title))
                .bind(target)
                .execute(&mut *tx)
                .await?;
        }
    }
    if count == 0 {
        return Err(super::invalid("Ludusavi manifest has no games"));
    }
    tx.commit().await?;
    db.close().await?;
    // Flush the completed index before its catalog pointer can be committed.
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("index.sqlite"))
        .await?
        .sync_all()
        .await?;
    Ok(count)
}

pub(super) async fn by_title(pool: &SqlitePool, title: &str) -> Result<Vec<Game>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM games JOIN names USING(title) WHERE name=? ORDER BY title",
    )
    .bind(title_key(title))
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|data| Ok(serde_json::from_str(&data)?))
        .collect()
}

pub(super) async fn by_id(pool: &SqlitePool, id: StoreId) -> Result<Vec<Game>> {
    let (store, id) = match id {
        StoreId::Steam(id) => ("steam", id),
        StoreId::Gog(id) => ("gog", id),
    };
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM games JOIN ids USING(title) WHERE store=? AND id=? ORDER BY title",
    )
    .bind(store)
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|data| Ok(serde_json::from_str(&data)?))
        .collect()
}
