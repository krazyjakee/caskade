//! The feed on a fake clock: hourly batches, replays, restarts, merges,
//! read-state races, failures, updates and cleanup.

use super::*;
use crate::{
    bus::Event,
    db::LibraryStore,
    install::{JobKind, JobSnapshot, JobStatus},
    model::{Game, GameEntry, SourceId, new_id},
};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

const T0: i64 = 1_800_000_000_000;
const MIN: i64 = 60 * 1000;

struct Fixture {
    store: LibraryStore,
    clock: Arc<AtomicI64>,
    feed: Notifications,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_store(LibraryStore::in_memory().await.unwrap()).await
    }

    async fn with_store(store: LibraryStore) -> Self {
        let clock = Arc::new(AtomicI64::new(T0));
        let feed = Notifications::with_clock(store.clone(), {
            let clock = clock.clone();
            Arc::new(move || clock.load(Ordering::SeqCst))
        });
        Self { store, clock, feed }
    }

    fn at(&self, at: i64) {
        self.clock.store(at, Ordering::SeqCst);
    }

    /// Imports a game from `source`, added at `at`.
    async fn add(&self, title: &str, source: &str, at: i64) -> String {
        let game = Game::new(title);
        let entry = GameEntry::new(&game.id, SourceId::from(source.to_owned()), new_id());
        self.import(&game, entry, at).await
    }

    async fn import(&self, game: &Game, entry: GameEntry, at: i64) -> String {
        let imported = self.store.import_group(game, &[entry], None).await.unwrap();
        if imported.created {
            self.store
                .set_addition_time(&imported.game_id, at)
                .await
                .unwrap();
        }
        imported.game_id
    }

    async fn items(&self) -> Vec<Notification> {
        self.feed.list(false, 0, 100).await.unwrap().items
    }
}

#[tokio::test]
async fn ten_additions_within_an_hour_are_one_item_with_their_sources() {
    let fx = Fixture::new().await;
    for i in 0..10 {
        let source = if i < 6 { "romm:home" } else { "steam" };
        fx.add(&format!("Game {i}"), source, T0 + i * 5 * MIN).await;
    }
    fx.at(T0 + 50 * MIN);
    assert!(fx.feed.ingest_additions().await.unwrap());
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    let batch = &items[0];
    assert_eq!(batch.category, NotificationCategory::GamesAdded);
    assert_eq!(batch.title, "10 games added");
    assert_eq!(batch.count, 10);
    assert_eq!((batch.first_at_ms, batch.last_at_ms), (T0, T0 + 45 * MIN));
    assert_eq!(batch.body, "RomM 6 · Steam 4");
    assert_eq!(batch.target, NotificationTarget::Members);
    assert!(batch.unread);
    assert_eq!(fx.feed.unread().await.unwrap(), 1);
}

#[tokio::test]
async fn a_batch_covers_an_hour_from_its_first_addition_then_the_next_opens() {
    let fx = Fixture::new().await;
    fx.add("A", "steam", T0).await;
    fx.add("B", "steam", T0 + 59 * MIN).await;
    fx.add("C", "steam", T0 + 60 * MIN).await;
    fx.add("D", "steam", T0 + 61 * MIN).await;
    fx.at(T0 + 62 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let mut items = fx.items().await;
    items.sort_by_key(|item| item.first_at_ms);
    let spans: Vec<_> = items
        .iter()
        .map(|item| (item.first_at_ms, item.last_at_ms, item.count))
        .collect();
    assert_eq!(
        spans,
        vec![(T0, T0 + 59 * MIN, 2), (T0 + 60 * MIN, T0 + 61 * MIN, 2)]
    );
    // Shown promptly: the first addition is in the feed before its hour is up.
    fx.add("E", "steam", T0 + 3 * 60 * MIN).await;
    fx.at(T0 + 3 * 60 * MIN + 1000);
    fx.feed.ingest_additions().await.unwrap();
    assert_eq!(fx.items().await[0].title, "E added");
}

#[tokio::test]
async fn an_initial_import_of_many_games_is_one_summary() {
    let fx = Fixture::new().await;
    for i in 0..1203 {
        fx.add(&format!("Game {i}"), "steam", T0 + i).await;
    }
    fx.at(T0 + 10 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1203);
    let members = fx.feed.members(&items[0].id, 1200, 100).await.unwrap();
    assert_eq!((members.total, members.items.len()), (1203, 3));
}

#[tokio::test]
async fn rescans_metadata_updates_and_replays_count_nothing_and_keep_read_state() {
    let fx = Fixture::new().await;
    let game = Game::new("Celeste");
    let entry = GameEntry::new(&game.id, SourceId::new("steam"), "504230");
    let id = fx.import(&game, entry.clone(), T0).await;
    fx.at(T0 + MIN);
    fx.feed.ingest_additions().await.unwrap();
    let item = fx.items().await.remove(0);
    fx.feed.mark_read(&item.id, item.version).await.unwrap();

    // The same source entry again, under a new proposed id: the same game.
    let again = Game::new("Celeste");
    let mut rescanned = entry.clone();
    rescanned.game_id = again.id.clone();
    assert_eq!(fx.import(&again, rescanned, T0 + 2 * MIN).await, id);
    // A metadata update rewrites the game.
    let mut updated = fx.store.game(&id).await.unwrap().unwrap();
    updated.description = "Climb.".into();
    fx.store.upsert_game(&updated).await.unwrap();
    // Replays: passes over the journal and the events that start them.
    assert!(!fx.feed.ingest_additions().await.unwrap());
    assert!(
        fx.feed
            .on_event(&Event::LibraryGameAdded { game_id: id })
            .await
    );
    assert!(!fx.feed.ingest_additions().await.unwrap());

    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].count, items[0].version), (1, item.version));
    assert!(!items[0].unread);
}

#[tokio::test]
async fn batches_and_unseen_additions_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    {
        let fx = Fixture::with_store(LibraryStore::open(&path).await.unwrap()).await;
        fx.add("A", "steam", T0).await;
        fx.at(T0 + MIN);
        fx.feed.ingest_additions().await.unwrap();
        // Imported, then the app stopped before the feed saw it.
        fx.add("B", "gog", T0 + 2 * MIN).await;
        fx.store.close().await;
    }
    let fx = Fixture::with_store(LibraryStore::open(&path).await.unwrap()).await;
    fx.at(T0 + 3 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    fx.add("C", "gog", T0 + 4 * MIN).await;
    fx.at(T0 + 5 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 3);
    assert_eq!(items[0].body, "GOG 2 · Steam 1");
    fx.store.close().await;
}

#[tokio::test]
async fn concurrent_passes_count_each_addition_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::open(&dir.path().join("library.db"))
        .await
        .unwrap();
    let fx = Fixture::with_store(store.clone()).await;
    // A second service over the same database: no shared lock.
    let other = Notifications::with_clock(store.clone(), Arc::new(|| T0 + 10 * MIN));
    for i in 0..40 {
        fx.add(&format!("Game {i}"), "steam", T0 + i).await;
    }
    fx.at(T0 + 10 * MIN);
    let (a, b) = tokio::join!(fx.feed.ingest_additions(), other.ingest_additions());
    a.unwrap();
    b.unwrap();
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 40);
    store.close().await;
}

#[tokio::test]
async fn merged_and_removed_games_are_not_counted_and_open_gracefully() {
    let fx = Fixture::new().await;
    let a = fx.add("Metroid", "romm:home", T0).await;
    let b = fx.add("Metroid", "steam", T0 + MIN).await;
    // Merged before the feed saw it: one game was added.
    fx.store.merge_games(&a, &b).await.unwrap();
    fx.at(T0 + 2 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    assert_eq!(fx.items().await[0].count, 1);

    let c = fx.add("Metroid Prime", "gog", T0 + 3 * MIN).await;
    let d = fx.add("Zelda", "gog", T0 + 4 * MIN).await;
    fx.at(T0 + 5 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let item = fx.items().await.remove(0);
    assert_eq!(item.count, 3);
    // Seen, then merged into another game and removed.
    fx.store.merge_games(&a, &c).await.unwrap();
    fx.store.delete_game(&d).await.unwrap();
    let members = fx.feed.members(&item.id, 0, 10).await.unwrap().items;
    let states: Vec<_> = members
        .iter()
        .map(|m| (m.label.as_str(), m.state, m.game_id.as_deref()))
        .collect();
    assert_eq!(
        states,
        vec![
            ("Metroid", MemberState::Present, Some(a.as_str())),
            ("Metroid Prime", MemberState::Merged, Some(a.as_str())),
            ("Zelda", MemberState::Removed, None),
        ]
    );
    assert_eq!(members[0].source.as_deref(), Some("RomM"));
}

#[tokio::test]
async fn separating_an_entry_is_not_an_addition() {
    let fx = Fixture::new().await;
    let game = Game::new("Doom");
    let first = GameEntry::new(&game.id, SourceId::new("steam"), "2280");
    let second = GameEntry::new(&game.id, SourceId::new("gog"), "doom");
    fx.store
        .import_group(&game, &[first, second.clone()], None)
        .await
        .unwrap();
    fx.store.set_addition_time(&game.id, T0).await.unwrap();
    fx.at(T0 + MIN);
    fx.feed.ingest_additions().await.unwrap();
    let entry = fx.store.entries_for_source("gog").await.unwrap().remove(0);
    fx.store.separate_entry(&entry.id, &[], &[]).await.unwrap();
    assert!(!fx.feed.ingest_additions().await.unwrap());
    assert_eq!(fx.items().await[0].count, 1);
}

#[tokio::test]
async fn a_game_without_entries_waits_briefly_for_its_source() {
    let fx = Fixture::new().await;
    let game = fx.store.upsert_game(&Game::new("Manual")).await.unwrap();
    fx.store.set_addition_time(&game.id, T0).await.unwrap();
    fx.at(T0 + 5 * 1000);
    assert!(!fx.feed.ingest_additions().await.unwrap());
    fx.at(T0 + ENTRY_GRACE_MS);
    assert!(fx.feed.ingest_additions().await.unwrap());
    let item = fx.items().await.remove(0);
    assert_eq!(
        (item.title.as_str(), item.body.as_str()),
        ("Manual added", "Other 1")
    );
}

#[tokio::test]
async fn read_state_acknowledges_only_what_was_shown() {
    let fx = Fixture::new().await;
    fx.add("A", "steam", T0).await;
    fx.at(T0 + MIN);
    fx.feed.ingest_additions().await.unwrap();
    let shown = fx.feed.list(false, 0, 10).await.unwrap();
    let item = shown.items[0].clone();

    // An addition arrives between showing the item and marking it read.
    fx.add("B", "steam", T0 + 2 * MIN).await;
    fx.at(T0 + 3 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    fx.feed.mark_read(&item.id, item.version).await.unwrap();
    let current = fx.items().await.remove(0);
    assert!(current.unread);
    assert_eq!(current.count, 2);
    // Mark all read at the old revision leaves it unread too.
    assert_eq!(fx.feed.mark_all_read(shown.revision).await.unwrap(), 0);
    assert_eq!(fx.feed.unread().await.unwrap(), 1);

    assert!(
        fx.feed
            .mark_read(&current.id, current.version)
            .await
            .unwrap()
    );
    assert_eq!(fx.feed.unread().await.unwrap(), 0);
    // A stale mark never moves read state back; unread does, on purpose.
    assert!(!fx.feed.mark_read(&current.id, item.version).await.unwrap());
    assert!(fx.feed.mark_unread(&current.id).await.unwrap());
    assert_eq!(fx.feed.list(true, 0, 10).await.unwrap().total, 1);

    // New additions to a read batch make it unread again.
    let revision = fx.feed.list(false, 0, 10).await.unwrap().revision;
    assert_eq!(fx.feed.mark_all_read(revision).await.unwrap(), 1);
    fx.add("C", "steam", T0 + 4 * MIN).await;
    fx.at(T0 + 5 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let page = fx.feed.list(true, 0, 10).await.unwrap();
    assert_eq!((page.total, page.unread, page.items[0].count), (1, 1, 3));
}

fn job(id: &str, status: JobStatus, error: Option<&str>) -> Event {
    Event::InstallJobChanged(Box::new(JobSnapshot {
        id: id.into(),
        revision: 0,
        kind: JobKind::Update,
        status,
        title: "Celeste 1.4".into(),
        input: String::new(),
        root_id: None,
        phase: None,
        progress: None,
        warnings: vec![],
        question: None,
        game_id: Some("game".into()),
        error: error.map(str::to_owned),
        candidates: vec![],
        selected_candidate_index: None,
        target_dir: None,
        dependencies: vec![],
        verification: None,
        failure: None,
        created_at_ms: T0,
        updated_at_ms: T0,
    }))
}

#[tokio::test]
async fn a_failed_job_is_one_item_until_it_works() {
    let fx = Fixture::new().await;
    fx.feed
        .on_event(&job("j1", JobStatus::Failed, Some("disk full")))
        .await;
    let first = fx.items().await.remove(0);
    assert_eq!(first.category, NotificationCategory::JobFailed);
    assert_eq!(first.title, "Celeste 1.4 couldn't update");
    assert_eq!(
        first.target,
        NotificationTarget::Game {
            game_id: "game".into()
        }
    );
    fx.feed.mark_read(&first.id, first.version).await.unwrap();
    let first = fx.items().await.remove(0);
    // The same failure again, and progress, change nothing.
    fx.feed
        .on_event(&job("j1", JobStatus::Failed, Some("disk full")))
        .await;
    fx.feed
        .on_event(&job("j1", JobStatus::Downloading, None))
        .await;
    assert_eq!(fx.items().await, vec![first.clone()]);
    // Another way to fail is news.
    fx.feed
        .on_event(&job("j1", JobStatus::Failed, Some("checksum mismatch")))
        .await;
    let second = fx.items().await.remove(0);
    assert!(second.unread && second.body == "checksum mismatch");
    // The retry worked: resolved and read.
    fx.feed
        .on_event(&job("j1", JobStatus::Completed, None))
        .await;
    let done = fx.items().await.remove(0);
    assert!(done.resolved && !done.unread);
    assert_eq!(fx.items().await.len(), 1);
}

#[tokio::test]
async fn a_disabled_plugin_is_noted_once() {
    let fx = Fixture::new().await;
    let event = Event::PluginDisabled {
        plugin: "dev.hltb".into(),
        reason: "It crashed 3 times.".into(),
    };
    fx.feed.on_event(&event).await;
    fx.feed.on_event(&event).await;
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].severity, Severity::Warning);
    assert_eq!(
        items[0].target,
        NotificationTarget::Settings {
            tab: "plugins".into(),
            section: None
        }
    );
}

#[tokio::test]
async fn a_failed_script_after_a_game_is_noted_once_per_session_and_phase() {
    use crate::scripts::{ScriptPhase, ScriptReport, ScriptRun, ScriptScope, ScriptStatus};
    let fx = Fixture::new().await;
    let run = |status| ScriptRun {
        script_id: "s".into(),
        name: "Unmount".into(),
        scope: ScriptScope::Global,
        phase: ScriptPhase::AfterExit,
        status,
        exit_code: Some(2),
        stdout: String::new(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 5,
        error: None,
    };
    let report = |phase, status| {
        Event::LifecycleScriptsRan(Box::new(ScriptReport {
            game_id: "g1".into(),
            game_title: "Portal".into(),
            session_id: Some("session".into()),
            phase: Some(phase),
            runs: vec![run(status)],
        }))
    };
    // Succeeded scripts and before-launch failures (the launch says why)
    // add nothing.
    fx.feed
        .on_event(&report(ScriptPhase::AfterExit, ScriptStatus::Succeeded))
        .await;
    fx.feed
        .on_event(&report(ScriptPhase::BeforeLaunch, ScriptStatus::Failed))
        .await;
    assert!(fx.items().await.is_empty());
    fx.feed
        .on_event(&report(ScriptPhase::AfterExit, ScriptStatus::Failed))
        .await;
    fx.feed
        .on_event(&report(ScriptPhase::AfterExit, ScriptStatus::Failed))
        .await;
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].category, NotificationCategory::ScriptFailed);
    assert_eq!(items[0].title, "A cleanup script after Portal failed");
    assert_eq!(items[0].body, "“Unmount” failed (exit code 2)");
    assert_eq!(
        items[0].target,
        NotificationTarget::Game {
            game_id: "g1".into()
        }
    );
}

#[tokio::test]
async fn new_achievement_unlocks_are_noted_once_each() {
    let fx = Fixture::new().await;
    let unlocks = |names: &[&str]| {
        Event::AchievementsUnlocked(Box::new(crate::achievements::NewUnlocks {
            game_id: "g1".into(),
            game_title: "Portal".into(),
            set_id: "set".into(),
            provider_name: "Steam".into(),
            account_name: "Ada".into(),
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }))
    };
    fx.feed.on_event(&unlocks(&[])).await;
    assert!(fx.items().await.is_empty());
    fx.feed.on_event(&unlocks(&["Lab Rat"])).await;
    // The same unlock announced again changes nothing.
    fx.feed.on_event(&unlocks(&["Lab Rat"])).await;
    fx.feed
        .on_event(&unlocks(&["Fratricide", "Partygoer"]))
        .await;
    let items = fx.items().await;
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| item.category == NotificationCategory::AchievementsUnlocked)
    );
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    assert!(titles.contains(&"Achievement unlocked in Portal"));
    assert!(titles.contains(&"2 achievements unlocked in Portal"));
    assert!(
        items
            .iter()
            .any(|item| item.body == "Fratricide, Partygoer (Steam, Ada)")
    );
    assert_eq!(
        items[0].target,
        NotificationTarget::Game {
            game_id: "g1".into()
        }
    );
}

fn update(key: &str) -> MemberRow {
    MemberRow {
        key: key.into(),
        kind: MemberKind::EmulatorUpdate,
        game_id: None,
        entry_id: None,
        label: key.into(),
    }
}

fn updates_notice(count: usize) -> Notice {
    Notice {
        category: NotificationCategory::UpdatesAvailable,
        severity: Severity::Info,
        key: "updates.available".into(),
        title: crate::i18n::Message::new(None, "{n} updates available", None, None)
            .arg("n", count.to_string()),
        body: crate::i18n::Text::Plain(String::new()),
        target: NotificationTarget::Members,
    }
}

#[tokio::test]
async fn updates_on_offer_are_grouped_and_repeated_checks_change_nothing() {
    let fx = Fixture::new().await;
    let sync = |members: Vec<MemberRow>| {
        let store = fx.store.clone();
        async move {
            store
                .sync_notice_members(&updates_notice(members.len()), &members, T0)
                .await
                .unwrap()
        }
    };
    assert!(!sync(vec![]).await);
    assert!(sync(vec![update("emulator:dolphin@2610")]).await);
    let item = fx.items().await.remove(0);
    fx.feed.mark_read(&item.id, item.version).await.unwrap();
    assert!(!sync(vec![update("emulator:dolphin@2610")]).await);
    assert!(!fx.items().await[0].unread);
    // Another update arrives: unread again.
    assert!(sync(vec![update("emulator:dolphin@2610"), update("game:e1@1.2")]).await);
    let item = fx.items().await.remove(0);
    assert!(item.unread && item.count == 2);
    fx.feed.mark_read(&item.id, item.version).await.unwrap();
    // One installed: fewer, still read.
    assert!(sync(vec![update("game:e1@1.2")]).await);
    let item = fx.items().await.remove(0);
    assert!(!item.unread && item.count == 1);
    // None left: resolved.
    assert!(sync(vec![]).await);
    assert!(fx.items().await[0].resolved);
}

#[tokio::test]
async fn app_updates_are_noted_once_per_version() {
    let fx = Fixture::new().await;
    assert!(fx.feed.set_app_update(Some("0.2.0")).await.unwrap());
    assert!(!fx.feed.set_app_update(Some("0.2.0")).await.unwrap());
    assert!(fx.feed.set_app_update(None).await.unwrap());
    assert!(fx.items().await[0].resolved);
    assert_eq!(fx.feed.unread().await.unwrap(), 0);
}

#[tokio::test]
async fn cleanup_drops_old_read_items_and_never_unread_ones() {
    let fx = Fixture::new().await;
    fx.add("Old and read", "steam", T0).await;
    fx.at(T0 + MIN);
    fx.feed.ingest_additions().await.unwrap();
    fx.add("Old and unread", "steam", T0 + 2 * 60 * MIN).await;
    fx.at(T0 + 2 * 60 * MIN + MIN);
    fx.feed.ingest_additions().await.unwrap();
    let items = fx.items().await;
    let read = items
        .iter()
        .find(|i| i.title == "Old and read added")
        .unwrap();
    fx.feed.mark_read(&read.id, read.version).await.unwrap();

    fx.at(T0 + KEEP_READ_MS);
    assert_eq!(fx.feed.prune().await.unwrap(), 0);
    fx.at(T0 + KEEP_READ_MS + 2 * MIN);
    assert_eq!(fx.feed.prune().await.unwrap(), 1);
    fx.at(T0 + 10 * KEEP_READ_MS);
    assert_eq!(fx.feed.prune().await.unwrap(), 0);
    let left = fx.items().await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].title, "Old and unread added");
    // The pruned batch's games are still in the library: never counted again.
    assert!(!fx.feed.ingest_additions().await.unwrap());
}

#[tokio::test]
async fn changes_tick_the_subscription() {
    let fx = Fixture::new().await;
    let mut changes = fx.feed.subscribe();
    fx.add("A", "steam", T0).await;
    fx.at(T0 + MIN);
    fx.feed.ingest_additions().await.unwrap();
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    fx.feed.ingest_additions().await.unwrap();
    assert!(!changes.has_changed().unwrap());
}

/// Swedish for the messages the next test reads.
const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "{game} couldn't update"
msgstr "{game} kunde inte uppdateras"

msgid "The download failed."
msgstr "Nedladdningen misslyckades."

msgid "{n} game added"
msgid_plural "{n} games added"
msgstr[0] "{n} spel lades till"
msgstr[1] "{n} spel lades till"

msgctxt "library source"
msgid "Other"
msgstr "Övrigt"

msgctxt "games added from a library source"
msgid "{source} {count}"
msgstr "{source} {count}"

msgctxt "list of library sources"
msgid "{first} · {rest}"
msgstr "{first} · {rest}"
"#;

#[tokio::test]
async fn a_language_switch_changes_earlier_notifications() {
    use caskade_installer::failure::{Failure, FailureKind, FailureStage};
    let fx = Fixture::new().await;
    let mut failed = job("j1", JobStatus::Failed, Some("connection reset"));
    if let Event::InstallJobChanged(snapshot) = &mut failed {
        snapshot.failure = Some(Failure::new(
            FailureKind::Download,
            FailureStage::Download,
            None,
            true,
            "connection reset",
        ));
    }
    fx.feed.on_event(&failed).await;
    fx.add("Manual", "other", T0).await;
    fx.add("Celeste", "steam", T0 + MIN).await;
    fx.at(T0 + 2 * MIN);
    fx.feed.ingest_additions().await.unwrap();
    let find = |items: &[Notification], category: NotificationCategory| {
        items
            .iter()
            .find(|item| item.category == category)
            .cloned()
            .unwrap()
    };

    // Stored while the language was English...
    let english = fx.items().await;
    let job_failed = find(&english, NotificationCategory::JobFailed);
    assert_eq!(job_failed.title, "Celeste 1.4 couldn't update");
    assert!(
        job_failed.body.starts_with("The download failed."),
        "{job_failed:?}"
    );
    let batch = find(&english, NotificationCategory::GamesAdded);
    assert_eq!(
        (batch.title.as_str(), batch.body.as_str()),
        ("2 games added", "Other 1 · Steam 1")
    );

    // ...and read after a switch to Swedish.
    let _sv = crate::i18n::scope_catalog("sv", SV);
    let swedish = fx.items().await;
    let job_failed = find(&swedish, NotificationCategory::JobFailed);
    assert_eq!(job_failed.title, "Celeste 1.4 kunde inte uppdateras");
    assert!(
        job_failed.body.starts_with("Nedladdningen misslyckades."),
        "{job_failed:?}"
    );
    let batch = find(&swedish, NotificationCategory::GamesAdded);
    assert_eq!(batch.title, "2 spel lades till");
    assert_eq!(batch.body, "Övrigt 1 · Steam 1");
    assert_eq!(batch.sources[0].source, "Övrigt");
    let members = fx.feed.members(&batch.id, 0, 10).await.unwrap().items;
    assert_eq!(members[0].source.as_deref(), Some("Övrigt"));
    // Reading changed nothing: the switch isn't news.
    assert_eq!(
        swedish
            .iter()
            .map(|i| (i.version, i.unread))
            .collect::<Vec<_>>(),
        english
            .iter()
            .map(|i| (i.version, i.unread))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sources_are_named_for_people_even_when_stored_as_ids() {
    assert_eq!(source_label("desktop"), "Desktop entries");
    assert_eq!(source_label("steam.shortcuts"), "Steam");
    // A feed stored before `desktop` had a name.
    assert_eq!(source_text("desktop").render(), "Desktop entries");
    // A new batch's body names it.
    let body = sources_body(&[SourceCount {
        source: source_name("desktop"),
        count: 5,
    }]);
    assert_eq!(body.render(), "Desktop entries 5");
}

#[tokio::test]
async fn someone_receiving_a_share_is_noted_once_per_share_and_person() {
    let fx = Fixture::new().await;
    let received = |endpoint: &str, recipient: &str| Event::ShareReceived {
        share_id: "s1".into(),
        title: "Celeste".into(),
        recipient: recipient.into(),
        endpoint: endpoint.into(),
    };
    fx.feed.on_event(&received("e1", "Sam's PC")).await;
    // The same person again (a resumed transfer) changes nothing.
    fx.feed.on_event(&received("e1", "Sam's PC")).await;
    fx.feed.on_event(&received("e2", "Kim")).await;
    let items = fx.items().await;
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| item.category == NotificationCategory::ShareReceived)
    );
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    assert!(titles.contains(&"Sam's PC received Celeste"), "{titles:?}");
    assert!(titles.contains(&"Kim received Celeste"));
    assert_eq!(
        items[0].target,
        NotificationTarget::Settings {
            tab: "devices".into(),
            section: None
        }
    );
}

#[tokio::test]
async fn a_shared_game_arriving_opens_its_page() {
    let fx = Fixture::new().await;
    fx.feed
        .on_event(&Event::ShareArrived {
            job: "j1".into(),
            title: "Celeste".into(),
            sharer: "Alex's Desk".into(),
            game_id: Some("g1".into()),
        })
        .await;
    let items = fx.items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].category, NotificationCategory::ShareArrived);
    assert_eq!(items[0].title, "Celeste arrived from Alex's Desk");
    assert_eq!(
        items[0].target,
        NotificationTarget::Game {
            game_id: "g1".into()
        }
    );
}
