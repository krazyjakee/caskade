//! Parameterized library queries; user search strings are literal words, never
//! SQL or FTS operators. Trigram queries cover >=3-character tokens; shorter
//! tokens use a GLOB fallback (FTS5 cannot index them). Both fold case and
//! accents ("pokemon" finds "Pokémon").
use super::{LibraryStore, collation::folded_glob, decode, missing};
use crate::{Result, model::*};
use sqlx::{QueryBuilder, Sqlite};

/// An entry's state alone, as on the game page: discovered launchers
/// (`.desktop` files, many Lutris games) are installed without a folder.
const INSTALLED: &str = "EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.state_kind IN ('installed','updating','broken'))";
/// Tracked sessions plus what reported totals add (#169, `db::playtime`).
const PLAYTIME: &str = "(coalesce((SELECT sum(s.duration_ms) FROM play_sessions s WHERE s.game_id=g.id),0)+coalesce((SELECT sum(r.counted_ms) FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id WHERE re.game_id=g.id),0))";
/// Owned by a store account that still owns it (#170).
const OWNED: &str = "EXISTS(SELECT 1 FROM game_entries oe JOIN account_entitlements ae ON ae.entry_id=oe.id WHERE oe.game_id=g.id AND ae.lost_at_ms IS NULL)";
/// The latest session start or reported last play.
const RECENT: &str = "(SELECT max(t) FROM (SELECT max(s.started_at_ms) AS t FROM play_sessions s WHERE s.game_id=g.id UNION ALL SELECT max(r.last_played_ms) FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id WHERE re.game_id=g.id))";
/// Ids are UUIDv7s, so the greatest is the newest: the game's own, or its
/// newest entry's. A game with no entries left (its install uninstalled)
/// has nothing added, so it sorts last.
const ADDED: &str = "(SELECT max(g.id,max(e.id)) FROM game_entries e WHERE e.game_id=g.id)";
const RELEASED: &str = "g.record_json->>'$.releaseDate'";
const EMULATED: &str =
    "EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind='emulator')";
const WEB: &str =
    "EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind='web')";
/// Not a web game alone: web games show in the Web tab only, and a game
/// that also has another release shows in the others too.
const NOT_WEB_ONLY: &str = "(NOT EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind='web') OR EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind IS NOT 'web'))";

/// By title in the language's alphabet (`db::collation`), then id.
const BY_TITLE: &str = " ORDER BY g.sort_title COLLATE caskade_title,g.id";
const BY_KEY_THEN_TITLE: &str = " ORDER BY k DESC,g.sort_title COLLATE caskade_title,g.id";

impl LibraryStore {
    pub async fn get_game(&self, id: &str) -> Result<Option<GameDetails>> {
        let Some(game) = self.game(id).await? else {
            return Ok(None);
        };
        let playtime = self.playtime_breakdown(id).await?;
        Ok(Some(GameDetails {
            game,
            entries: self.entries_for_game(id).await?,
            artwork: self.shown_artwork(id).await?,
            playtime_ms: playtime.total_ms,
            last_played_at_ms: playtime.last_played_ms,
        }))
    }
    pub async fn list_games(&self, request: &ListGames) -> Result<GamePage> {
        self.list_games_for_account(request, None).await
    }
    /// Account-specific Library browsing. A nonblank search also returns
    /// restricted games, marked for display; it never grants play permission.
    pub async fn list_games_for_account(
        &self,
        request: &ListGames,
        account: Option<&str>,
    ) -> Result<GamePage> {
        self.list_with_collection(request, None, false, account)
            .await
    }
    /// The hidden games, by title (Settings lists them to show again).
    pub async fn hidden_games(&self, offset: u32, limit: u32) -> Result<GamePage> {
        let request = ListGames {
            offset,
            limit,
            ..Default::default()
        };
        self.list_with_collection(&request, None, true, None).await
    }
    /// One of `account`'s collections' games; another account's collection
    /// is not found.
    pub async fn collection_games(
        &self,
        account: &str,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<GamePage> {
        let collection = self
            .get_collection(account, id)
            .await?
            .ok_or_else(|| missing("collection", id))?;
        self.collection_page(&collection, offset, limit).await
    }
    /// Every collection of `account`'s as its card shows it: pinned ones
    /// first, then by name.
    pub async fn collection_summaries(&self, account: &str) -> Result<Vec<CollectionSummary>> {
        let mut out = Vec::new();
        for collection in self.collection_list(account).await? {
            let page = self.collection_page(&collection, 0, 4).await?;
            let (request, members) = collection_query(&collection, 0, 0);
            let mut playtime = QueryBuilder::<Sqlite>::new("SELECT coalesce(sum(");
            playtime
                .push(PLAYTIME)
                .push("),0) FROM games g WHERE g.hidden=0");
            filters(&mut playtime, &request, members, false);
            let playtime_ms: i64 = playtime.build_query_scalar().fetch_one(&self.pool).await?;
            out.push(CollectionSummary {
                id: collection.id,
                name: collection.name,
                description: collection.description,
                pinned: collection.pinned,
                kind: collection.kind,
                updated_at_ms: collection.updated_at_ms,
                games: page.total,
                playtime_ms: playtime_ms as u64,
                covers: page
                    .items
                    .into_iter()
                    .filter_map(|game| game.cover)
                    .collect(),
            });
        }
        Ok(out)
    }
    async fn collection_page(
        &self,
        collection: &Collection,
        offset: u32,
        limit: u32,
    ) -> Result<GamePage> {
        let (request, members) = collection_query(collection, offset, limit);
        self.list_with_collection(&request, members, false, None)
            .await
    }
    async fn list_with_collection(
        &self,
        request: &ListGames,
        collection: Option<&str>,
        hidden: bool,
        account: Option<&str>,
    ) -> Result<GamePage> {
        request.validate()?;
        let mut tx = self.pool.begin().await?;
        let mut count = QueryBuilder::<Sqlite>::new("SELECT count(*) FROM games g WHERE g.hidden=");
        count.push_bind(hidden);
        filters(&mut count, request, collection, hidden);
        age_visibility(&mut count, request, account);
        let total: i64 = count.build_query_scalar().fetch_one(&mut *tx).await?;
        // The page's ids come first, sorted by their key alone, so the full
        // rows and their artwork are read for the page only, not for every
        // game the offset skips (deep pages of 8,000 games: 100 ms -> 10 ms).
        // Titles sort in the language's alphabet (#218).
        let (key, order) = match request.sort {
            GameSort::Popular => (
                "(SELECT p.value FROM igdb_matches m JOIN igdb_popularity p ON p.igdb_id=m.igdb_id WHERE m.game_id=g.id)",
                BY_KEY_THEN_TITLE,
            ),
            GameSort::Title => ("NULL", BY_TITLE),
            GameSort::Recent => (RECENT, BY_KEY_THEN_TITLE),
            GameSort::Playtime => (PLAYTIME, BY_KEY_THEN_TITLE),
            GameSort::Added => (ADDED, " ORDER BY k DESC,g.id"),
            GameSort::Released => (RELEASED, BY_KEY_THEN_TITLE),
        };
        let mut query = QueryBuilder::<Sqlite>::new("SELECT ");
        age_blocked(&mut query, account);
        query.push(" AS admin_pin_needed,g.record_json,");
        query.push(INSTALLED).push(" AS installed,").push(PLAYTIME).push(" AS playtime_ms,").push(RECENT).push(" AS last_played_at_ms,(SELECT a.record_json FROM artwork a WHERE a.game_id=g.id AND a.kind='cover' AND ").push(super::artwork_choice::SHOWN).push(" ORDER BY a.id LIMIT 1) AS cover,(SELECT a.record_json FROM artwork a WHERE a.game_id=g.id AND a.kind='icon' AND ").push(super::artwork_choice::SHOWN).push(" ORDER BY a.record_json->>'$.source' LIKE 'igdb:%' DESC,a.id LIMIT 1) AS icon FROM (SELECT g.id,");
        query.push(key).push(" AS k FROM games g WHERE g.hidden=");
        query.push_bind(hidden);
        filters(&mut query, request, collection, hidden);
        age_visibility(&mut query, request, account);
        query
            .push(order)
            .push(" LIMIT ")
            .push_bind(request.limit)
            .push(" OFFSET ")
            .push_bind(request.offset)
            .push(") p JOIN games g ON g.id=p.id")
            .push(order);
        let rows = query
            .build_query_as::<(
                bool,
                String,
                bool,
                i64,
                Option<i64>,
                Option<String>,
                Option<String>,
            )>()
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        // An unreadable record leaves its game out of the page, and an
        // unreadable image leaves the game without it, rather than failing
        // the whole library (#160).
        let image = |text: Option<String>| {
            text.and_then(|text| {
                decode(text)
                    .inspect_err(|error| tracing::warn!(%error, "library image record unreadable"))
                    .ok()
            })
        };
        let items = rows
            .into_iter()
            .filter_map(
                |(admin_pin_needed, record, installed, duration, last, cover, icon)| {
                    let game: Game = decode(record)
                        .inspect_err(
                            |error| tracing::warn!(%error, "game record unreadable; left out"),
                        )
                        .ok()?;
                    Some(GameSummary {
                        admin_pin_needed,
                        id: game.id,
                        title: game.title,
                        sort_title: game.sort_title,
                        description: game.description,
                        genres: game.genres,
                        tags: game.tags,
                        release_date: game.release_date,
                        platforms: game.platforms,
                        favorite: game.favorite,
                        hidden: game.hidden,
                        rating: game.rating,
                        cover: image(cover),
                        icon: image(icon),
                        installed,
                        playtime_ms: duration as u64,
                        last_played_at_ms: last,
                    })
                },
            )
            .collect::<Vec<_>>();
        Ok(GamePage {
            items,
            total: total as u64,
        })
    }
    /// The platforms of the visible games in `filter`, by name, each with its
    /// games' count, playtime and last play. A game on two platforms counts
    /// on both.
    pub async fn library_platforms(&self, filter: GameFilter) -> Result<Vec<PlatformSummary>> {
        self.library_platforms_for_account(filter, None).await
    }
    pub async fn library_platforms_for_account(
        &self,
        filter: GameFilter,
        account: Option<&str>,
    ) -> Result<Vec<PlatformSummary>> {
        let request = ListGames {
            filter,
            ..Default::default()
        };
        let mut query = QueryBuilder::<Sqlite>::new("SELECT p.value,count(*),coalesce(sum(");
        query
            .push(PLAYTIME)
            .push("),0),max(")
            .push(RECENT)
            .push(") FROM games g, json_each(g.record_json,'$.platforms') p WHERE p.type='text' AND g.hidden=0");
        filters(&mut query, &request, None, false);
        age_visibility(&mut query, &request, account);
        query.push(" GROUP BY p.value ORDER BY p.value COLLATE NOCASE");
        let rows = query
            .build_query_as::<(String, i64, i64, Option<i64>)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(name, games, playtime, last)| PlatformSummary {
                name,
                games: games as u64,
                playtime_ms: playtime as u64,
                last_played_at_ms: last,
            })
            .collect())
    }
    /// Where each initial letter starts in `request`'s games sorted by title
    /// (its sort, offset and limit are ignored), in that order: the
    /// language's letters (A to Z, then Å, Ä and Ö in Swedish; É files
    /// under E), and `#` for titles that start with anything else, at the
    /// first of them.
    pub async fn title_letters(&self, request: &ListGames) -> Result<Vec<TitleLetter>> {
        self.title_letters_for_account(request, None).await
    }
    pub async fn title_letters_for_account(
        &self,
        request: &ListGames,
        account: Option<&str>,
    ) -> Result<Vec<TitleLetter>> {
        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT g.sort_title FROM games g WHERE g.hidden=0");
        filters(&mut query, request, None, false);
        age_visibility(&mut query, request, account);
        query.push(BY_TITLE);
        let titles: Vec<String> = query.build_query_scalar().fetch_all(&self.pool).await?;
        Ok(self
            .titles
            .current()
            .letters(titles.iter().map(String::as_str))
            .into_iter()
            .map(|(letter, offset)| TitleLetter { letter, offset })
            .collect())
    }
    pub async fn library_stats(&self) -> Result<LibraryStats> {
        self.library_stats_for_account(None).await
    }
    pub async fn library_stats_for_account(&self, account: Option<&str>) -> Result<LibraryStats> {
        // Each tab's count; play time is the whole library's.
        let query = format!(
            "SELECT coalesce(sum(n),0),coalesce(sum(n AND {INSTALLED}),0),coalesce(sum(n AND g.favorite),0),coalesce(sum(n AND {RECENT} IS NOT NULL),0),coalesce(sum(n AND {EMULATED}),0),coalesce(sum({WEB}),0),coalesce(sum({PLAYTIME}),0),coalesce(sum(n AND {OWNED} AND NOT {INSTALLED}),0) FROM (SELECT g.*,{NOT_WEB_ONLY} AS n FROM games g WHERE g.hidden=0"
        );
        let mut query = QueryBuilder::<Sqlite>::new(query);
        age_visibility(&mut query, &ListGames::default(), account);
        query.push(") g");
        let (total, installed, favorites, recent, emulated, web, playtime, owned): (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = query.build_query_as().fetch_one(&self.pool).await?;
        Ok(LibraryStats {
            total: total as u64,
            installed: installed as u64,
            favorites: favorites as u64,
            recent: recent as u64,
            emulated: emulated as u64,
            web: web as u64,
            owned: owned as u64,
            playtime_ms: playtime as u64,
        })
    }
}

/// The library query that lists a collection's games: a smart one's own,
/// or a manual one's members by title.
fn collection_query(collection: &Collection, offset: u32, limit: u32) -> (ListGames, Option<&str>) {
    match &collection.kind {
        CollectionKind::Manual => (
            ListGames {
                offset,
                limit,
                ..Default::default()
            },
            Some(&collection.id),
        ),
        CollectionKind::Smart { query } => (
            ListGames {
                offset,
                limit,
                ..query.clone()
            },
            None,
        ),
    }
}

/// `request`'s conditions. Web games alone leave the library's other
/// filters, but not a manual collection or the hidden games' list.
fn filters(
    query: &mut QueryBuilder<Sqlite>,
    request: &ListGames,
    collection: Option<&str>,
    hidden: bool,
) {
    if request.filter != GameFilter::Web && collection.is_none() && !hidden {
        query.push(" AND ").push(NOT_WEB_ONLY);
    }
    match request.filter {
        GameFilter::All => (),
        GameFilter::Installed => {
            query.push(" AND ").push(INSTALLED);
        }
        GameFilter::Favorites => {
            query.push(" AND g.favorite=1");
        }
        GameFilter::Recent => {
            query.push(" AND ").push(RECENT).push(" IS NOT NULL");
        }
        GameFilter::Emulated => {
            query.push(" AND ").push(EMULATED);
        }
        GameFilter::Web => {
            query.push(" AND ").push(WEB);
        }
        GameFilter::Owned => {
            query
                .push(" AND ")
                .push(OWNED)
                .push(" AND NOT ")
                .push(INSTALLED);
        }
    }
    if let Some(platform) = &request.platform {
        query
            .push(
                " AND EXISTS(SELECT 1 FROM json_each(g.record_json,'$.platforms') p WHERE p.value=",
            )
            .push_bind(platform.clone())
            .push(")");
    }
    if let Some(collection) = collection {
        query.push(" AND EXISTS(SELECT 1 FROM collection_members cm WHERE cm.game_id=g.id AND cm.collection_id=").push_bind(collection.to_owned()).push(")");
    }
    let Some(text) = request.query.as_deref() else {
        return;
    };
    let mut long = Vec::new();
    for token in text.split_whitespace() {
        if token.chars().count() >= 3 {
            long.push(format!("\"{}\"", token.replace('"', "\"\"")));
        } else {
            // Case and accents aside, as the trigram index (#218).
            let pattern = folded_glob(token);
            query
                .push(" AND (g.title GLOB ")
                .push_bind(pattern.clone())
                .push(" OR g.sort_title GLOB ")
                .push_bind(pattern.clone())
                .push(" OR g.aliases_json GLOB ")
                .push_bind(pattern.clone())
                .push(" OR g.tags_json GLOB ")
                .push_bind(pattern)
                .push(")");
        }
    }
    if !long.is_empty() {
        query
            .push(" AND g.rowid IN (SELECT rowid FROM game_search WHERE game_search MATCH ")
            .push_bind(long.join(" AND "))
            .push(")");
    }
}

/// The same persistent restriction as the core play gate. One-time approvals
/// deliberately do not affect browsing or lock labels.
fn age_blocked(query: &mut QueryBuilder<Sqlite>, account: Option<&str>) {
    let Some(account) = account else {
        query.push("0");
        return;
    };
    query.push("EXISTS(SELECT 1 FROM accounts ac WHERE ac.id=")
        .push_bind(account.to_owned())
        .push(" AND ac.role='standard' AND ac.max_age IS NOT NULL AND (g.min_age>ac.max_age OR (g.min_age IS NULL AND ac.unrated_needs_pin)) AND NOT EXISTS(SELECT 1 FROM account_game_approvals ap WHERE ap.account_id=ac.id AND ap.game_id=g.id))");
}

fn age_visibility(query: &mut QueryBuilder<Sqlite>, request: &ListGames, account: Option<&str>) {
    if account.is_some()
        && !request
            .query
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        query.push(" AND NOT ");
        age_blocked(query, account);
    }
}
