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
/// The entries created from other devices' installed-copy announcements.
const PEER_ENTRIES: &str =
    " FROM game_entries pe WHERE pe.game_id=g.id AND pe.source_id LIKE 'peer:%'";
/// Tracked sessions plus what reported totals add (#169, `db::playtime`).
const PLAYTIME: &str = "(coalesce((SELECT sum(s.duration_ms) FROM play_sessions s WHERE s.game_id=g.id),0)+coalesce((SELECT sum(r.counted_ms) FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id WHERE re.game_id=g.id),0))";
/// Owned by a store account that still owns it (#170).
const OWNED: &str = "EXISTS(SELECT 1 FROM game_entries oe JOIN account_entitlements ae ON ae.entry_id=oe.id WHERE oe.game_id=g.id AND ae.lost_at_ms IS NULL)";
/// The latest session start or reported last play.
const RECENT: &str = "(SELECT max(t) FROM (SELECT max(s.started_at_ms) AS t FROM play_sessions s WHERE s.game_id=g.id UNION ALL SELECT max(r.last_played_ms) FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id WHERE re.game_id=g.id))";
/// The device the latest session was played on, as a `PlayedOn`, when it
/// is another one (#74): its name is what this device calls it.
fn played_on() -> String {
    format!(
        "(SELECT CASE WHEN s.device_id IS NOT NULL AND s.device_id IS NOT {this} \
         THEN json_object('device',s.device_id,'name',(SELECT d.name FROM paired_devices d WHERE d.endpoint_id=s.device_id)) END \
         FROM play_sessions s WHERE s.game_id=g.id ORDER BY s.started_at_ms DESC,s.id LIMIT 1)",
        this = super::sessions::THIS_DEVICE
    )
}
/// Ids are UUIDv7s, so the greatest is the newest: the game's own, or its
/// newest entry's. A game with no entries left (its install uninstalled)
/// has nothing added, so it sorts last.
const ADDED: &str = "(SELECT max(g.id,max(e.id)) FROM game_entries e WHERE e.game_id=g.id)";
const RELEASED: &str = "g.record_json->>'$.releaseDate'";
const EMULATED: &str =
    "EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind='emulator')";
const WEB: &str =
    "EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id AND e.launch_kind='web')";
/// Game `g` needs an admin PIN for account `ac` (#209): the core play gate's
/// rule, before approvals. Standard accounts with an age limit only.
pub(super) const AGE_HELD: &str = "(ac.role='standard' AND ac.max_age IS NOT NULL AND (g.min_age>ac.max_age OR (g.min_age IS NULL AND ac.unrated_needs_pin)))";
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
        Ok(self
            .list_with_collection(request, None, false, account, false)
            .await?
            .0)
    }
    /// The hidden games, by title (Settings lists them to show again).
    pub async fn hidden_games(&self, offset: u32, limit: u32) -> Result<GamePage> {
        let request = ListGames {
            offset,
            limit,
            ..Default::default()
        };
        Ok(self
            .list_with_collection(&request, None, true, None, false)
            .await?
            .0)
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
        let (request, members) = collection_query(&collection, offset, limit);
        Ok(self
            .list_with_collection(&request, members, false, Some(account), false)
            .await?
            .0)
    }
    /// Every collection of `account`'s as its card shows it: pinned ones
    /// first, then by name.
    pub async fn collection_summaries(&self, account: &str) -> Result<Vec<CollectionSummary>> {
        let collections = self.collection_list(account).await?;
        if collections.is_empty() {
            return Ok(Vec::new());
        }
        let age_account = self.age_limited_account(Some(account)).await?;
        let mut out = Vec::with_capacity(collections.len());
        let mut tx = self.pool.begin().await?;
        for collection in collections {
            let (request, members) = collection_query(&collection, 0, 4);
            let (page, playtime_ms) = list_with_collection_in(
                &mut tx,
                &request,
                members,
                false,
                age_account.as_deref(),
                true,
            )
            .await?;
            out.push(CollectionSummary {
                id: collection.id,
                name: collection.name,
                description: collection.description,
                pinned: collection.pinned,
                kind: collection.kind,
                updated_at_ms: collection.updated_at_ms,
                games: page.total,
                playtime_ms,
                covers: page
                    .items
                    .into_iter()
                    .filter_map(|game| game.cover)
                    .collect(),
            });
        }
        tx.commit().await?;
        Ok(out)
    }
    /// Returns `account` only if it is a standard account with an age limit
    /// (`max_age IS NOT NULL`). Admin and unrestricted accounts skip the
    /// per-row `EXISTS(SELECT 1 FROM accounts ...)` check completely.
    async fn age_limited_account(&self, account: Option<&str>) -> Result<Option<String>> {
        let Some(account) = account else {
            return Ok(None);
        };
        let limited: Option<bool> = sqlx::query_scalar(
            "SELECT role='standard' AND max_age IS NOT NULL FROM accounts WHERE id=?",
        )
        .bind(account)
        .fetch_optional(&self.pool)
        .await?;
        Ok(limited.unwrap_or(false).then(|| account.to_owned()))
    }
    /// A page of games, and with `playtime` their total playtime (0
    /// without). A collection is its account's, so it browses as the
    /// Library does for that account: games held back by age limits show
    /// only where a smart collection searches by title, locked.
    async fn list_with_collection(
        &self,
        request: &ListGames,
        collection: Option<&str>,
        hidden: bool,
        account: Option<&str>,
        playtime: bool,
    ) -> Result<(GamePage, u64)> {
        let age_account = self.age_limited_account(account).await?;
        let mut tx = self.pool.begin().await?;
        let result = list_with_collection_in(
            &mut tx,
            request,
            collection,
            hidden,
            age_account.as_deref(),
            playtime,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
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
        self.library_platforms_for_scope(filter, account, false)
            .await
    }
    pub async fn library_platforms_for_scope(
        &self,
        filter: GameFilter,
        account: Option<&str>,
        local_only: bool,
    ) -> Result<Vec<PlatformSummary>> {
        let age_account = self.age_limited_account(account).await?;
        let request = ListGames {
            filter,
            local_only,
            ..Default::default()
        };
        // Aggregate per-game playtime and recency once before expanding multi-platform JSON arrays.
        let mut query = QueryBuilder::<Sqlite>::new(
            "WITH sess AS (SELECT game_id,sum(duration_ms) AS dur,max(started_at_ms) AS last_at \
             FROM play_sessions GROUP BY game_id), \
             rem AS (SELECT re.game_id,sum(r.counted_ms) AS dur,max(r.last_played_ms) AS last_at \
             FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id GROUP BY re.game_id) \
             SELECT p.value,count(*),coalesce(sum(coalesce(sess.dur,0)+coalesce(rem.dur,0)),0),\
             max(CASE WHEN sess.last_at IS NULL THEN rem.last_at \
                      WHEN rem.last_at IS NULL THEN sess.last_at \
                      WHEN sess.last_at>=rem.last_at THEN sess.last_at \
                      ELSE rem.last_at END) \
             FROM games g \
             LEFT JOIN sess ON sess.game_id=g.id \
             LEFT JOIN rem ON rem.game_id=g.id, \
             json_each(g.record_json,'$.platforms') p \
             WHERE p.type='text' AND g.hidden=0",
        );
        filters(&mut query, &request, None, false);
        age_visibility(&mut query, &request, age_account.as_deref());
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
        let age_account = self.age_limited_account(account).await?;
        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT g.sort_title FROM games g WHERE g.hidden=0");
        filters(&mut query, request, None, false);
        age_visibility(&mut query, request, age_account.as_deref());
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
    /// IDs of games that have at least one installed non-plugin launchable release.
    pub async fn playable_game_ids(&self) -> Result<std::collections::HashSet<String>> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT game_id FROM game_entries \
             WHERE state_kind='installed' AND launch_kind IS NOT NULL AND launch_kind<>'plugin'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    pub async fn library_stats(&self) -> Result<LibraryStats> {
        self.library_stats_for_account(None).await
    }
    pub async fn library_stats_for_account(&self, account: Option<&str>) -> Result<LibraryStats> {
        self.library_stats_for_scope(account, false).await
    }
    pub async fn library_stats_for_scope(
        &self,
        account: Option<&str>,
        local_only: bool,
    ) -> Result<LibraryStats> {
        // Each tab's count; play time is the whole library's. Ready to
        // Install counts other devices' copies that can come here (#232).
        // Pre-aggregate entries, sessions, remote playtime, entitlements and
        // copyable peer copies once per statement instead of running 10+
        // correlated subqueries per game.
        let age_account = self.age_limited_account(account).await?;
        let copyable_set = super::sync_copies::copyable_games_cte_sql();
        let sql = format!(
            "WITH ent AS (\
                SELECT game_id,\
                       max(state_kind IN ('installed','updating','broken')) AS has_installed,\
                       max(launch_kind='web') AS has_web,\
                       max(launch_kind IS NOT 'web') AS has_non_web,\
                       max(launch_kind='emulator') AS has_emulated,\
                       max(source_id LIKE 'peer:%') AS has_peer \
                FROM game_entries GROUP BY game_id\
             ), \
             sess AS (\
                SELECT game_id,sum(duration_ms) AS dur,max(started_at_ms) AS last_at \
                FROM play_sessions GROUP BY game_id\
             ), \
             rem AS (\
                SELECT re.game_id,sum(r.counted_ms) AS dur,max(r.last_played_ms) AS last_at \
                FROM remote_playtime r JOIN game_entries re ON re.id=r.entry_id GROUP BY re.game_id\
             ), \
             own AS (\
                SELECT DISTINCT oe.game_id \
                FROM game_entries oe JOIN account_entitlements ae ON ae.entry_id=oe.id \
                WHERE ae.lost_at_ms IS NULL\
             ), \
             cpy AS ({copyable_set}) \
             SELECT \
                coalesce(sum(n),0),\
                coalesce(sum(n AND inst),0),\
                coalesce(sum(n AND fav),0),\
                coalesce(sum(n AND rec),0),\
                coalesce(sum(n AND emu),0),\
                coalesce(sum(web),0),\
                coalesce(sum(pt),0),\
                coalesce(sum(n AND (is_own OR is_cpy) AND NOT inst),0) \
             FROM (\
                SELECT \
                    g.favorite AS fav,\
                    (coalesce(ent.has_web,0)=0 OR coalesce(ent.has_non_web,0)=1) AS n,\
                    coalesce(ent.has_installed,0) AS inst,\
                    (sess.last_at IS NOT NULL OR rem.last_at IS NOT NULL) AS rec,\
                    coalesce(ent.has_emulated,0) AS emu,\
                    coalesce(ent.has_web,0) AS web,\
                    (coalesce(sess.dur,0)+coalesce(rem.dur,0)) AS pt,\
                    (own.game_id IS NOT NULL) AS is_own,\
                    (cpy.game_id IS NOT NULL) AS is_cpy \
                FROM games g \
                LEFT JOIN ent ON ent.game_id=g.id \
                LEFT JOIN sess ON sess.game_id=g.id \
                LEFT JOIN rem ON rem.game_id=g.id \
                LEFT JOIN own ON own.game_id=g.id \
                LEFT JOIN cpy ON cpy.game_id=g.id \
                WHERE g.hidden=0"
        );
        let mut query = QueryBuilder::<Sqlite>::new(sql);
        if local_only {
            query.push(" AND (coalesce(ent.has_installed,0)=1 OR coalesce(ent.has_peer,0)=0)");
        }
        age_visibility(&mut query, &ListGames::default(), age_account.as_deref());
        query.push(")");
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

async fn list_with_collection_in(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    request: &ListGames,
    collection: Option<&str>,
    hidden: bool,
    age_account: Option<&str>,
    playtime: bool,
) -> Result<(GamePage, u64)> {
    request.validate()?;
    let mut count = QueryBuilder::<Sqlite>::new("SELECT count(*),");
    if playtime {
        count.push("coalesce(sum(").push(PLAYTIME).push("),0)");
    } else {
        count.push("0");
    }
    count
        .push(" FROM games g WHERE g.hidden=")
        .push_bind(hidden);
    filters(&mut count, request, collection, hidden);
    age_visibility(&mut count, request, age_account);
    let (total, playtime_ms): (i64, i64) = count.build_query_as().fetch_one(&mut **tx).await?;
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
    age_blocked(&mut query, age_account);
    query.push(" AS admin_pin_needed,g.record_json,");
    query
        .push(INSTALLED)
        .push(" AS installed,(SELECT json_group_array(DISTINCT substr(pe.source_id,6))")
        .push(PEER_ENTRIES)
        .push(") AS remote_device_ids,");
    query.push(PLAYTIME).push(" AS playtime_ms,").push(RECENT).push(" AS last_played_at_ms,").push(played_on()).push(" AS played_on,(SELECT a.record_json FROM artwork a WHERE a.game_id=g.id AND a.kind='cover' AND ").push(super::artwork_choice::SHOWN).push(" ORDER BY a.id LIMIT 1) AS cover,(SELECT a.record_json FROM artwork a WHERE a.game_id=g.id AND a.kind='icon' AND ").push(super::artwork_choice::SHOWN).push(" ORDER BY a.record_json->>'$.source' LIKE 'igdb:%' DESC,a.id LIMIT 1) AS icon FROM (SELECT g.id,");
    query.push(key).push(" AS k FROM games g WHERE g.hidden=");
    query.push_bind(hidden);
    filters(&mut query, request, collection, hidden);
    age_visibility(&mut query, request, age_account);
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
            sqlx::types::Json<Vec<String>>,
            i64,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>()
        .fetch_all(&mut **tx)
        .await?;
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
            |(
                admin_pin_needed,
                record,
                installed,
                remote_devices,
                duration,
                last,
                played_on,
                cover,
                icon,
            )| {
                let game: Game = decode(record)
                    .inspect_err(|error| tracing::warn!(%error, "game record unreadable; left out"))
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
                    remote_device_ids: remote_devices.0,
                    playtime_ms: duration as u64,
                    last_played_at_ms: last,
                    last_played_on: played_on.and_then(|text| {
                        decode(text)
                            .inspect_err(
                                |error| tracing::warn!(%error, "played-on device unreadable"),
                            )
                            .ok()
                    }),
                })
            },
        )
        .collect::<Vec<_>>();
    Ok((
        GamePage {
            items,
            total: total as u64,
        },
        playtime_ms as u64,
    ))
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
    local_games(query, request.local_only);
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
            // Owned, or another device's copy that can come here (#232).
            query
                .push(" AND (")
                .push(OWNED)
                .push(" OR ")
                .push(super::sync_copies::copyable_here_sql())
                .push(") AND NOT ")
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

fn local_games(query: &mut QueryBuilder<Sqlite>, local_only: bool) {
    if local_only {
        query
            .push(" AND (")
            .push(INSTALLED)
            .push(" OR NOT EXISTS(SELECT 1")
            .push(PEER_ENTRIES)
            .push("))");
    }
}

/// The same persistent restriction as the core play gate. One-time approvals
/// deliberately do not affect browsing or lock labels.
fn age_blocked(query: &mut QueryBuilder<Sqlite>, account: Option<&str>) {
    let Some(account) = account else {
        query.push("0");
        return;
    };
    query
        .push("EXISTS(SELECT 1 FROM accounts ac WHERE ac.id=")
        .push_bind(account.to_owned())
        .push(" AND ")
        .push(AGE_HELD)
        .push(" AND NOT EXISTS(SELECT 1 FROM account_game_approvals ap WHERE ap.account_id=ac.id AND ap.game_id=g.id))");
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
