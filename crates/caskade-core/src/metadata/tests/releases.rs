//! Merged games, releases of one game, separated releases and RomM's games.

use super::*;

#[tokio::test]
async fn merged_games_keep_a_match_and_are_applied_again() {
    let fixture = Fixture::new().await;
    let keep = fixture.add("Portal 2", vec![]).await;
    let donor = fixture
        .add("Portal 2 (Steam)", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();

    fixture
        .store
        .merge_games(&keep.id, &donor.id)
        .await
        .unwrap();
    let moved = fixture.store.igdb_match(&keep.id).await.unwrap().unwrap();
    assert_eq!((moved.igdb_id, moved.pending), (Some(72), true));
    fixture.metadata.run().await.unwrap();
    let merged = fixture.game(&keep.id).await;
    assert_eq!(merged.description, "Portal 2 from IGDB.");
    let covers = fixture
        .artwork(&keep.id)
        .await
        .into_iter()
        .filter(|(kind, _)| *kind == ArtworkKind::Cover)
        .count();
    assert_eq!(covers, 1);
}

#[tokio::test]
async fn a_match_by_hand_that_merges_the_game_answers_for_the_merged_game() {
    let fixture = Fixture::new().await;
    let release = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.set(&release.id, Some(72)).await.unwrap();
    let added = fixture.add("Portal Two", vec![]).await;
    fixture.metadata.run().await.unwrap();

    // Both chosen by hand: the older one stays and the added one goes into it.
    let set = fixture.metadata.set(&added.id, Some(72)).await.unwrap();
    assert_eq!(set.game_id, release.id);
    assert_eq!((set.igdb_id, set.source), (Some(72), MatchSource::Manual));
    assert!(fixture.store.game(&added.id).await.unwrap().is_none());
    assert_eq!(
        fixture
            .store
            .entries_for_game(&release.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn catalog_games_in_the_library_are_owned_and_maybe_installed() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    let owned = fixture
        .store
        .igdb_owned(&[72, 1942, u64::MAX])
        .await
        .unwrap();
    assert_eq!(
        owned,
        [crate::db::IgdbOwned {
            igdb_id: 72,
            game_id: portal.id.clone(),
            installed: false,
        }]
    );
    fixture
        .store
        .set_game_hidden(&portal.id, true)
        .await
        .unwrap();
    assert!(fixture.store.igdb_owned(&[72]).await.unwrap().is_empty());
}

#[tokio::test]
async fn romm_games_match_by_romms_id_and_keep_romms_values_and_artwork() {
    let fixture = Fixture::new().await;
    let mut game = Game::new("Portal 2");
    game.description = "From RomM.".into();
    let game = fixture.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, crate::romm::source_id("home"), "10");
    // Its Steam id would match too, but RomM's own match comes first.
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    let entry = fixture.store.upsert_entry(&entry).await.unwrap();
    let rom = serde_json::from_value(json!({
        "id": 10, "platform_id": 1, "fs_name": "portal.iso", "igdb_id": 72
    }))
    .unwrap();
    fixture
        .store
        .save_romm_rom(&crate::romm::RommRom {
            server_id: "home".into(),
            rom_id: 10,
            entry_id: entry.id.clone(),
            game_id: game.id.clone(),
            platform_id: "sony_playstation".into(),
            rom,
            synced_target: crate::emulation::EmulatorTarget::PlatformDefault,
            pinned: false,
            orphaned: false,
            cache: None,
            cover_unavailable: None,
        })
        .await
        .unwrap();
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let image = cache.import_bytes(png()).await.unwrap();
    for kind in [ArtworkKind::Cover, ArtworkKind::Screenshot] {
        fixture
            .store
            .upsert_artwork(&image.record(&game.id, kind, "romm:home"))
            .await
            .unwrap();
    }

    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 0);
    let found = fixture.metadata.game(&game.id).await.unwrap().unwrap();
    assert_eq!((found.igdb_id, found.source), (Some(72), MatchSource::Romm));
    // IGDB fills only what RomM left empty, and adds a hero and an icon.
    fixture.metadata.page_images(&game.id).await.unwrap();
    let filled = fixture.game(&game.id).await;
    assert_eq!(filled.description, "From RomM.");
    assert_eq!(filled.genres, ["Puzzle"]);
    assert_eq!(
        fixture.artwork(&game.id).await,
        [
            (ArtworkKind::Hero, "igdb:ar72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
            (ArtworkKind::Cover, "romm:home".to_owned()),
            (ArtworkKind::Screenshot, "romm:home".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_romm_row_without_an_id_is_matched_again() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    // Saved by a version that left RomM games alone.
    let mut row = IgdbMatch::new(&game.id, None, MatchSource::Romm, "");
    row.pending = false;
    fixture.store.save_igdb_match(&row).await.unwrap();

    fixture.metadata.run().await.unwrap();
    let found = fixture.metadata.game(&game.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source),
        (Some(72), MatchSource::Store)
    );
}

impl Fixture {
    /// A ROM on `platform` (the default catalog has no names, so the id is
    /// its name) that RomM matched to `igdb_id`.
    async fn add_rom(&self, title: &str, platform: &str, igdb_id: u64) -> Game {
        let mut game = Game::new(title);
        game.platforms = vec![platform.to_owned()];
        let game = self.store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(
            &game.id,
            SourceId::new("roms"),
            format!("{platform}/{title}"),
        );
        entry.state = caskade_discovery::InstallState::Installed;
        entry.launch = Some(crate::model::LaunchSpec::Emulator {
            platform_id: platform.to_owned(),
            target: crate::emulation::EmulatorTarget::PlatformDefault,
            roms: vec![],
            args: crate::emulation::ArgsOverride::Default,
        });
        self.store.upsert_entry(&entry).await.unwrap();
        let row = IgdbMatch::new(&game.id, Some(igdb_id), MatchSource::Romm, "");
        self.store.save_igdb_match(&row).await.unwrap();
        game
    }

    async fn entries(&self, game_id: &str) -> Vec<String> {
        self.store
            .entries_for_game(game_id)
            .await
            .unwrap()
            .into_iter()
            .map(|entry| entry.external_id)
            .collect()
    }
}

#[tokio::test]
async fn releases_of_one_igdb_game_become_one_game_with_an_entry_each() {
    let fixture = Fixture::new().await;
    let snes = fixture.add_rom("Portal 2", "snes", 72).await;
    let genesis = fixture.add_rom("Portal 2", "genesis", 72).await;
    let steam = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let hidden = fixture.add_rom("Portal 2", "gb", 72).await;
    fixture
        .store
        .set_game_hidden(&hidden.id, true)
        .await
        .unwrap();
    let other = fixture.add_rom("Worms", "snes", 9331).await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(report.merged, 2);
    // The oldest stays, with every release's entry and platform.
    assert!(fixture.store.game(&genesis.id).await.unwrap().is_none());
    assert!(fixture.store.game(&steam.id).await.unwrap().is_none());
    assert_eq!(
        fixture.entries(&snes.id).await,
        ["genesis/Portal 2", "snes/Portal 2", "Portal 2"]
    );
    let game = fixture.game(&snes.id).await;
    assert_eq!(game.platforms, ["genesis", "snes"]);
    assert_eq!(game.description, "Portal 2 from IGDB.");
    // A hidden game stays hidden, apart; other games stay as they were.
    assert_eq!(fixture.entries(&hidden.id).await, ["gb/Portal 2"]);
    assert_eq!(fixture.entries(&other.id).await, ["snes/Worms"]);
    assert_eq!(fixture.metadata.run().await.unwrap().merged, 0);
}

#[tokio::test]
async fn a_separated_release_stays_apart() {
    let fixture = Fixture::new().await;
    let snes = fixture.add_rom("Portal 2", "snes", 72).await;
    let mut pc = fixture.add_rom("Portal 2", "genesis", 72).await;
    pc.platforms = vec!["PC".into()];
    fixture.store.upsert_game(&pc).await.unwrap();
    let mut steam = GameEntry::new(&pc.id, SourceId::new("steam"), "620");
    steam.state = caskade_discovery::InstallState::Installed;
    fixture.store.upsert_entry(&steam).await.unwrap();
    fixture.metadata.run().await.unwrap();
    // The game with the most entries is the one kept.
    assert!(fixture.store.game(&snes.id).await.unwrap().is_none());
    assert_eq!(fixture.game(&pc.id).await.platforms, ["PC", "snes"]);
    let genesis = fixture
        .store
        .entries_for_game(&pc.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.external_id == "genesis/Portal 2")
        .unwrap();

    let apart = fixture.metadata.separate(&genesis.id).await.unwrap();
    assert_eq!(fixture.entries(&apart).await, ["genesis/Portal 2"]);
    let separated = fixture.game(&apart).await;
    assert_eq!(separated.platforms, ["genesis"]);
    assert_eq!(separated.description, "Portal 2 from IGDB.");
    assert!(!fixture.artwork(&apart).await.is_empty());
    assert_eq!(
        fixture
            .metadata
            .game(&apart)
            .await
            .unwrap()
            .unwrap()
            .igdb_id,
        Some(72)
    );
    // The PC entry keeps the platforms that aren't an emulated entry's.
    assert_eq!(fixture.game(&pc.id).await.platforms, ["PC", "snes"]);
    assert_eq!(fixture.metadata.run().await.unwrap().merged, 0);
    assert_eq!(fixture.entries(&apart).await, ["genesis/Portal 2"]);
    // A game's last entry has nowhere to go.
    assert!(fixture.metadata.separate(&genesis.id).await.is_err());
}

impl Fixture {
    /// A ROM on `platform` released on `released`, which RomM matched to
    /// `igdb_id` or (`None`) left without one.
    async fn dated_rom(
        &self,
        title: &str,
        platform: &str,
        released: &str,
        igdb_id: Option<u64>,
    ) -> Game {
        let game = match igdb_id {
            Some(id) => self.add_rom(title, platform, id).await,
            None => {
                let mut game = Game::new(title);
                game.platforms = vec![platform.to_owned()];
                let game = self.store.upsert_game(&game).await.unwrap();
                let mut entry = GameEntry::new(
                    &game.id,
                    SourceId::new("roms"),
                    format!("{platform}/{title}"),
                );
                entry.launch = Some(crate::model::LaunchSpec::Emulator {
                    platform_id: platform.to_owned(),
                    target: crate::emulation::EmulatorTarget::PlatformDefault,
                    roms: vec![],
                    args: crate::emulation::ArgsOverride::Default,
                });
                self.store.upsert_entry(&entry).await.unwrap();
                game
            }
        };
        let mut game = self.game(&game.id).await;
        game.release_date = Some(released.to_owned());
        self.store.upsert_game(&game).await.unwrap()
    }
}

#[tokio::test]
async fn a_release_romm_left_unmatched_joins_its_game_offline_and_stays_joined() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    // RomM matched the N64 ROM; the PlayStation one has no IGDB id.
    let n64 = fixture
        .dated_rom("A Bug's Life", "n64", "1998-11-01", Some(2847))
        .await;
    let psx = fixture
        .dated_rom("A Bug's Life", "psx", "1998-11-18", None)
        .await;
    // A game of the same name from another era, and a sequel.
    let remake = fixture
        .dated_rom("A Bug's Life", "ps4", "2024-01-01", None)
        .await;
    let sequel = fixture
        .dated_rom("A Bug's Life 2", "psx", "1999-06-01", None)
        .await;

    let report = fixture.metadata.run().await.unwrap();
    // The server is down, but the merge needs only the library.
    assert!(report.unreachable);
    assert_eq!((report.merged, report.none), (1, 3));
    assert!(fixture.store.game(&psx.id).await.unwrap().is_none());
    let mut entries = fixture.entries(&n64.id).await;
    entries.sort();
    assert_eq!(entries, ["n64/A Bug's Life", "psx/A Bug's Life"]);
    let row = fixture.store.igdb_match(&n64.id).await.unwrap().unwrap();
    // Applied once the server answers again.
    assert_eq!((row.igdb_id, row.pending), (Some(2847), true));
    assert_eq!(fixture.entries(&remake.id).await, ["ps4/A Bug's Life"]);
    assert_eq!(fixture.entries(&sequel.id).await, ["psx/A Bug's Life 2"]);

    // Later runs (and restarts) leave it one game.
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(report.merged, 0);
    let mut entries = fixture.entries(&n64.id).await;
    entries.sort();
    assert_eq!(entries, ["n64/A Bug's Life", "psx/A Bug's Life"]);
}
