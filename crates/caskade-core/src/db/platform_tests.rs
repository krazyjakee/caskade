//! The platform filter and the platform list (#148).
use super::LibraryStore;
use crate::model::{
    Game, GameEntry, GameFilter, InstallState, LaunchSpec, ListGames, PlatformSummary, PlaySession,
    SourceId, new_id,
};

#[tokio::test]
async fn games_filter_by_platform_and_the_library_lists_its_platforms() {
    let store = LibraryStore::in_memory().await.unwrap();
    for (title, platforms, hidden) in [
        ("Chrono", vec!["snes"], false),
        ("Doom", vec!["pc", "snes"], false),
        ("Quake", vec!["pc"], false),
        ("Secret", vec!["n64"], true),
    ] {
        let mut game = Game::new(title);
        game.platforms = platforms.into_iter().map(String::from).collect();
        game.hidden = hidden;
        store.upsert_game(&game).await.unwrap();
    }
    let titles = |platform: &str| {
        let store = store.clone();
        let platform = platform.to_owned();
        async move {
            let page = store
                .list_games(&ListGames {
                    platform: Some(platform),
                    filter: GameFilter::All,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert_eq!(page.total as usize, page.items.len());
            page.items
                .into_iter()
                .map(|game| game.title)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(titles("snes").await, ["Chrono", "Doom"]);
    assert_eq!(titles("pc").await, ["Doom", "Quake"]);
    assert!(titles("n64").await.is_empty(), "hidden games stay hidden");
    let names = store.library_platforms(GameFilter::All).await.unwrap();
    let names: Vec<_> = names.into_iter().map(|platform| platform.name).collect();
    assert_eq!(names, ["pc", "snes"]);
}

/// An emulated game on `platforms`, played for each `(start, duration)`.
async fn emulated(store: &LibraryStore, title: &str, platforms: &[&str], plays: &[(i64, u64)]) {
    let mut game = Game::new(title);
    game.platforms = platforms.iter().map(|name| name.to_string()).collect();
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: "fixture".into(),
        target: crate::emulation::EmulatorTarget::AskEmulator,
        roms: vec![crate::emulation::GameRom::absolute("/fixture/game.rom")],
        args: crate::emulation::ArgsOverride::Default,
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    for &(start, duration) in plays {
        let session = PlaySession::new(&game.id, &entry.id, start);
        store.start_session(&session).await.unwrap();
        store
            .finish_session(&session.id, start + duration as i64, duration)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn platforms_in_a_filter_carry_their_games_playtime_and_last_play() {
    let store = LibraryStore::in_memory().await.unwrap();
    emulated(&store, "Chrono", &["snes"], &[(1000, 50), (9000, 10)]).await;
    emulated(&store, "Doom", &["snes", "n64"], &[(5000, 200)]).await;
    emulated(&store, "Unplayed", &["nes"], &[]).await;
    let mut pc = Game::new("Quake");
    pc.platforms = vec!["pc".into()];
    store.upsert_game(&pc).await.unwrap();
    let summary = |name: &str, games, playtime_ms, last_played_at_ms| PlatformSummary {
        name: name.into(),
        games,
        playtime_ms,
        last_played_at_ms,
    };
    assert_eq!(
        store.library_platforms(GameFilter::Emulated).await.unwrap(),
        [
            summary("n64", 1, 200, Some(5000)),
            summary("nes", 1, 0, None),
            summary("snes", 2, 260, Some(9000)),
        ],
        "PC games aren't emulated, and Doom counts on both its platforms"
    );
    assert_eq!(
        store
            .library_platforms(GameFilter::All)
            .await
            .unwrap()
            .len(),
        4
    );
}
