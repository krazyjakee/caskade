//! Reported play time against tracked sessions (#169), on a fake clock.

use super::*;
use crate::model::{Game, GameEntry, InstallLocation, InstallState, PlaySession};
use caskade_discovery::ExternalId;

const HOUR: u64 = 3_600_000;
const T0: i64 = 1_700_000_000_000;

struct Fixture {
    store: LibraryStore,
    game: String,
    /// Steam discovery's release of app 620.
    steam: String,
    /// The store account's release of app 620.
    owned: String,
}

async fn fixture() -> Fixture {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Portal 2");
    let release = |source: &str, key: &str, installed: bool| {
        let mut entry = GameEntry::new(&game.id, source.to_owned().into(), key);
        entry.ids = vec![ExternalId::Steam { app_id: 620 }];
        if installed {
            entry.state = InstallState::Installed;
            entry.location = Some(InstallLocation::External {
                path: "/games/portal2".into(),
            });
        }
        entry
    };
    let steam = release("steam", "620", true);
    let owned = release("store:acct", "620", false);
    store
        .import_group(&game, &[steam.clone(), owned.clone()], None)
        .await
        .unwrap();
    Fixture {
        store,
        game: game.id,
        steam: steam.id,
        owned: owned.id,
    }
}

impl Fixture {
    async fn report(&self, account: &str, entry: &str, total: Option<u64>, at: i64) -> Vec<String> {
        self.report_in(account, entry, total, at, PlaytimeImport::Always)
            .await
    }
    async fn report_in(
        &self,
        account: &str,
        entry: &str,
        total: Option<u64>,
        at: i64,
        mode: PlaytimeImport,
    ) -> Vec<String> {
        self.store
            .record_remote_playtime(
                "steam",
                account,
                &[RemoteTotal {
                    entry_id: entry.into(),
                    total_ms: total,
                    last_played_ms: None,
                }],
                at,
                mode,
            )
            .await
            .unwrap()
    }
    /// A session Caskade tracked on `entry`, ending at `end`.
    async fn play(&self, entry: &str, hours: u64, end: i64) {
        let mut session = PlaySession::new(&self.game, entry, end - signed(hours * HOUR));
        session.ended_at_ms = Some(end);
        session.heartbeat_at_ms = end;
        session.duration_ms = hours * HOUR;
        self.store.upsert_session(&session).await.unwrap();
    }
    async fn total(&self) -> u64 {
        self.store
            .get_game(&self.game)
            .await
            .unwrap()
            .unwrap()
            .playtime_ms
    }
}

#[tokio::test]
async fn remote_totals_catch_up_with_local_sessions_without_counting_twice() {
    let f = fixture().await;
    // Steam says 10 h before Caskade played anything.
    f.report("acct", &f.owned, Some(10 * HOUR), T0).await;
    assert_eq!(f.total().await, 10 * HOUR);
    // Two hours through Caskade (launched with Steam): 12 h until Steam's
    // total says so too.
    f.play(&f.steam, 2, T0 + 5 * signed(HOUR)).await;
    assert_eq!(f.total().await, 12 * HOUR);
    // Steam catches up: 12 h, of which 2 h are Caskade's own.
    f.report("acct", &f.owned, Some(12 * HOUR), T0 + 6 * signed(HOUR))
        .await;
    assert_eq!(f.total().await, 12 * HOUR);
    // The same answer again changes nothing.
    assert!(
        f.report("acct", &f.owned, Some(12 * HOUR), T0 + 7 * signed(HOUR))
            .await
            .is_empty()
    );
    assert_eq!(f.total().await, 12 * HOUR);
    let breakdown = f.store.playtime_breakdown(&f.game).await.unwrap();
    assert_eq!(
        (
            breakdown.tracked_ms,
            breakdown.reported_ms,
            breakdown.total_ms
        ),
        (2 * HOUR, 10 * HOUR, 12 * HOUR)
    );
    assert_eq!(breakdown.sources[0].total_ms, Some(12 * HOUR));
}

#[tokio::test]
async fn a_launcher_and_an_account_reporting_one_total_count_once() {
    let f = fixture().await;
    // Steam's client on this computer and the account report the same time.
    f.report("", &f.steam, Some(8 * HOUR), T0).await;
    f.report("acct", &f.owned, Some(9 * HOUR), T0).await;
    assert_eq!(f.total().await, 9 * HOUR);
    // A second account's play is play of its own.
    f.report("other", &f.owned, Some(3 * HOUR), T0).await;
    assert_eq!(f.total().await, 12 * HOUR);
}

#[tokio::test]
async fn resets_stale_reads_and_unknown_totals_never_lower_or_invent_play() {
    let f = fixture().await;
    f.report("acct", &f.owned, Some(10 * HOUR), T0).await;
    // The store resets it: the largest total stays.
    f.report("acct", &f.owned, Some(HOUR), T0 + 1).await;
    assert_eq!(f.total().await, 10 * HOUR);
    // An older read than the one kept is ignored.
    assert!(
        f.report("acct", &f.owned, Some(50 * HOUR), T0 - 1)
            .await
            .is_empty()
    );
    assert_eq!(f.total().await, 10 * HOUR);
    // Unknown isn't zero: nothing is written.
    let g = fixture().await;
    g.report("acct", &g.owned, None, T0).await;
    let breakdown = g.store.playtime_breakdown(&g.game).await.unwrap();
    assert!(breakdown.sources.is_empty());
    // Zero is a total.
    g.report("acct", &g.owned, Some(0), T0).await;
    let breakdown = g.store.playtime_breakdown(&g.game).await.unwrap();
    assert_eq!(breakdown.sources[0].total_ms, Some(0));
    assert_eq!(breakdown.last_played_ms, None);
}

#[tokio::test]
async fn import_modes_keep_the_first_total_or_none() {
    let f = fixture().await;
    f.report_in(
        "acct",
        &f.owned,
        Some(5 * HOUR),
        T0,
        PlaytimeImport::NewGames,
    )
    .await;
    f.report_in(
        "acct",
        &f.owned,
        Some(9 * HOUR),
        T0 + 1,
        PlaytimeImport::NewGames,
    )
    .await;
    assert_eq!(f.total().await, 5 * HOUR);
    f.report_in(
        "acct",
        &f.owned,
        Some(9 * HOUR),
        T0 + 2,
        PlaytimeImport::Never,
    )
    .await;
    assert_eq!(f.total().await, 0);
    f.report("acct", &f.owned, Some(9 * HOUR), T0 + 3).await;
    assert_eq!(f.forget_and_total("acct").await, 0);
}

impl Fixture {
    async fn forget_and_total(&self, account: &str) -> u64 {
        self.store.forget_remote_playtime(account).await.unwrap();
        self.total().await
    }
}

#[tokio::test]
async fn copies_on_two_stores_add_up_and_a_merge_counts_shared_totals_once() {
    let store = LibraryStore::in_memory().await.unwrap();
    let release = |game: &Game, source: &str, id: ExternalId| {
        let mut entry = GameEntry::new(&game.id, source.to_owned().into(), "x");
        entry.ids = vec![id];
        entry
    };
    let (a, b) = (Game::new("Stardew Valley"), Game::new("Stardew Valley"));
    let steam = release(&a, "store:s", ExternalId::Steam { app_id: 413_150 });
    let gog = release(
        &b,
        "store:g",
        ExternalId::Gog {
            product_id: 1_207_666_393,
        },
    );
    let launcher = release(
        &b,
        "gog",
        ExternalId::Gog {
            product_id: 1_207_666_393,
        },
    );
    store
        .import_group(&a, std::slice::from_ref(&steam), None)
        .await
        .unwrap();
    store
        .import_group(&b, &[gog.clone(), launcher.clone()], None)
        .await
        .unwrap();
    let total = |entry: &str, provider: &str, account: &str, hours: u64| {
        let store = store.clone();
        let (entry, provider, account) =
            (entry.to_owned(), provider.to_owned(), account.to_owned());
        async move {
            store
                .record_remote_playtime(
                    &provider,
                    &account,
                    &[RemoteTotal {
                        entry_id: entry,
                        total_ms: Some(hours * HOUR),
                        last_played_ms: Some(T0),
                    }],
                    T0,
                    PlaytimeImport::Always,
                )
                .await
                .unwrap()
        }
    };
    total(&steam.id, "steam", "s", 4).await;
    total(&gog.id, "gog", "g", 6).await;
    total(&launcher.id, "gog", "", 5).await;
    // One game now: Steam's 4 h and GOG's 6 h (its launcher's 5 h overlap).
    store.merge_games(&a.id, &b.id).await.unwrap();
    let breakdown = store.playtime_breakdown(&a.id).await.unwrap();
    assert_eq!(breakdown.total_ms, 10 * HOUR);
    assert_eq!(breakdown.last_played_ms, Some(T0));
}

#[tokio::test]
async fn a_running_session_and_real_history_survive_refreshes() {
    let f = fixture().await;
    f.play(&f.steam, 1, T0).await;
    let mut running = PlaySession::new(&f.game, &f.steam, T0 + 10);
    running.duration_ms = HOUR / 2;
    f.store.start_session(&running).await.unwrap();
    f.report("acct", &f.owned, Some(3 * HOUR), T0 + 20).await;
    // 1 h tracked inside Steam's 3 h, plus the half hour running.
    assert_eq!(f.total().await, 3 * HOUR + HOUR / 2);
    let sessions = f.store.sessions_for_game(&f.game, 10).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn the_migration_turns_seed_sessions_into_a_launcher_total_once() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Seeded");
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "70");
    entry.ids = vec![ExternalId::Steam { app_id: 70 }];
    store
        .import_group(&game, std::slice::from_ref(&entry), None)
        .await
        .unwrap();
    // As #100 seeded it.
    let mut seed = PlaySession::new(&game.id, &entry.id, T0);
    seed.ended_at_ms = Some(T0);
    seed.heartbeat_at_ms = T0;
    seed.duration_ms = 7 * HOUR;
    seed.tracking =
        Some(serde_json::json!({"discoverySeed":{"source":"steam","key":"70","aggregate":true}}));
    store.upsert_session(&seed).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/0024_remote_playtime.down.sql"
    ))
    .execute(&store.pool)
    .await
    .unwrap();
    // The down migration made one seed from nothing; drop it, keep ours.
    sqlx::query("DELETE FROM play_sessions WHERE id<>?")
        .bind(&seed.id)
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version=24")
        .execute(&store.pool)
        .await
        .unwrap();
    super::super::MIGRATOR.run(&store.pool).await.unwrap();
    let breakdown = store.playtime_breakdown(&game.id).await.unwrap();
    assert_eq!(
        (breakdown.tracked_ms, breakdown.reported_ms),
        (0, 7 * HOUR),
        "{breakdown:?}"
    );
    assert_eq!(breakdown.sources[0].provider, "steam");
    assert_eq!(breakdown.last_played_ms, Some(T0));
    assert!(
        store
            .sessions_for_game(&game.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // Discovery seeding again (a re-import) doesn't make a second copy.
    let mut again = PlaySession::new(&game.id, &entry.id, T0);
    again.ended_at_ms = Some(T0);
    again.duration_ms = 7 * HOUR;
    again.tracking = seed.tracking.clone();
    store
        .import_group(&game, std::slice::from_ref(&entry), Some(&again))
        .await
        .unwrap();
    assert_eq!(
        store.get_game(&game.id).await.unwrap().unwrap().playtime_ms,
        7 * HOUR
    );
}
