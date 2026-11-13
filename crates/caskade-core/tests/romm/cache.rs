//! The download cache: launches that download, multi-file ROMs, resume and
//! cancel, mapping and overrides, the cache limit, BIOS files, zipped
//! hashes and eviction while offline.

use super::*;

#[tokio::test]
async fn launching_downloads_verifies_and_caches_then_plays_offline() {
    let fixture = Fixture::new().await;
    let profile = fixture.emulator("snes").await;
    fixture
        .emulation
        .set_platform_settings(PlatformSettings {
            platform: "nintendo_snes".into(),
            default: Some(profile),
            bios_dir: None,
        })
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    let record = fixture.record(10).await;
    let mut progress = fixture.romm.subscribe();

    let launch = LaunchService::new(fixture.store.clone())
        .with_emulators(EmulatorLaunches::new(
            fixture.store.clone(),
            fixture.emulation.clone(),
        ))
        .with_preparer(Arc::new(fixture.romm.clone()))
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(20),
            launch_timeout: Duration::from_secs(10),
            exit_grace: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(200),
        });
    #[cfg(unix)]
    {
        let handle = launch.launch(&record.entry_id).await.unwrap();
        let session = handle.wait().await.unwrap();
        assert_eq!(session.game_id, record.game_id);
        let sessions = fixture
            .store
            .sessions_for_game(&record.game_id, 10)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1, "a play session is recorded");
    }
    #[cfg(not(unix))]
    {
        // No shell script emulator here: prepare as the launch would.
        let _ = &launch;
        assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    }

    let mut phases = Vec::new();
    while let Ok(update) = progress.try_recv() {
        phases.push(update.phase);
    }
    assert_eq!(
        phases.first(),
        Some(&caskade_core::romm::DownloadPhase::Downloading)
    );
    assert_eq!(
        phases.last(),
        Some(&caskade_core::romm::DownloadPhase::Done)
    );

    let record = fixture.record(10).await;
    let cache = record.cache.clone().expect("cached");
    assert_eq!(cache.dir, fixture.cache().join("home").join("10"));
    assert_eq!(std::fs::read(&cache.roms[0]).unwrap(), CHRONO);
    assert!(!fixture.cache().join(".staging/home-10").exists());
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    let (_, _, roms) = emulated(&entry.launch);
    assert_eq!(roms, cache.roms);
    assert_eq!(
        entry.location,
        Some(InstallLocation::External {
            path: cache.dir.clone()
        })
    );
    let prepared = EmulatorLaunches::new(fixture.store.clone(), fixture.emulation.clone())
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.args,
        [cache.roms[0].to_string_lossy().into_owned()]
    );

    // The second launch needs no network, even with the server gone.
    let before = fixture.requests().await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    assert_eq!(fixture.requests().await, before);
    // Port 1, which nothing in the tests binds: a port freed by a dropped
    // listener could go to another test's server and answer (#119).
    let closed = "127.0.0.1:1";
    let (romm, store) = (fixture.romm.clone(), fixture.store.clone());
    romm.save_server(server_config(&format!("http://{closed}")), None)
        .await
        .unwrap();
    assert!(romm.prepare_entry(&record.entry_id).await.unwrap());

    // An uncached game is unavailable, with a clear message.
    let mana = store.romm_rom("home", 11).await.unwrap().unwrap();
    let error = romm.prepare_entry(&mana.entry_id).await.unwrap_err();
    assert!(error.to_string().contains("can't be reached"), "{error}");
    let status = romm.game_status(&mana.entry_id).await.unwrap().unwrap();
    assert_eq!(status.status, "unavailable");
    let entry = store.get_entry(&mana.entry_id).await.unwrap().unwrap();
    assert!(matches!(entry.state, InstallState::Broken { .. }));
    // The cached one still is.
    let status = romm.game_status(&record.entry_id).await.unwrap().unwrap();
    assert_eq!(status.status, "cached");
}

#[tokio::test]
async fn a_multi_file_rom_is_verified_unpacked_and_gets_a_playlist() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let record = fixture.record(20).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(20).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Final Fantasy VII.m3u")]);
    assert_eq!(
        std::fs::read_to_string(&cache.roms[0]).unwrap(),
        "Disc 1.cue\nDisc 2.cue\n"
    );
    assert_eq!(
        std::fs::read(cache.dir.join("Disc 2.bin")).unwrap(),
        DISC2_BIN
    );
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(emulated(&entry.launch).0, "sony_playstation");

    // A file that doesn't match the server's hash fails and isn't cached.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"corrupted".to_vec()))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Verify(_)), "{error}");
    assert!(fixture.record(11).await.cache.is_none());
}

#[tokio::test]
async fn an_interrupted_download_resumes_and_a_download_can_be_cancelled() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    // Half of Secret of Mana is already on disk.
    let staging = fixture.cache().join(".staging/home-11");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("download.part"), &MANA[..10]).unwrap();
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .and(header("range", "bytes=10-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header(
                    "content-range",
                    format!("bytes 10-{}/{}", MANA.len() - 1, MANA.len()).as_str(),
                )
                .set_body_bytes(MANA[10..].to_vec()),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    assert!(fixture.romm.prepare_entry(&mana.entry_id).await.unwrap());
    let cache = fixture.record(11).await.cache.unwrap();
    assert_eq!(std::fs::read(&cache.roms[0]).unwrap(), MANA);
    let ranged: Vec<Request> = fixture
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.headers.contains_key("range"))
        .collect();
    assert_eq!(ranged.len(), 1);

    // A slow download, cancelled.
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/10/content/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(CHRONO.to_vec())
                .set_delay(Duration::from_secs(30)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let chrono = fixture.record(10).await;
    let romm = fixture.romm.clone();
    let entry = chrono.entry_id.clone();
    let running = tokio::spawn(async move { romm.prepare_entry(&entry).await });
    for _ in 0..200 {
        if fixture.romm.cancel_download(&chrono.entry_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("cancelling stops the download")
        .unwrap();
    assert!(matches!(outcome, Err(RommError::Cancelled)));
    assert!(fixture.record(10).await.cache.is_none());
}

#[tokio::test]
async fn the_platform_mapping_and_a_per_game_override_take_effect_at_launch() {
    let fixture = Fixture::new().await;
    let first = fixture.emulator("first").await;
    let second = fixture.emulator("second").await;
    fixture.romm.sync("home").await.unwrap();

    // Map SNES to the first emulator.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].platforms = vec![RommPlatformMapping {
        slug: "snes".into(),
        enabled: true,
        platform: Some("nintendo_snes".into()),
        emulator_id: Some(first.emulator_id.clone()),
        profile_id: Some(first.profile_id.clone()),
    }];
    fixture.romm.save_settings(settings).await.unwrap();
    let chrono = fixture.record(10).await;
    assert!(fixture.romm.prepare_entry(&chrono.entry_id).await.unwrap());
    let entry = fixture
        .store
        .get_entry(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    let expected = EmulatorTarget::Profile {
        emulator_id: first.emulator_id.clone(),
        profile_id: first.profile_id.clone(),
    };
    assert_eq!(*emulated(&entry.launch).1, expected);

    // The game page pins Secret of Mana to the second emulator; a resync
    // and its launch keep that.
    let mana = fixture.record(11).await;
    let override_target = EmulatorTarget::Profile {
        emulator_id: second.emulator_id.clone(),
        profile_id: second.profile_id.clone(),
    };
    fixture
        .emulation
        .set_game(
            &mana.entry_id,
            override_target.clone(),
            ArgsOverride::Default,
        )
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    assert!(fixture.romm.prepare_entry(&mana.entry_id).await.unwrap());
    let entry = fixture
        .store
        .get_entry(&mana.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*emulated(&entry.launch).1, override_target);
    let prepared = EmulatorLaunches::new(fixture.store.clone(), fixture.emulation.clone())
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.emulation.unwrap().emulator_id,
        second.emulator_id
    );
}

/// A cached record with a pretend size (the limit is in GiB).
async fn cached(fixture: &Fixture, rom: u32, gib_tenths: u64, used: i64, pinned: bool) -> RommRom {
    let mut record = fixture.record(rom).await;
    let dir = fixture.cache().join("home").join(rom.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rom.sfc"), b"x").unwrap();
    record.pinned = pinned;
    record.cache = Some(caskade_core::romm::CachedRom {
        roms: vec![dir.join("rom.sfc")],
        dir,
        size: (gib_tenths << 30) / 10,
        cached_at_ms: used,
        last_used_ms: used,
    });
    fixture.store.save_romm_rom(&record).await.unwrap();
    record
}

#[tokio::test]
async fn the_cache_limit_evicts_least_recently_played_but_never_pinned_or_favourites() {
    let server = MockServer::builder().start().await;
    let snes: Vec<Value> = (10..16)
        .map(|id| {
            snes_rom(
                id,
                &format!("Game {id}"),
                &format!("Game {id}.sfc"),
                CHRONO,
                "2026-01-01T00:00:00Z",
            )
        })
        .collect();
    mount(&server, snes).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();
    // Oldest first: 10 pinned, 11 a favourite, 12 and 13 plain, 14 newest.
    let pinned = cached(&fixture, 10, 4, 1, true).await;
    let favourite = cached(&fixture, 11, 4, 2, false).await;
    let old = cached(&fixture, 12, 4, 3, false).await;
    let older_used = cached(&fixture, 13, 4, 4, false).await;
    let newest = cached(&fixture, 14, 4, 5, false).await;
    let mut game = fixture
        .store
        .game(&favourite.game_id)
        .await
        .unwrap()
        .unwrap();
    game.favorite = true;
    fixture.store.upsert_game(&game).await.unwrap();

    // 2 GiB cached, 1 GiB allowed.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    let overview = fixture.romm.save_settings(settings).await.unwrap();
    let kept: Vec<u32> = overview
        .cache
        .games
        .iter()
        .map(|game| game.rom_id)
        .collect();
    assert!(kept.contains(&pinned.rom_id));
    assert!(kept.contains(&favourite.rom_id));
    assert!(
        !kept.contains(&old.rom_id),
        "least recently played goes first"
    );
    assert!(!kept.contains(&older_used.rom_id));
    assert!(!kept.contains(&newest.rom_id));
    assert!(!old.cache.unwrap().dir.exists());
    assert!(pinned.cache.unwrap().dir.exists());
    // Evicted games stay in the library and download again at launch.
    let entry = fixture
        .store
        .get_entry(&old.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.state, InstallState::Installed);
    let status = fixture
        .romm
        .game_status(&old.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");

    // A ROM deleted on the server stays while cached, then goes with it.
    // The whole platform went at once: the first sync keeps them, the next
    // one the user starts confirms it.
    fixture.server.reset().await;
    mount(&fixture.server, Vec::new()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.orphaned, report.removed), (0, 0));
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.orphaned, 2);
    assert_eq!(report.removed, 4);
    assert!(fixture.romm.evict(&favourite.entry_id).await.unwrap());
    assert!(
        fixture
            .store
            .get_entry(&favourite.entry_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!Path::new(&fixture.cache().join("home/11")).exists());
}

#[tokio::test]
async fn bios_files_come_from_the_servers_firmware_when_their_md5_matches() {
    use caskade_core::emulation::{Bios, BiosProvider};
    const BIOS: &[u8] = b"playstation bios";
    const OTHER: &[u8] = b"another dump";
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/api/firmware"))
        .and(query_param("platform_id", "2"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 7, "platform_id": 2, "file_name": "SCPH5501.BIN", "md5_hash": md5(BIOS), "missing_from_fs": false },
            { "id": 8, "platform_id": 2, "file_name": "scph5502.bin", "md5_hash": md5(OTHER), "missing_from_fs": false },
            { "id": 9, "platform_id": 2, "file_name": "scph1001.bin", "md5_hash": null, "missing_from_fs": true }
        ])))
        .mount(&f.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/firmware/7/content/SCPH5501.BIN"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BIOS))
        .mount(&f.server)
        .await;
    let bios = |file: &str, md5: Option<String>| Bios {
        file: file.into(),
        md5,
        region: None,
        required: true,
    };
    let wanted = [
        bios("scph5501.bin", Some(md5(BIOS))),
        // Not the dump the definition names.
        bios("scph5502.bin", Some(md5(b"the right dump"))),
        // Gone from the server's disk.
        bios("scph1001.bin", None),
    ];
    let dir = f.temp.path().join("bios");
    let fetched = BiosProvider::fetch(&f.romm, "sony_playstation", &wanted, &dir)
        .await
        .unwrap();
    assert_eq!(fetched, ["scph5501.bin"]);
    assert_eq!(std::fs::read(dir.join("scph5501.bin")).unwrap(), BIOS);
    assert!(!dir.join("scph5502.bin").exists());
    // Written beside it and renamed: nothing half-written is left.
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["scph5501.bin"]);
    // Platforms the server doesn't map to fetch nothing.
    let none = BiosProvider::fetch(&f.romm, "nintendo_gba", &wanted, &dir)
        .await
        .unwrap();
    assert!(none.is_empty());
}

/// RomM hashes what an archive holds, not the archive: a zipped ROM's
/// SHA-1 is its contents', every member in path order.
#[tokio::test]
async fn zipped_roms_are_checked_by_what_they_hold_as_romm_hashes_them() {
    const BUGS: &[u8] = b"a bug's life game boy color rom";
    const CUE: &[u8] = b"FILE \"Ape Escape.bin\" BINARY\n";
    const BIN: &[u8] = b"ape escape disc data";
    let sha1 = |parts: &[&[u8]]| {
        use sha1::Digest;
        let mut hasher = sha1::Sha1::new();
        for part in parts {
            hasher.update(part);
        }
        format!("{:x}", hasher.finalize())
    };
    let zipped = |id: u32, platform: u32, slug: &str, fs_name: &str, bytes: &[u8], sha1: String| {
        json!({
            "id": id, "platform_id": platform, "platform_slug": slug,
            "fs_name": fs_name, "fs_name_no_ext": fs_name.trim_end_matches(".zip"),
            "fs_extension": "zip", "fs_size_bytes": bytes.len(),
            "name": fs_name.trim_end_matches(".zip"), "has_multiple_files": false,
            "files": [{
                "id": id, "file_name": fs_name, "file_path": "roms",
                "file_size_bytes": bytes.len(), "sha1_hash": sha1, "category": "game"
            }],
            "sha1_hash": sha1,
            "updated_at": "2026-01-01T00:00:00Z", "missing_from_fs": false
        })
    };
    let server = MockServer::builder().start().await;
    let bugs = zip(&[("Bug's Life, A (U) [C][!].sfc", BUGS)]);
    let other = zip(&[("Other.sfc", b"other bytes")]);
    let mut snes = default_snes();
    snes.push(zipped(
        12,
        1,
        "snes",
        "Bug's Life, A (U) [C][!].zip",
        &bugs,
        sha1(&[BUGS]),
    ));
    // RomM hashed something else than what the server sends.
    snes.push(zipped(
        13,
        1,
        "snes",
        "Other.zip",
        &other,
        sha1(&[b"not these bytes"]),
    ));
    mount(&server, snes).await;
    content(&server, 12, bugs).await;
    content(&server, 13, other).await;
    // A disc in one zip: RomM hashes its members sorted by name.
    let ape = zip(&[("Ape Escape.cue", CUE), ("Ape Escape.bin", BIN)]);
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![
            ps1_rom(),
            zipped(21, 2, "ps", "Ape Escape.zip", &ape, sha1(&[BIN, CUE])),
        ])))
        .with_priority(1)
        .mount(&server)
        .await;
    content(&server, 21, ape).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();

    // SNES takes zips: the archive is kept for the emulator.
    let record = fixture.record(12).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(12).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Bug's Life, A (U) [C][!].zip")]);
    assert_eq!(std::fs::read_dir(&cache.dir).unwrap().count(), 1);

    // PS1 doesn't: the disc is unpacked and checked as unpacked.
    let record = fixture.record(21).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(21).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Ape Escape.cue")]);

    let record = fixture.record(13).await;
    let error = fixture
        .romm
        .prepare_entry(&record.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Verify(_)), "{error}");
    assert!(fixture.record(13).await.cache.is_none());
}

/// Port 1, which nothing in the tests binds (it takes root on Unix): a port
/// freed by a dropped listener could go to another test's server (#119).
fn closed_port() -> String {
    "http://127.0.0.1:1".to_owned()
}

async fn cached_ids(fixture: &Fixture) -> Vec<u32> {
    let mut ids: Vec<u32> = fixture
        .romm
        .overview()
        .await
        .unwrap()
        .cache
        .games
        .iter()
        .map(|game| game.rom_id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Play on a server that can't be reached frees no room: the download
/// never started, so other downloads stay.
#[tokio::test]
async fn play_with_the_server_unreachable_evicts_nothing() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    cached(&fixture, 10, 4, 1, false).await;
    cached(&fixture, 20, 4, 2, false).await;
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    fixture.romm.save_settings(settings).await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
    // Secret of Mana needs half a GiB more than fits.
    let mut mana = fixture.record(11).await;
    mana.rom.fs_size_bytes = 1 << 29;
    fixture.store.save_romm_rom(&mana).await.unwrap();

    fixture
        .romm
        .save_server(server_config(&closed_port()), None)
        .await
        .unwrap();
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("can't be reached"), "{error}");
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
    for rom in [10, 20] {
        assert!(fixture.record(rom).await.cache.unwrap().dir.exists());
        assert_eq!(entry_state(&fixture, rom).await, InstallState::Installed);
    }

    // A server that answers with an error frees nothing either.
    fixture
        .romm
        .save_server(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
}

/// Automatic eviction waits for a working connection: a lower limit while
/// the server is offline evicts nothing, and the next sync that reaches it
/// evicts down to the limit.
#[tokio::test]
async fn the_cache_limit_waits_for_the_server_to_answer() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    cached(&fixture, 10, 4, 1, false).await;
    cached(&fixture, 11, 4, 2, false).await;
    cached(&fixture, 20, 4, 3, false).await;

    fixture
        .romm
        .save_server(server_config(&closed_port()), None)
        .await
        .unwrap();
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Unreachable(_)), "{error}");
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    fixture.romm.save_settings(settings).await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [10, 11, 20]);
    // Unpinning enforces the limit too, and holds back the same way.
    let pinned = fixture.record(20).await;
    for pin in [true, false] {
        fixture
            .romm
            .set_pinned(&pinned.entry_id, pin)
            .await
            .unwrap();
    }
    assert_eq!(cached_ids(&fixture).await, [10, 11, 20]);
    // Downloaded games stay playable offline.
    for rom in [10, 11, 20] {
        assert_eq!(entry_state(&fixture, rom).await, InstallState::Installed);
    }

    // Back online: the sync evicts the least recently played.
    fixture
        .romm
        .save_server(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [11, 20]);
    assert!(fixture.record(10).await.cache.is_none());
}
