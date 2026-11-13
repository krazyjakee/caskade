//! Syncing: imports, resyncs, covers and RomM's values, ROMs lost on the
//! server's disk or gone at once, broken listings, stalls and paging.

use super::*;

#[tokio::test]
async fn a_platform_without_a_shipped_emulator_or_extensions_still_imports() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    roms(
        &server,
        "3",
        vec![json!({
            "id": 30, "platform_id": 3, "fs_name": "Mine Storm.vec", "name": "Mine Storm"
        })],
    )
    .await;
    let platforms = format!(
        r#"{PLATFORMS}
[[platform]]
id = "vectrex"
name = "GCE Vectrex"
short = "Vectrex"
ids = {{ romm = ["vectrex"] }}
"#
    );
    let fixture = Fixture::build_with_platforms(server, "secret", None, &platforms).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.added, 4);
    assert!(report.unmatched_platforms.is_empty());
    assert!(report.warnings.is_empty());
    let record = fixture.record(30).await;
    assert_eq!(record.platform_id, "vectrex");
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(emulated(&entry.launch).0, "vectrex");
    let view = fixture
        .emulation
        .platforms()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.platform.id == "vectrex")
        .unwrap();
    assert_eq!(view.rom_count, 1);
    assert!(view.compatible.is_empty());
    assert!(view.settings.default.is_none());
    let again = fixture.romm.sync("home").await.unwrap();
    assert_eq!(again.added, 0);
    assert_eq!(again.unchanged, 4);
}

#[tokio::test]
async fn sync_imports_the_selected_platforms_and_a_resync_keeps_user_fields() {
    let fixture = Fixture::new().await;
    let report = fixture
        .romm
        .test_connection(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    assert_eq!(report.version.as_deref(), Some("4.4.0"));
    let suggested: Vec<_> = report
        .platforms
        .iter()
        .map(|found| (found.platform.slug.as_str(), found.suggested.as_deref()))
        .collect();
    assert_eq!(
        suggested,
        [
            ("snes", Some("nintendo_snes")),
            ("ps", Some("sony_playstation")),
            ("vectrex", None)
        ]
    );

    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.added, report.updated, report.removed), (3, 0, 0));
    assert_eq!(report.unmatched_platforms, ["Vectrex"]);

    // Searchable, installed-looking and filterable like local games.
    let found = fixture
        .store
        .list_games(&ListGames {
            query: Some("Chrono".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.items.len(), 1);
    let chrono = &found.items[0];
    assert!(chrono.installed);
    assert_eq!(chrono.platforms, ["Super Nintendo Entertainment System"]);
    assert_eq!(chrono.genres, ["RPG"]);
    assert!(chrono.cover.is_some(), "the server's cover is imported");
    for filter in [GameFilter::Installed, GameFilter::Emulated] {
        let page = fixture
            .store
            .list_games(&ListGames {
                filter,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 3, "{filter:?}");
    }
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert_eq!(details.game.release_date.as_deref(), Some("1995-03-15"));
    assert_eq!(details.game.aliases, ["Chrono Trigger (JP)"]);
    // The sync brings the cover; the screenshots wait for the game's page.
    let kinds = |details: &caskade_core::model::GameDetails| -> Vec<ArtworkKind> {
        details.artwork.iter().map(|art| art.kind).collect()
    };
    assert_eq!(kinds(&details), [ArtworkKind::Cover]);
    assert!(fixture.romm.page_images(&chrono.id).await.unwrap());
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert_eq!(
        kinds(&details),
        [
            ArtworkKind::Cover,
            ArtworkKind::Screenshot,
            ArtworkKind::Screenshot
        ]
    );
    assert!(!fixture.romm.page_images(&chrono.id).await.unwrap());
    let entry = &details.entries[0];
    assert_eq!(entry.source_id.as_str(), "romm:home");
    assert_eq!(entry.external_id, "10");
    assert_eq!(entry.state, InstallState::Installed);
    let (platform, target, roms) = emulated(&entry.launch);
    assert_eq!(platform, "nintendo_snes");
    assert_eq!(*target, EmulatorTarget::PlatformDefault);
    assert!(roms[0].starts_with(fixture.cache()));
    // Not downloaded, but not "ROM missing" either: it downloads at launch.
    let game = fixture.emulation.game(&entry.id).await.unwrap();
    assert!(!matches!(game.readiness, Readiness::RomMissing { .. }));

    // The user's own fields.
    let mut game = details.game.clone();
    game.favorite = true;
    game.rating = Some(90);
    game.notes = "Play the DS version next".into();
    game.title = "Chrono Trigger (my copy)".into();
    game.aliases.push("CT".into());
    fixture.store.upsert_game(&game).await.unwrap();

    // The server changes one ROM and deletes another.
    fixture.server.reset().await;
    let mut changed = snes_rom(
        10,
        "Chrono Trigger",
        "Chrono Trigger (USA).sfc",
        CHRONO,
        "2026-02-01T00:00:00Z",
    );
    changed["summary"] = json!("A new summary.");
    changed["alternative_names"] = json!(["Kurono Torigā"]);
    mount(&fixture.server, vec![changed]).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(
        (
            report.added,
            report.updated,
            report.unchanged,
            report.removed
        ),
        (0, 1, 1, 1)
    );
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert!(details.game.favorite);
    assert_eq!(details.game.rating, Some(90));
    assert_eq!(details.game.notes, "Play the DS version next");
    assert_eq!(details.game.title, "Chrono Trigger (my copy)");
    assert_eq!(details.game.description, "A new summary.");
    // The server's old name goes; the user's stays.
    assert_eq!(details.game.aliases, ["CT", "Kurono Torigā"]);
    // Unchanged screenshots aren't imported twice.
    let screenshots = details
        .artwork
        .iter()
        .filter(|art| art.kind == ArtworkKind::Screenshot)
        .count();
    assert_eq!(screenshots, 2);
    let all = fixture
        .store
        .list_games(&ListGames::default())
        .await
        .unwrap();
    assert_eq!(all.total, 2, "Secret of Mana left with the server");
}

#[tokio::test]
async fn a_resync_retries_covers_that_failed_to_import() {
    let server = MockServer::builder().start().await;
    Mock::given(method("GET"))
        .and(path_regex("^/assets/romm/resources/"))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&server)
        .await;
    mount(&server, default_snes()).await;
    let fixture = Fixture::with_server(server, "secret").await;
    let chrono = async |fixture: &Fixture| {
        let found = fixture
            .store
            .list_games(&ListGames {
                query: Some("Chrono".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        found.items[0].clone()
    };

    let cover_requests = async |fixture: &Fixture| {
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().ends_with("/cover/big.png"))
            .count()
    };

    fixture.romm.sync("home").await.unwrap();
    let game = chrono(&fixture).await;
    assert!(game.cover.is_none());
    assert!(!fixture.romm.page_images(&game.id).await.unwrap());
    let details = fixture.store.get_game(&game.id).await.unwrap().unwrap();
    assert!(details.artwork.is_empty());
    // A cover the server refused isn't asked for at every sync.
    let asked = cover_requests(&fixture).await;
    assert!(asked > 0);
    fixture.romm.sync("home").await.unwrap();
    assert_eq!(cover_requests(&fixture).await, asked);

    // A week on, the server's covers work again, though the ROMs are
    // unchanged.
    let mut record = fixture.record(10).await;
    let failed = record
        .cover_unavailable
        .as_mut()
        .expect("the 404 is recorded");
    failed.at_ms -= caskade_core::romm::COVER_RETRY.as_millis() as i64;
    fixture.store.save_romm_rom(&record).await.unwrap();
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.added, 0);
    let game = chrono(&fixture).await;
    assert!(game.cover.is_some());
    assert!(fixture.record(10).await.cover_unavailable.is_none());
    // The page tries its screenshots again too.
    assert!(fixture.romm.page_images(&game.id).await.unwrap());
    let details = fixture.store.get_game(&game.id).await.unwrap().unwrap();
    assert_eq!(details.artwork.len(), 3, "the cover and both screenshots");
}

#[tokio::test]
async fn romms_values_and_artwork_replace_what_igdb_filled_in() {
    let server = MockServer::builder().start().await;
    let rom = |updated: &str| {
        snes_rom(
            10,
            "Chrono Trigger",
            "Chrono Trigger (USA).sfc",
            CHRONO,
            updated,
        )
    };
    // At first RomM has no summary or artwork for the ROM.
    let mut bare = rom("2026-01-01T00:00:00Z");
    bare["summary"] = Value::Null;
    bare["path_cover_large"] = Value::Null;
    bare["merged_screenshots"] = json!([]);
    mount(&server, vec![bare]).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();
    let id = fixture
        .store
        .list_games(&ListGames {
            query: Some("Chrono".into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .items[0]
        .id
        .clone();

    // IGDB fills the description in and adds its cover and a screenshot.
    let mut game = fixture.store.get_game(&id).await.unwrap().unwrap().game;
    game.description = "From IGDB.".into();
    fixture.store.upsert_game(&game).await.unwrap();
    fixture
        .store
        .save_igdb_match(&IgdbMatch {
            game_id: id.clone(),
            igdb_id: Some(1),
            source: MatchSource::Romm,
            pending: false,
            checked_at_ms: 0,
            dumps: String::new(),
            applied: Applied {
                description: Some("From IGDB.".into()),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let cache = ArtworkCache::new(fixture.temp.path().join("artwork")).unwrap();
    let image = cache.import_bytes(png()).await.unwrap();
    for (kind, source) in [
        (ArtworkKind::Cover, "igdb:co1"),
        (ArtworkKind::Screenshot, "igdb:sc1"),
    ] {
        fixture
            .store
            .upsert_artwork(&image.record(&id, kind, source))
            .await
            .unwrap();
    }

    // RomM gets a summary and artwork of its own.
    fixture.server.reset().await;
    mount(&fixture.server, vec![rom("2026-02-01T00:00:00Z")]).await;
    fixture.romm.sync("home").await.unwrap();
    fixture.romm.page_images(&id).await.unwrap();
    let details = fixture.store.get_game(&id).await.unwrap().unwrap();
    assert_eq!(details.game.description, "Chrono Trigger summary");
    let art: Vec<_> = details
        .artwork
        .iter()
        .map(|art| (art.kind, art.source.as_str()))
        .collect();
    assert_eq!(
        art,
        [
            (ArtworkKind::Cover, "romm:home"),
            (ArtworkKind::Screenshot, "romm:home"),
            (ArtworkKind::Screenshot, "romm:home"),
        ]
    );
}

/// The RomM host's NAS goes away and a rescan flags every ROM as missing:
/// the games stay, unavailable, and come back with the files.
#[tokio::test]
async fn roms_lost_on_the_servers_disk_stay_unavailable() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let missing = |mut rom: Value| {
        rom["missing_from_fs"] = json!(true);
        rom
    };
    // Every game, and a new ROM that was never in the library.
    let mut snes: Vec<Value> = default_snes().into_iter().map(missing).collect();
    snes.push(missing(snes_rom(
        12,
        "Lost",
        "Lost.sfc",
        CHRONO,
        "2026-01-01T00:00:00Z",
    )));
    fixture.server.reset().await;
    mount(&fixture.server, snes).await;
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![missing(ps1_rom())])))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.added, report.removed, report.orphaned), (0, 0, 0));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for rom in [10, 11, 20] {
        assert!(
            matches!(
                entry_state(&fixture, rom).await,
                InstallState::Broken { .. }
            ),
            "{rom}"
        );
    }
    let chrono = fixture.record(10).await;
    let status = fixture
        .romm
        .game_status(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "unavailable");
    assert!(status.message.unwrap().contains("can't find"));
    assert!(fixture.store.romm_rom("home", 12).await.unwrap().is_none());

    // The files are back.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    fixture.romm.sync("home").await.unwrap();
    for rom in [10, 11, 20] {
        assert_eq!(
            entry_state(&fixture, rom).await,
            InstallState::Installed,
            "{rom}"
        );
    }
    let status = fixture
        .romm
        .game_status(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");
}

/// A listing that loses a platform's games, or most of the server's, is
/// kept back with a note until a sync of the user's finds them gone again.
#[tokio::test]
async fn games_that_vanish_at_once_stay_until_a_second_sync_confirms() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let games = || async {
        fixture
            .store
            .list_games(&ListGames::default())
            .await
            .unwrap()
            .total
    };
    assert_eq!(games().await, 3);

    // Every SNES game gone in one listing: kept, with a note.
    fixture.server.reset().await;
    mount(&fixture.server, Vec::new()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert_eq!(games().await, 3);
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::Connected);
    assert!(status.error.unwrap().contains("Sync again"));
    // They are back: nothing is held any more.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert!(report.warnings.is_empty());
    assert!(server_status(&fixture).await.error.is_none());

    // Most of the server's games gone, though no platform is empty.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()[..1].to_vec()).await;
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(Vec::new())))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(games().await, 3);
    // The user syncs again and they are still gone: now they go.
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 2);
    assert!(report.warnings.is_empty());
    assert_eq!(games().await, 1);
    assert!(server_status(&fixture).await.error.is_none());
}

/// An answer without `items` is no page of ROMs: the sync fails and
/// removes nothing.
#[tokio::test]
async fn a_rom_listing_without_items_fails_the_sync() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "detail": "upgrading" })))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Server(_)), "{error}");
    assert!(fixture.store.romm_rom("home", 10).await.unwrap().is_some());
    assert!(fixture.store.romm_rom("home", 11).await.unwrap().is_some());
}

/// A server that stalls mid-request answered: the request fails, but the
/// server isn't offline and its games stay playable.
#[tokio::test]
async fn a_stalled_server_is_not_taken_for_an_offline_one() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    let fixture =
        Fixture::with_timeouts(server, Duration::from_millis(300), Duration::from_secs(1)).await;
    fixture.romm.sync("home").await.unwrap();

    // A ROM download that stalls.
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(MANA.to_vec())
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Interrupted(_)), "{error}");
    assert_ne!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    for rom in [10, 11, 20] {
        assert_eq!(
            entry_state(&fixture, rom).await,
            InstallState::Installed,
            "{rom}"
        );
    }
    let status = fixture
        .romm
        .game_status(&mana.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");

    // A listing that stalls fails the sync the same way.
    Mock::given(method("GET"))
        .and(path("/api/platforms"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([]))
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Interrupted(_)), "{error}");
    assert_ne!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    assert_eq!(entry_state(&fixture, 10).await, InstallState::Installed);

    // A refused connection is offline.
    // Port 1, which nothing in the tests binds: a port freed by a dropped
    // listener could go to another test's server and answer (#119).
    let closed = "127.0.0.1:1";
    fixture
        .romm
        .save_server(server_config(&format!("http://{closed}")), None)
        .await
        .unwrap();
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Unreachable(_)), "{error}");
    // It says why, not reqwest's "error sending request".
    assert!(
        !error.to_string().contains("error sending request"),
        "{error}"
    );
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    assert!(matches!(
        entry_state(&fixture, 10).await,
        InstallState::Broken { .. }
    ));
}

/// A server that ignores `offset` and sends no `total` repeats its first
/// page: paging stops at the first page with nothing new.
#[tokio::test]
async fn paging_stops_when_a_page_repeats() {
    let server = MockServer::builder().start().await;
    mount(&server, Vec::new()).await;
    let items: Vec<Value> = (1000..1250)
        .map(|id| {
            json!({
                "id": id, "platform_id": 1, "platform_slug": "snes",
                "fs_name": format!("Game {id}.sfc"), "name": format!("Game {id}"),
                "files": [file(&format!("Game {id}.sfc"), CHRONO)]
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": items })))
        .with_priority(1)
        .mount(&server)
        .await;
    let fixture = Fixture::with_server(server, "secret").await;
    let report = tokio::time::timeout(Duration::from_secs(60), fixture.romm.sync("home"))
        .await
        .expect("paging ends")
        .unwrap();
    assert_eq!(report.added, 251);
    let pages = fixture
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| {
            request.url.path() == "/api/roms"
                && request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "platform_ids" && value == "1")
        })
        .count();
    assert_eq!(pages, 2);
}
