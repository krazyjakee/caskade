//! Store accounts and what they own (#164). See `crate::store_accounts`.

use super::{
    LibraryStore, decode,
    games::{save_entry, save_game},
    missing,
};
use crate::{
    Error, Result,
    model::{Game, GameEntry, InstallState},
    store_accounts::{
        Entitlement, GameRef, LibrarySnapshot, OwnedProduct, Ownership, RefreshOutcome,
        RemovalPreview, StoreAccount, SyncState, source_id,
    },
};
use caskade_discovery::ExternalId;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What applying a snapshot changed in the library.
#[derive(Debug, Default)]
pub(crate) struct Applied {
    pub outcome: RefreshOutcome,
    /// Games created for new products.
    pub added: Vec<String>,
    /// Games that gained or changed an entry.
    pub updated: BTreeSet<String>,
}

/// Account, entry, game, record, first seen, last seen, lost.
type EntitlementRow = (
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
    Option<i64>,
);

type AccountRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    bool,
    String,
    i64,
);

fn account(row: AccountRow) -> StoreAccount {
    let (id, provider, external_id, display_name, owner_id, enabled, sync, created_at_ms) = row;
    StoreAccount {
        id,
        provider,
        external_id,
        display_name,
        owner_id,
        enabled,
        sync: serde_json::from_str::<SyncState>(&sync).unwrap_or_default(),
        created_at_ms,
    }
}

/// The store accounts query, then `$rest`.
macro_rules! accounts {
    ($rest:literal) => {
        concat!(
            "SELECT id,provider,external_id,display_name,owner_id,enabled,sync_json,",
            "created_at_ms FROM store_accounts",
            $rest
        )
    };
}

impl LibraryStore {
    /// Saves a store account (by its id). Another record with the same
    /// provider and external id is refused: one ownership namespace each.
    pub async fn save_store_account(&self, account: &StoreAccount) -> Result<()> {
        let blank = |value: &str| value.trim().is_empty() || value.len() > 256;
        if blank(&account.provider) {
            return Err(Error::Invalid(crate::tr!(
                "a store account needs a provider"
            )));
        }
        if blank(&account.external_id) {
            return Err(Error::Invalid(crate::tr!(
                "a store account needs its id at the provider"
            )));
        }
        sqlx::query(
            "INSERT INTO store_accounts(id,provider,external_id,display_name,owner_id,enabled,\
             sync_json,created_at_ms) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET \
             display_name=excluded.display_name,owner_id=excluded.owner_id,\
             enabled=excluded.enabled,sync_json=excluded.sync_json",
        )
        .bind(&account.id)
        .bind(&account.provider)
        .bind(&account.external_id)
        .bind(&account.display_name)
        .bind(&account.owner_id)
        .bind(account.enabled)
        .bind(serde_json::to_string(&account.sync)?)
        .bind(account.created_at_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn store_accounts(&self) -> Result<Vec<StoreAccount>> {
        Ok(
            sqlx::query_as::<_, AccountRow>(accounts!(" ORDER BY created_at_ms,id"))
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(account)
                .collect(),
        )
    }

    pub async fn store_account(&self, id: &str) -> Result<Option<StoreAccount>> {
        Ok(sqlx::query_as::<_, AccountRow>(accounts!(" WHERE id=?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(account))
    }

    pub async fn store_account_by_external(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<StoreAccount>> {
        Ok(
            sqlx::query_as::<_, AccountRow>(accounts!(" WHERE provider=? AND external_id=?"))
                .bind(provider)
                .bind(external_id)
                .fetch_optional(&self.pool)
                .await?
                .map(account),
        )
    }

    /// Products owned now, lost, and owned with a release installed.
    pub async fn entitlement_counts(&self, account_id: &str) -> Result<(u32, u32, u32)> {
        let (owned, lost, installed): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE a.lost_at_ms IS NULL), \
             count(*) FILTER (WHERE a.lost_at_ms IS NOT NULL), \
             count(*) FILTER (WHERE a.lost_at_ms IS NULL AND EXISTS(SELECT 1 FROM game_entries i \
               WHERE i.game_id=e.game_id AND i.state_kind IN ('installed','updating'))) \
             FROM account_entitlements a LEFT JOIN game_entries e ON e.id=a.entry_id \
             WHERE a.store_account_id=?",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;
        let count = |n: i64| u32::try_from(n).unwrap_or(u32::MAX);
        Ok((count(owned), count(lost), count(installed)))
    }

    /// What `account_id` owns, by product id.
    pub async fn entitlements(&self, account_id: &str) -> Result<Vec<Entitlement>> {
        let rows: Vec<EntitlementRow> = sqlx::query_as(
            "SELECT a.store_account_id,a.entry_id,e.game_id,a.record_json,a.first_seen_ms,\
                 a.last_seen_ms,a.lost_at_ms FROM account_entitlements a \
                 LEFT JOIN game_entries e ON e.id=a.entry_id \
                 WHERE a.store_account_id=? ORDER BY a.product_id",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(
                |(store_account_id, entry_id, game_id, record, first, last, lost)| {
                    Ok(Entitlement {
                        store_account_id,
                        product: decode(record)?,
                        entry_id,
                        game_id,
                        first_seen_ms: first,
                        last_seen_ms: last,
                        lost_at_ms: lost,
                    })
                },
            )
            .collect()
    }

    /// The store accounts that own `game_id`.
    pub async fn ownership(&self, game_id: &str) -> Result<Vec<Ownership>> {
        let rows: Vec<(String, String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT s.id,s.provider,s.display_name,a.product_id,a.lost_at_ms \
             FROM account_entitlements a JOIN game_entries e ON e.id=a.entry_id \
             JOIN store_accounts s ON s.id=a.store_account_id WHERE e.game_id=? \
             ORDER BY s.created_at_ms,s.id,a.product_id",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(store_account_id, provider, display_name, product_id, lost_at_ms)| Ownership {
                    store_account_id,
                    provider,
                    display_name,
                    product_id,
                    lost_at_ms,
                },
            )
            .collect())
    }

    /// Applies one refresh of `account` in a transaction. Every product
    /// listed is recorded, and a new one becomes a library game: an
    /// existing game when one of its entries has the product's store id
    /// (an installed copy discovery found), else a new one. Existing games
    /// keep their own fields. Only a complete snapshot marks products it
    /// doesn't list as lost; nothing is deleted.
    pub(crate) async fn apply_snapshot(
        &self,
        account: &StoreAccount,
        snapshot: &LibrarySnapshot,
        now: i64,
    ) -> Result<Applied> {
        let source = source_id(&account.id);
        let mut products: BTreeMap<String, OwnedProduct> = BTreeMap::new();
        for product in &snapshot.products {
            let id = product.product_id.trim();
            if id.is_empty() || id.len() > 256 {
                continue;
            }
            let mut product = product.clone();
            product.product_id = id.to_owned();
            product.title = product.title.trim().to_owned();
            if product.title.is_empty() {
                product.title = product.product_id.clone();
            }
            products.insert(product.product_id.clone(), product);
        }
        let mut applied = Applied::default();
        // Reads before it writes: IMMEDIATE waits for another writer
        // where a deferred transaction would fail as busy.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let known: HashMap<String, (Option<String>, Option<i64>)> = sqlx::query_as::<
            _,
            (String, Option<String>, Option<i64>),
        >(
            "SELECT product_id,entry_id,lost_at_ms FROM account_entitlements WHERE store_account_id=?",
        )
        .bind(&account.id)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(product, entry, lost)| (product, (entry, lost)))
        .collect();
        let mut by_id = StoreIds::load(&mut tx).await?;
        for product in products.values() {
            let record = serde_json::to_string(product)?;
            match known.get(&product.product_id) {
                Some((entry_id, lost)) => {
                    if lost.is_some() {
                        applied.outcome.regained += 1;
                    }
                    sqlx::query(
                        "UPDATE account_entitlements SET record_json=?,last_seen_ms=?,\
                         lost_at_ms=NULL WHERE store_account_id=? AND product_id=?",
                    )
                    .bind(&record)
                    .bind(now)
                    .bind(&account.id)
                    .bind(&product.product_id)
                    .execute(&mut *tx)
                    .await?;
                    if let Some(entry_id) = entry_id
                        && let Some(game_id) = add_ids(&mut tx, entry_id, product).await?
                    {
                        applied.outcome.updated += 1;
                        applied.updated.insert(game_id);
                    }
                }
                None => {
                    let game_id = match by_id.game(product)? {
                        Some(game_id) => {
                            applied.updated.insert(game_id.clone());
                            game_id
                        }
                        None => {
                            let mut game = Game::new(&product.title);
                            game.platforms = product.platforms.clone();
                            save_game(&mut tx, &game).await?;
                            applied.added.push(game.id.clone());
                            game.id
                        }
                    };
                    let mut entry = GameEntry::new(&game_id, source.clone(), &product.product_id);
                    entry.ids = product.ids.clone();
                    entry.state = InstallState::Partial;
                    let entry = entry.normalized()?;
                    save_entry(&mut tx, &entry).await?;
                    by_id.add(&entry, true)?;
                    sqlx::query(
                        "INSERT INTO account_entitlements(store_account_id,product_id,entry_id,\
                         record_json,first_seen_ms,last_seen_ms) VALUES (?,?,?,?,?,?)",
                    )
                    .bind(&account.id)
                    .bind(&product.product_id)
                    .bind(&entry.id)
                    .bind(&record)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
                    applied.outcome.added += 1;
                }
            }
        }
        if snapshot.complete {
            for (product_id, (_, lost)) in &known {
                if lost.is_none() && !products.contains_key(product_id) {
                    sqlx::query(
                        "UPDATE account_entitlements SET lost_at_ms=? \
                         WHERE store_account_id=? AND product_id=?",
                    )
                    .bind(now)
                    .bind(&account.id)
                    .bind(product_id)
                    .execute(&mut *tx)
                    .await?;
                    applied.outcome.lost += 1;
                }
            }
        } else {
            applied.outcome.partial = true;
        }
        tx.commit().await?;
        Ok(applied)
    }

    /// Games to merge because they share a store id with an account's
    /// entry: an owned game and the installed copy discovery found later.
    /// Each group starts with the game the others merge into, the oldest.
    /// Hidden games and those kept apart stay out.
    pub(crate) async fn store_id_groups(&self) -> Result<Vec<Vec<String>>> {
        let rows: Vec<(String, bool, String)> = sqlx::query_as(
            "SELECT e.game_id,e.source_id LIKE 'store:%',json(j.value) \
             FROM game_entries e JOIN games g ON g.id=e.game_id, json_each(e.record_json,'$.ids') j \
             WHERE g.hidden=0 AND NOT EXISTS(SELECT 1 FROM kept_apart k WHERE k.game_id=e.game_id) \
             ORDER BY g.rowid",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut order: Vec<String> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut by_id: HashMap<String, (Vec<usize>, bool)> = HashMap::new();
        for (game_id, store, id) in rows {
            let at = *index.entry(game_id.clone()).or_insert_with(|| {
                order.push(game_id);
                order.len() - 1
            });
            let Some(id) = key_text(&id) else {
                continue;
            };
            let slot = by_id.entry(id).or_default();
            if !slot.0.contains(&at) {
                slot.0.push(at);
            }
            slot.1 |= store;
        }
        let mut parent: Vec<usize> = (0..order.len()).collect();
        fn root(parent: &mut [usize], mut at: usize) -> usize {
            while parent[at] != at {
                parent[at] = parent[parent[at]];
                at = parent[at];
            }
            at
        }
        for (games, store) in by_id.values() {
            if !store || games.len() < 2 {
                continue;
            }
            for pair in games.windows(2) {
                let (a, b) = (root(&mut parent, pair[0]), root(&mut parent, pair[1]));
                // The older game is the root: the one the others merge into.
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (at, game_id) in order.into_iter().enumerate() {
            let top = root(&mut parent, at);
            groups.entry(top).or_default().push(game_id);
        }
        Ok(groups.into_values().filter(|g| g.len() > 1).collect())
    }

    /// What removing `account_id` would do: games with no entry but its
    /// own go; the others lose only its entries.
    pub async fn removal_preview(&self, account_id: &str) -> Result<RemovalPreview> {
        let rows: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT DISTINCT g.id,g.title,EXISTS(SELECT 1 FROM game_entries o \
               WHERE o.game_id=g.id AND o.source_id<>?) \
             FROM game_entries e JOIN games g ON g.id=e.game_id WHERE e.source_id=? \
             ORDER BY g.sort_title,g.id",
        )
        .bind(source_id(account_id).as_str())
        .bind(source_id(account_id).as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut preview = RemovalPreview::default();
        for (game_id, title, other) in rows {
            let game = GameRef { game_id, title };
            if other {
                preview.kept.push(game);
            } else {
                preview.removed.push(game);
            }
        }
        Ok(preview)
    }

    /// Removes `account_id`, what it owns, its entries and the games only
    /// it brought ([`Self::removal_preview`]). Returns the games removed
    /// and the games that stay but lost an entry.
    pub async fn remove_store_account(
        &self,
        account_id: &str,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let preview = self.removal_preview(account_id).await?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let deleted = sqlx::query("DELETE FROM store_accounts WHERE id=?")
            .bind(account_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(missing("store account", account_id));
        }
        sqlx::query("DELETE FROM game_entries WHERE source_id=?")
            .bind(source_id(account_id).as_str())
            .execute(&mut *tx)
            .await?;
        for game in &preview.removed {
            // Only if nothing joined it since the preview.
            sqlx::query(
                "DELETE FROM games WHERE id=? AND NOT EXISTS(SELECT 1 FROM game_entries e \
                 WHERE e.game_id=games.id)",
            )
            .bind(&game.game_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok((
            preview.removed.into_iter().map(|g| g.game_id).collect(),
            preview.kept.into_iter().map(|g| g.game_id).collect(),
        ))
    }
}

/// What makes two store ids the same: Epic's namespace and catalog item
/// (its app name differs between a library and some local sources, and is
/// missing from others), every other id whole. An Epic id without both
/// names nothing for sure: `None`.
pub(super) fn key(id: &ExternalId) -> Result<Option<String>> {
    Ok(match id {
        ExternalId::Epic {
            namespace,
            catalog_item,
            ..
        } if namespace.is_empty() || catalog_item.is_empty() => None,
        ExternalId::Epic {
            namespace,
            catalog_item,
            ..
        } => Some(format!("epic:{namespace}:{catalog_item}")),
        other => Some(serde_json::to_string(other)?),
    })
}

/// [`key`] of an id as the database's JSON.
fn key_text(json: &str) -> Option<String> {
    key(&serde_json::from_str(json).ok()?).ok().flatten()
}

/// Which game carries each store id, found once per refresh: a game found
/// by discovery (an installed copy) wins over another account's. Keys are
/// the ids' JSON, as serde writes them and SQLite's `json()` reads them
/// back (compact, fields in order).
struct StoreIds(HashMap<String, (bool, String)>);

impl StoreIds {
    async fn load(connection: &mut SqliteConnection) -> Result<Self> {
        let rows: Vec<(String, bool, String)> = sqlx::query_as(
            "SELECT e.game_id,e.source_id LIKE 'store:%',json(j.value) FROM game_entries e \
             JOIN games g ON g.id=e.game_id, json_each(e.record_json,'$.ids') j ORDER BY g.rowid",
        )
        .fetch_all(&mut *connection)
        .await?;
        let mut ids = Self(HashMap::new());
        for (game_id, store, id) in rows {
            if let Some(id) = key_text(&id) {
                ids.insert(id, store, game_id);
            }
        }
        Ok(ids)
    }

    fn insert(&mut self, id: String, store: bool, game_id: String) {
        match self.0.get(&id) {
            // Keep the oldest; a discovered copy beats an account's.
            Some((false, _)) => {}
            Some((true, _)) if store => {}
            _ => {
                self.0.insert(id, (store, game_id));
            }
        }
    }

    fn add(&mut self, entry: &GameEntry, store: bool) -> Result<()> {
        for id in &entry.ids {
            if let Some(key) = key(id)? {
                self.insert(key, store, entry.game_id.clone());
            }
        }
        Ok(())
    }

    /// The game one of `product`'s store ids belongs to.
    fn game(&self, product: &OwnedProduct) -> Result<Option<String>> {
        for id in &product.ids {
            if let Some((_, game_id)) = key(id)?.and_then(|key| self.0.get(&key)) {
                return Ok(Some(game_id.clone()));
            }
        }
        Ok(None)
    }
}

/// Adds store ids `product` lists that the entry lacks; the entry's game
/// when it changed.
async fn add_ids(
    connection: &mut SqliteConnection,
    entry_id: &str,
    product: &OwnedProduct,
) -> Result<Option<String>> {
    let Some(record) =
        sqlx::query_scalar::<_, String>("SELECT record_json FROM game_entries WHERE id=?")
            .bind(entry_id)
            .fetch_optional(&mut *connection)
            .await?
    else {
        return Ok(None);
    };
    let mut entry: GameEntry = decode(record)?;
    let before = entry.ids.len();
    for id in &product.ids {
        if !entry.ids.contains(id) {
            entry.ids.push(id.clone());
        }
    }
    if entry.ids.len() == before {
        return Ok(None);
    }
    save_entry(connection, &entry).await?;
    Ok(Some(entry.game_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Artwork, ArtworkKind, InstallLocation, StorageRoot},
        store_accounts::OwnedProduct,
    };
    use caskade_discovery::ExternalId;
    use std::path::PathBuf;

    async fn counts(store: &LibraryStore) -> BTreeMap<String, i64> {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' \
             AND name NOT LIKE 'game_search%' AND name NOT LIKE '_sqlx%'",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let mut out = BTreeMap::new();
        for table in tables {
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM \"{table}\""
            )))
            .fetch_one(&store.pool)
            .await
            .unwrap();
            out.insert(table, count);
        }
        out
    }

    #[tokio::test]
    async fn the_migration_comes_and_goes_without_touching_the_library() {
        let store = LibraryStore::in_memory().await.unwrap();
        let root = StorageRoot {
            id: "01a1aaaa-0000-7000-8000-000000000001".into(),
            path: PathBuf::from("/games"),
            is_default: true,
            label: None,
        };
        store.upsert_root(&root).await.unwrap();
        let game = Game::new("Kept");
        store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "620");
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::Managed {
            root_id: root.id.clone(),
            rel_dir: PathBuf::from("Kept"),
        });
        entry.alias_id = Some("alias".into());
        store.upsert_entry(&entry).await.unwrap();
        store
            .upsert_artwork(&Artwork::new(
                &game.id,
                ArtworkKind::Cover,
                "a".repeat(64),
                "user",
                "a/original.webp",
                1,
                1,
            ))
            .await
            .unwrap();
        let account = StoreAccount {
            id: crate::model::new_id(),
            provider: "steam".into(),
            external_id: "1".into(),
            display_name: "Me".into(),
            owner_id: None,
            enabled: true,
            sync: SyncState::default(),
            created_at_ms: 0,
        };
        store.save_store_account(&account).await.unwrap();
        store
            .apply_snapshot(
                &account,
                &LibrarySnapshot {
                    products: vec![OwnedProduct {
                        product_id: "400".into(),
                        title: "Owned".into(),
                        ids: vec![ExternalId::Steam { app_id: 400 }],
                        platforms: vec![],
                        playtime_minutes: None,
                        last_played_ms: None,
                        systems: Vec::new(),
                    }],
                    complete: true,
                },
                1,
            )
            .await
            .unwrap();
        let before = counts(&store).await;
        // 0022 down, then the migrator brings it back.
        sqlx::raw_sql(include_str!(
            "../../migrations/0022_store_accounts.down.sql"
        ))
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version=22")
            .execute(&store.pool)
            .await
            .unwrap();
        super::super::MIGRATOR.run(&store.pool).await.unwrap();
        let after = counts(&store).await;
        for (table, rows) in &before {
            let expected = if table.contains("store_accounts") || table.contains("entitlements") {
                0
            } else {
                *rows
            };
            assert_eq!(after.get(table), Some(&expected), "{table}");
        }
        assert_eq!(before["store_accounts"], 1);
        assert_eq!(
            store.get_entry(&entry.id).await.unwrap().unwrap(),
            entry.normalized().unwrap()
        );
    }
}
