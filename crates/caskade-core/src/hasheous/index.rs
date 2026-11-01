//! The dumps' SQLite index (`index.sqlite`): games, their names and
//! aliases, and their ROMs' hashes. It is a cache: a schema change drops it
//! and the zips on disk are indexed again.
//!
//! The Windows dump alone has ~600k ROMs, so rows are kept small: dumps are
//! integer ids and hashes are binary.

use super::{DumpGame, DumpRom, LocalDump, MetadataId, RomHashes, RomMatch, hex};
use crate::{Error, Result};
use serde_json::Value;
use sqlx::{AssertSqlSafe, QueryBuilder, Sqlite, SqlitePool};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;

const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = "
DROP TABLE IF EXISTS roms;
DROP TABLE IF EXISTS names;
DROP TABLE IF EXISTS games;
DROP TABLE IF EXISTS dumps;
CREATE TABLE dumps(
    id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, md5 TEXT NOT NULL,
    size INTEGER NOT NULL, modified TEXT, games INTEGER NOT NULL
);
CREATE TABLE games(
    dump INTEGER NOT NULL, id INTEGER NOT NULL, name TEXT NOT NULL,
    metadata TEXT NOT NULL, PRIMARY KEY(dump, id)
) WITHOUT ROWID;
CREATE TABLE names(dump INTEGER NOT NULL, game INTEGER NOT NULL, name TEXT NOT NULL COLLATE NOCASE);
CREATE INDEX names_name ON names(name);
CREATE INDEX names_dump ON names(dump);
CREATE TABLE roms(
    dump INTEGER NOT NULL, game INTEGER NOT NULL, name TEXT NOT NULL, size INTEGER,
    crc BLOB, md5 BLOB, sha1 BLOB, sha256 BLOB
);
CREATE INDEX roms_dump ON roms(dump);
CREATE INDEX roms_crc ON roms(crc) WHERE crc IS NOT NULL;
CREATE INDEX roms_md5 ON roms(md5) WHERE md5 IS NOT NULL;
CREATE INDEX roms_sha1 ON roms(sha1) WHERE sha1 IS NOT NULL;
CREATE INDEX roms_sha256 ON roms(sha256) WHERE sha256 IS NOT NULL;
";
/// Games per insert batch.
const BATCH: usize = 200;
/// Rows per multi-row `INSERT` (8 columns, well under SQLite's 32766
/// parameters).
const ROWS: usize = 500;
/// A game's JSON is tens of KiB; anything this large is not one.
const MAX_ENTRY: u64 = 16 << 20;

/// The hash columns, strongest first, and their sizes in bytes.
const HASHES: [(&str, usize); 4] = [("sha256", 32), ("sha1", 20), ("md5", 16), ("crc", 4)];

pub(super) struct Index {
    pool: SqlitePool,
}

/// A game as read from a dump.
struct Parsed {
    id: i64,
    name: String,
    names: Vec<String>,
    metadata: Vec<MetadataId>,
    roms: Vec<ParsedRom>,
}

struct ParsedRom {
    name: String,
    size: Option<i64>,
    /// `HASHES`' order.
    hashes: [Option<Vec<u8>>; 4],
}

type GameRow = (String, i64, String, String);
type Hash = Option<Vec<u8>>;
type MatchRow = (
    String,
    i64,
    String,
    Option<i64>,
    Hash,
    Hash,
    Hash,
    Hash,
    String,
    String,
);

impl Index {
    pub(super) async fn open(path: PathBuf) -> Result<Self> {
        let pool = crate::cache_db::pool(&path, SCHEMA_VERSION, |_| SCHEMA).await?;
        Ok(Self { pool })
    }

    /// The MD5 of the zip `dump` was indexed from.
    pub(super) async fn md5(&self, dump: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT md5 FROM dumps WHERE name=?")
            .bind(dump)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub(super) async fn dumps(&self) -> Result<Vec<LocalDump>> {
        let rows: Vec<(String, String, i64, Option<String>, i64)> =
            sqlx::query_as("SELECT name, md5, size, modified, games FROM dumps ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(name, md5, size, modified, games)| LocalDump {
                name,
                md5,
                size: u64::try_from(size).unwrap_or_default(),
                modified,
                games: u32::try_from(games).unwrap_or_default(),
            })
            .collect())
    }

    /// Replaces `dump`'s games with the zip's. The old ones stay until the
    /// whole zip is read.
    pub(super) async fn import(
        &self,
        dump: &str,
        md5: &str,
        size: u64,
        modified: Option<String>,
        zip: PathBuf,
    ) -> Result<u32> {
        let (sender, mut receiver) = mpsc::channel::<Vec<Parsed>>(4);
        let parse = caskade_jobs::blocking(
            crate::tr!("Indexing {dump} database", dump = dump),
            move || parse(&zip, &sender),
        );
        let insert = async move {
            let mut tx = self.pool.begin().await?;
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO dumps(name, md5, size, games) VALUES (?, '', 0, 0) \
                 ON CONFLICT(name) DO UPDATE SET name=excluded.name RETURNING id",
            )
            .bind(dump)
            .fetch_one(&mut *tx)
            .await?;
            for table in ["roms", "names", "games"] {
                // `table` is one of the names above.
                sqlx::query(AssertSqlSafe(format!("DELETE FROM {table} WHERE dump=?")))
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            let mut games = 0u32;
            while let Some(batch) = receiver.recv().await {
                games += u32::try_from(batch.len()).unwrap_or(u32::MAX);
                insert(&mut tx, id, &batch).await?;
            }
            Ok::<_, Error>((tx, id, games))
        };
        let (parsed, inserted) = tokio::join!(parse, insert);
        parsed.map_err(|error| Error::Io(std::io::Error::other(error)))??;
        let (mut tx, id, games) = inserted?;
        sqlx::query("UPDATE dumps SET md5=?, size=?, modified=?, games=? WHERE id=?")
            .bind(md5.to_ascii_lowercase())
            .bind(i64::try_from(size).unwrap_or(i64::MAX))
            .bind(modified)
            .bind(games)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        // A large import leaves a WAL as large as the index.
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        Ok(games)
    }

    pub(super) async fn lookup(&self, hashes: &RomHashes) -> Result<Vec<RomMatch>> {
        let given = [&hashes.sha256, &hashes.sha1, &hashes.md5, &hashes.crc];
        let size = hashes.size.and_then(|size| i64::try_from(size).ok());
        for ((column, len), value) in HASHES.into_iter().zip(given) {
            let Some(value) = value.as_deref().and_then(|value| {
                // CRCs are often written without their leading zeros.
                let value = value.trim();
                let padded = if column == "crc" {
                    format!("{value:0>8}")
                } else {
                    value.to_owned()
                };
                unhex(&padded, len)
            }) else {
                continue;
            };
            let by_size = column == "crc" && size.is_some();
            // `column` is one of `HASHES`, never input.
            let mut sql = format!(
                "SELECT d.name, r.game, r.name, r.size, r.sha256, r.sha1, r.md5, r.crc, g.name, g.metadata \
                 FROM roms r JOIN games g ON g.dump=r.dump AND g.id=r.game JOIN dumps d ON d.id=r.dump \
                 WHERE r.{column}=?"
            );
            if by_size {
                sql.push_str(" AND (r.size IS NULL OR r.size=?)");
            }
            sql.push_str(" ORDER BY d.name, r.game LIMIT 100");
            let mut query = sqlx::query_as::<_, MatchRow>(AssertSqlSafe(sql)).bind(value);
            if by_size {
                query = query.bind(size);
            }
            let rows = query.fetch_all(&self.pool).await?;
            if !rows.is_empty() {
                return rows.into_iter().map(rom_match).collect();
            }
        }
        Ok(vec![])
    }

    pub(super) async fn search(
        &self,
        title: &str,
        dumps: &[String],
        limit: u32,
    ) -> Result<Vec<DumpGame>> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(vec![]);
        }
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT d.name, g.id, g.name, g.metadata FROM names n \
             JOIN games g ON g.dump=n.dump AND g.id=n.game JOIN dumps d ON d.id=n.dump WHERE n.name=",
        );
        sql.push_bind(title);
        if !dumps.is_empty() {
            sql.push(" AND d.name IN (");
            let mut list = sql.separated(",");
            for dump in dumps {
                list.push_bind(dump);
            }
            sql.push(")");
        }
        // Exact names before aliases.
        sql.push(" ORDER BY g.name = ")
            .push_bind(title)
            .push(" COLLATE NOCASE DESC, d.name, g.id LIMIT ")
            .push_bind(limit);
        let rows: Vec<GameRow> = sql.build_query_as().fetch_all(&self.pool).await?;
        rows.into_iter().map(game).collect()
    }
}

async fn insert(tx: &mut sqlx::SqliteConnection, dump: i64, batch: &[Parsed]) -> Result<()> {
    let mut games = Vec::with_capacity(batch.len());
    for parsed in batch {
        games.push((parsed, serde_json::to_string(&parsed.metadata)?));
    }
    for chunk in games.chunks(ROWS) {
        let mut rows =
            QueryBuilder::<Sqlite>::new("INSERT OR REPLACE INTO games(dump, id, name, metadata) ");
        rows.push_values(chunk, |mut row, (parsed, metadata)| {
            row.push_bind(dump)
                .push_bind(parsed.id)
                .push_bind(&parsed.name)
                .push_bind(metadata);
        });
        rows.build().execute(&mut *tx).await?;
    }
    let names: Vec<(i64, &String)> = batch
        .iter()
        .flat_map(|parsed| parsed.names.iter().map(|name| (parsed.id, name)))
        .collect();
    for chunk in names.chunks(ROWS) {
        let mut rows = QueryBuilder::<Sqlite>::new("INSERT INTO names(dump, game, name) ");
        rows.push_values(chunk, |mut row, (id, name)| {
            row.push_bind(dump).push_bind(*id).push_bind(*name);
        });
        rows.build().execute(&mut *tx).await?;
    }
    let roms: Vec<(i64, &ParsedRom)> = batch
        .iter()
        .flat_map(|parsed| parsed.roms.iter().map(|rom| (parsed.id, rom)))
        .collect();
    for chunk in roms.chunks(ROWS) {
        let mut rows = QueryBuilder::<Sqlite>::new(
            "INSERT INTO roms(dump, game, name, size, sha256, sha1, md5, crc) ",
        );
        rows.push_values(chunk, |mut row, (id, rom)| {
            row.push_bind(dump)
                .push_bind(*id)
                .push_bind(&rom.name)
                .push_bind(rom.size);
            for hash in &rom.hashes {
                row.push_bind(hash);
            }
        });
        rows.build().execute(&mut *tx).await?;
    }
    Ok(())
}

/// Reads every game in the zip and sends them in batches. An entry that
/// isn't a game is skipped, so one odd file can't stop the rest.
fn parse(zip: &Path, sender: &mpsc::Sender<Vec<Parsed>>) -> Result<()> {
    let zip_error = |error: zip::result::ZipError| Error::Io(std::io::Error::other(error));
    let stopped = |_| Error::Io(std::io::Error::other("indexing stopped"));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(zip_error)?;
    let mut batch = Vec::with_capacity(BATCH);
    let mut bytes = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(zip_error)?;
        if file.is_dir() || !file.name_raw().ends_with(b".json") || file.size() > MAX_ENTRY {
            continue;
        }
        bytes.clear();
        file.take(MAX_ENTRY).read_to_end(&mut bytes)?;
        let Some(parsed) = serde_json::from_slice(&bytes)
            .ok()
            .and_then(|value| game_of(&value))
        else {
            continue;
        };
        batch.push(parsed);
        if batch.len() == BATCH {
            sender
                .blocking_send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH)))
                .map_err(stopped)?;
        }
    }
    if !batch.is_empty() {
        sender.blocking_send(batch).map_err(stopped)?;
    }
    Ok(())
}

fn game_of(value: &Value) -> Option<Parsed> {
    let id = value.get("Id")?.as_i64()?;
    let name = text(value.get("Name"))?;
    let metadata = value
        .get("Metadata")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("Status").and_then(Value::as_str) == Some("Mapped"))
        .filter_map(|entry| {
            Some(MetadataId {
                source: text(entry.get("Source"))?,
                id: text(entry.get("Id"))?,
                link: text(entry.get("Link")),
            })
        })
        .collect();
    let mut names = vec![name.clone()];
    let mut roms = vec![];
    for attribute in value
        .get("Attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let value = attribute.get("Value");
        match attribute.get("attributeName").and_then(Value::as_str) {
            Some("SearchAliases") => {
                for alias in value
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .split(", ")
                {
                    let alias = alias.trim();
                    if !alias.is_empty()
                        && !names.iter().any(|name| name.eq_ignore_ascii_case(alias))
                    {
                        names.push(alias.to_owned());
                    }
                }
            }
            Some("ROMs") => {
                roms.extend(
                    value
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|rom| {
                            let hash = |key: &str, len: usize| {
                                text(rom.get(key)).and_then(|value| unhex(&value, len))
                            };
                            Some(ParsedRom {
                                name: text(rom.get("Name"))?,
                                size: rom
                                    .get("Size")
                                    .and_then(Value::as_u64)
                                    .and_then(|size| i64::try_from(size).ok()),
                                hashes: [
                                    hash("Sha256", 32),
                                    hash("Sha1", 20),
                                    hash("Md5", 16),
                                    hash("Crc", 4),
                                ],
                            })
                        }),
                );
            }
            _ => {}
        }
    }
    Some(Parsed {
        id,
        name,
        names,
        metadata,
        roms,
    })
}

/// A non-empty string or number.
fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.trim())
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// `len` bytes written as hex digits, in either case.
fn unhex(text: &str, len: usize) -> Option<Vec<u8>> {
    let text = text.trim().as_bytes();
    if text.len() != len * 2 || !text.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    text.chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn game((dump, id, name, metadata): GameRow) -> Result<DumpGame> {
    Ok(DumpGame {
        dump,
        id: id.to_string(),
        name,
        metadata: serde_json::from_str(&metadata)?,
    })
}

fn rom_match(row: MatchRow) -> Result<RomMatch> {
    let (dump, id, rom, size, sha256, sha1, md5, crc, name, metadata) = row;
    let hex = |hash: Hash| hash.as_deref().map(hex);
    Ok(RomMatch {
        game: game((dump, id, name, metadata))?,
        rom: DumpRom {
            name: rom,
            size: size.and_then(|size| u64::try_from(size).ok()),
            crc: hex(crc),
            md5: hex(md5),
            sha1: hex(sha1),
            sha256: hex(sha256),
        },
    })
}
