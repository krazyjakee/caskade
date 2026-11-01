//! RetroAchievements against a local copy of its web API (#189): hashes,
//! connecting, progress and linking releases by verified hash.

use super::{hash::*, *};
use crate::{
    achievements::{AccountAccess, AchievementLink, Achievements, LinkMethod, SetStatus},
    bus::Bus,
    emulation::{ArgsOverride, EmulatorTarget, GameRom},
    model::{Game, SourceId},
    store_accounts::SyncState,
};
use md5::{Digest, Md5};
use serde_json::json;
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn md5(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn hashes_follow_retroachievements_rules() {
    let body = vec![7u8; 32 * 1024];
    // NES: the iNES header isn't hashed.
    let mut nes = b"NES\x1a".to_vec();
    nes.extend([0u8; 12]);
    nes.extend(&body);
    assert_eq!(hash_bytes(&nes, Rule::Nes).unwrap(), md5(&body));
    assert_eq!(hash_bytes(&body, Rule::Nes).unwrap(), md5(&body));
    // SNES: a 512-byte copier header isn't either.
    let mut snes = vec![0u8; 512];
    snes.extend(&body);
    assert_eq!(hash_bytes(&snes, Rule::Snes).unwrap(), md5(&body));
    // N64: byte-swapped and little-endian dumps hash as big-endian.
    let z64: Vec<u8> = [0x80, 0x37, 0x12, 0x40, 1, 2, 3, 4].to_vec();
    let v64: Vec<u8> = [0x37, 0x80, 0x40, 0x12, 2, 1, 4, 3].to_vec();
    let n64: Vec<u8> = [0x40, 0x12, 0x37, 0x80, 4, 3, 2, 1].to_vec();
    for dump in [&v64, &n64] {
        assert_eq!(hash_bytes(dump, Rule::N64).unwrap(), md5(&z64));
    }
    assert_eq!(hash_bytes(&body, Rule::Whole).unwrap(), md5(&body));
    assert_eq!(console("nintendo_snes"), Some((3, Rule::Snes)));
    assert_eq!(console("sony_playstation"), Some((12, Rule::Unsupported)));
    assert_eq!(console("nintendo_switch"), None);
}

#[tokio::test]
async fn arcade_sets_hash_their_name_and_archives_are_left_alone() {
    let temp = tempfile::tempdir().unwrap();
    let set = temp.path().join("sf2.zip");
    std::fs::write(&set, b"whatever").unwrap();
    assert_eq!(hash_file(&set, Rule::Arcade).await.unwrap(), md5(b"sf2"));
    assert_eq!(hash_file(&set, Rule::Whole).await, Err(HashError::Archive));
    assert_eq!(
        hash_file(&set, Rule::Unsupported).await,
        Err(HashError::UnsupportedSystem)
    );
}

fn provider(server: &MockServer) -> RetroAchievements {
    RetroAchievements::with_base(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        "https://media.example",
    )
}
fn credentials() -> StoreCredentials {
    StoreCredentials {
        access_token: "KEY".into(),
        refresh_token: None,
        expires_at_ms: None,
        extra: [("username".to_owned(), "Player".to_owned())].into(),
    }
}

#[tokio::test]
async fn connecting_checks_the_name_and_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetUserProfile.php"))
        .and(query_param("y", "KEY"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"User": "Player", "ULID": "01HXYZ"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/API/API_GetUserProfile.php"))
        .and(query_param("y", "WRONG"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let ra = provider(&server);
    let fields = |key: &str| AuthInput::Fields {
        fields: [
            ("username".into(), "player".into()),
            ("key".into(), key.into()),
        ]
        .into(),
    };
    let signed = ra.complete_auth("flow", &fields("KEY")).await.unwrap();
    assert_eq!(signed.external_id, "01HXYZ");
    assert_eq!(signed.display_name, "Player");
    assert_eq!(signed.credentials.unwrap().extra["username"], "Player");
    assert!(matches!(
        ra.complete_auth("flow", &fields("WRONG")).await,
        Err(ProviderError::Rejected(_))
    ));
    assert!(matches!(
        ra.complete_auth("flow", &AuthInput::Code { code: "x".into() })
            .await,
        Err(ProviderError::Rejected(_))
    ));
    // A connection for achievements owns no games.
    assert!(!AccountProvider::capabilities(&ra).library);
}

fn account() -> StoreAccount {
    StoreAccount {
        id: "ra".into(),
        provider: ID.into(),
        external_id: "01HXYZ".into(),
        display_name: "Player".into(),
        owner_id: None,
        enabled: true,
        sync: SyncState::default(),
        created_at_ms: 0,
    }
}

async fn progress(server: &MockServer, game: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameInfoAndUserProgress.php"))
        .and(query_param("g", game))
        .and(query_param("u", "Player"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn progress_keeps_hardcore_and_softcore_apart() {
    let server = MockServer::start().await;
    progress(
        &server,
        "1446",
        json!({
            "ID": 1446, "Title": "Super Mario Bros.", "NumDistinctPlayers": 200,
            "Achievements": {
                "2": {"ID": 2, "Title": "World 2", "Description": "Reach world 2", "Points": 10,
                      "BadgeName": "12345", "DisplayOrder": 2, "NumAwarded": 50,
                      "DateEarned": "2024-01-02 10:00:00"},
                "1": {"ID": 1, "Title": "World 1", "Description": "Beat 1-1", "Points": 5,
                      "BadgeName": "12344", "DisplayOrder": 1, "NumAwarded": 150,
                      "DateEarned": "2024-01-01 10:00:00", "DateEarnedHardcore": "2024-01-01 10:00:00"},
                "3": {"ID": 3, "Title": "World 8", "Description": "", "Points": 25,
                      "BadgeName": "12346", "DisplayOrder": 3, "NumAwarded": 0}
            }
        }),
    )
    .await;
    progress(
        &server,
        "99",
        json!({"ID": 99, "Title": "Empty", "Achievements": []}),
    )
    .await;
    progress(&server, "404", json!({})).await;
    let ra = provider(&server);
    let cancel = CancellationToken::new();
    let fetched = ra
        .fetch(&account(), Some(&credentials()), "1446", &cancel)
        .await
        .unwrap();
    let ids: Vec<_> = fetched.definitions.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "3"], "in the game's display order");
    let first = &fetched.states["1"];
    assert!(first.unlocked && first.hardcore.as_ref().unwrap().unlocked);
    let second = &fetched.states["2"];
    assert!(second.unlocked && !second.hardcore.as_ref().unwrap().unlocked);
    assert!(!fetched.states["3"].unlocked);
    assert_eq!(fetched.definitions[0].rarity_percent, Some(75.0));
    assert_eq!(fetched.definitions[0].points, Some(5));
    assert_eq!(
        fetched.definitions[0].locked_icon_url.as_deref(),
        Some("https://media.example/Badge/12344_lock.png")
    );
    assert_eq!(fetched.definitions[2].description, None);
    assert!(fetched.definitions.iter().all(|d| !d.hidden));
    // A game with none, and a game RetroAchievements doesn't have.
    let empty = ra
        .fetch(&account(), Some(&credentials()), "99", &cancel)
        .await
        .unwrap();
    assert!(empty.complete && empty.definitions.is_empty());
    assert!(matches!(
        ra.fetch(&account(), Some(&credentials()), "404", &cancel)
            .await,
        Err(FetchError::Unsupported(_))
    ));
    // No sign-in: connect again.
    assert!(matches!(
        ra.fetch(&account(), None, "1446", &cancel).await,
        Err(FetchError::Provider(ProviderError::Expired(_)))
    ));
}

/// The library's store accounts, signed in with the test key.
struct Access(LibraryStore);
#[async_trait]
impl AccountAccess for Access {
    async fn accounts(&self) -> crate::Result<Vec<StoreAccount>> {
        self.0.store_accounts().await
    }
    async fn credentials(
        &self,
        _: &StoreAccount,
    ) -> Result<Option<StoreCredentials>, ProviderError> {
        Ok(Some(credentials()))
    }
    fn provider_name(&self, _: &str) -> Option<String> {
        Some(NAME.into())
    }
}

/// An emulated game on `platform` with these ROM files.
async fn emulated(
    store: &LibraryStore,
    title: &str,
    platform: &str,
    roms: &[std::path::PathBuf],
) -> (String, String) {
    let game = Game::new(title);
    let mut entry = GameEntry::new(&game.id, SourceId::new("roms"), title);
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: platform.into(),
        target: EmulatorTarget::PlatformDefault,
        roms: roms.iter().map(GameRom::absolute).collect(),
        args: ArgsOverride::default(),
    });
    let group = store.import_group(&game, &[entry], None).await.unwrap();
    (group.game_id, group.entry_ids[0].clone())
}

#[tokio::test]
async fn releases_link_only_by_a_verified_hash_and_can_be_linked_by_hand() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let rom = |name: &str, bytes: &[u8]| {
        let path = temp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let known = rom("smb.nes", &[1u8; 4096]);
    let revision = rom("smb (rev a).nes", &[2u8; 4096]);
    let shared = rom("shared.nes", &[3u8; 4096]);
    let disc = rom("game.cue", b"FILE");
    let other = rom("other.nes", &[4u8; 4096]);
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameList.php"))
        .and(query_param("i", "7"))
        .and(query_param("h", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"ID": 1446, "Title": "Super Mario Bros.", "Hashes": [md5(&[1u8; 4096])]},
            {"ID": 2000, "Title": "Twin A", "Hashes": [md5(&[3u8; 4096])]},
            {"ID": 2001, "Title": "Twin B", "Hashes": [md5(&[3u8; 4096])]},
            {"ID": 3000, "Title": "Other", "Hashes": [md5(&[4u8; 4096])]}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    progress(
        &server,
        "1446",
        json!({"ID": 1446, "NumDistinctPlayers": 1, "Achievements": {
            "1": {"ID": 1, "Title": "World 1", "DateEarnedHardcore": "2024-01-01 10:00:00"}
        }}),
    )
    .await;
    let store = LibraryStore::in_memory().await.unwrap();
    store.save_store_account(&account()).await.unwrap();
    let ra = Arc::new(provider(&server));
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(ra.clone());
    service.register_linker(ra.clone());
    let cancel = CancellationToken::new();

    let (smb, _) = emulated(&store, "Super Mario Bros.", "nintendo_nes", &[known]).await;
    let sets = service.refresh_game(&smb, None, &cancel).await.unwrap();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].product_id, "1446");
    assert_eq!(sets[0].sync.status, SetStatus::Ready);
    assert_eq!((sets[0].unlocked, sets[0].hardcore_unlocked), (1, Some(1)));
    let link = service.links(&smb).await.unwrap().remove(0);
    assert!(matches!(link.method, LinkMethod::Hash { .. }));

    // The same title in another revision isn't the same game.
    let unresolved = |reason: &str, link: &AchievementLink| match &link.method {
        LinkMethod::Unresolved { reason: why } => assert!(why.contains(reason), "{why}"),
        method => panic!("expected unresolved, got {method:?}"),
    };
    let (rev, rev_entry) = emulated(
        &store,
        "Super Mario Bros. (Rev A)",
        "nintendo_nes",
        &[revision],
    )
    .await;
    assert!(
        service
            .refresh_game(&rev, None, &cancel)
            .await
            .unwrap()
            .is_empty()
    );
    unresolved("another revision", &service.links(&rev).await.unwrap()[0]);
    // A hash two games share, a disc system and a release whose ROMs are
    // two games stay unresolved.
    let (twin, _) = emulated(&store, "Twin", "nintendo_nes", &[shared]).await;
    service.refresh_game(&twin, None, &cancel).await.unwrap();
    unresolved("more than one", &service.links(&twin).await.unwrap()[0]);
    let (psx, _) = emulated(&store, "Disc game", "sony_playstation", &[disc]).await;
    service.refresh_game(&psx, None, &cancel).await.unwrap();
    unresolved("by hand", &service.links(&psx).await.unwrap()[0]);
    let (two, _) = emulated(
        &store,
        "Two games",
        "nintendo_nes",
        &[temp.path().join("smb.nes"), other],
    )
    .await;
    service.refresh_game(&two, None, &cancel).await.unwrap();
    unresolved("different", &service.links(&two).await.unwrap()[0]);

    // Linked by hand, the revision shows the set; clearing it undoes that.
    store
        .save_achievement_link(&AchievementLink {
            entry_id: rev_entry.clone(),
            provider: ID.into(),
            product_id: "1446".into(),
            method: LinkMethod::Manual,
            linked_at_ms: 0,
        })
        .await
        .unwrap();
    let sets = service.refresh_game(&rev, None, &cancel).await.unwrap();
    assert_eq!(sets.len(), 1);
    assert!(matches!(
        service.links(&rev).await.unwrap()[0].method,
        LinkMethod::Manual
    ));
    assert!(store.delete_achievement_link(&rev_entry, ID).await.unwrap());
    assert!(service.sets(&rev, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn refreshing_the_account_links_the_library_and_fetches_its_sets() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let smb = temp.path().join("smb.nes");
    std::fs::write(&smb, [1u8; 4096]).unwrap();
    let unknown = temp.path().join("homebrew.nes");
    std::fs::write(&unknown, [9u8; 4096]).unwrap();
    Mock::given(method("GET"))
        .and(path("/API/API_GetGameList.php"))
        .and(query_param("i", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"ID": 1446, "Title": "Super Mario Bros.", "Hashes": [md5(&[1u8; 4096])]}
        ])))
        .mount(&server)
        .await;
    progress(
        &server,
        "1446",
        json!({"ID": 1446, "Achievements": {
            "1": {"ID": 1, "Title": "World 1", "DateEarned": "2024-01-01 10:00:00"}
        }}),
    )
    .await;
    let store = LibraryStore::in_memory().await.unwrap();
    store.save_store_account(&account()).await.unwrap();
    let ra = Arc::new(provider(&server));
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(ra.clone());
    service.register_linker(ra);
    let (game, _) = emulated(&store, "Super Mario Bros.", "nintendo_nes", &[smb]).await;
    let (homebrew, _) = emulated(&store, "Homebrew", "nintendo_nes", &[unknown]).await;
    let cancel = CancellationToken::new();

    service.refresh_account(&account(), &cancel).await.unwrap();
    let set = service.sets(&game, None).await.unwrap().remove(0);
    assert_eq!(set.sync.status, SetStatus::Ready);
    assert_eq!(set.unlocked, 1);
    assert!(service.sets(&homebrew, None).await.unwrap().is_empty());
    assert!(matches!(
        service.links(&homebrew).await.unwrap()[0].method,
        LinkMethod::Unresolved { .. }
    ));
    // Unreachable: the refresh says so and keeps what was known.
    let offline = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    offline.register_provider(Arc::new(RetroAchievements::with_base(
        Url::parse("http://127.0.0.1:1/").unwrap(),
        "https://media.example",
    )));
    assert!(matches!(
        offline.refresh_account(&account(), &cancel).await,
        Err(ProviderError::Offline(_))
    ));
    assert_eq!(offline.sets(&game, None).await.unwrap()[0].unlocked, 1);
}

#[tokio::test]
async fn an_invalid_key_needs_a_new_sign_in_and_offline_keeps_progress() {
    let server = MockServer::start().await;
    let store = LibraryStore::in_memory().await.unwrap();
    store.save_store_account(&account()).await.unwrap();
    let ra = Arc::new(provider(&server));
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(ra.clone());
    let (game, entry) = emulated(&store, "Linked", "nintendo_nes", &[]).await;
    store
        .save_achievement_link(&AchievementLink {
            entry_id: entry,
            provider: ID.into(),
            product_id: "1446".into(),
            method: LinkMethod::Manual,
            linked_at_ms: 0,
        })
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    progress(
        &server,
        "1446",
        json!({"ID": 1446, "Achievements": {
            "1": {"ID": 1, "Title": "World 1", "DateEarned": "2024-01-01 10:00:00"}
        }}),
    )
    .await;
    service.refresh_game(&game, None, &cancel).await.unwrap();
    // The service can't be reached: what was known stays, marked stale.
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(Arc::new(RetroAchievements::with_base(
        Url::parse("http://127.0.0.1:1/").unwrap(),
        "https://media.example",
    )));
    let set = service
        .refresh_game(&game, None, &cancel)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(set.sync.status, SetStatus::Ready);
    assert_eq!(set.unlocked, 1);
    assert!(set.sync.error.is_some());
    // A key RetroAchievements refuses.
    let refusing = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&refusing)
        .await;
    let service = Achievements::new(
        store.clone(),
        Bus::default(),
        Arc::new(Access(store.clone())),
    );
    service.register_provider(Arc::new(provider(&refusing)));
    let set = service
        .refresh_game(&game, None, &cancel)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(set.sync.status, SetStatus::SignInNeeded);
    assert_eq!(set.unlocked, 1, "progress already seen stays");
}

/// Against the real service, with `CASKADE_RA_USER` and `CASKADE_RA_KEY`
/// (and optionally `CASKADE_RA_GAME`, 1446 by default): connects and
/// reads one game's progress. Read only.
#[tokio::test]
#[ignore = "needs a RetroAchievements account: CASKADE_RA_USER, CASKADE_RA_KEY"]
async fn live() {
    let user = std::env::var("CASKADE_RA_USER").expect("CASKADE_RA_USER");
    let key = std::env::var("CASKADE_RA_KEY").expect("CASKADE_RA_KEY");
    let game = std::env::var("CASKADE_RA_GAME").unwrap_or_else(|_| "1446".into());
    let ra = RetroAchievements::new();
    let signed = ra
        .complete_auth(
            "live",
            &AuthInput::Fields {
                fields: [("username".into(), user), ("key".into(), key)].into(),
            },
        )
        .await
        .unwrap();
    let fetched = ra
        .fetch(
            &account(),
            signed.credentials.as_ref(),
            &game,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(fetched.complete);
    println!(
        "{}: {} achievements, {} unlocked",
        signed.display_name,
        fetched.definitions.len(),
        fetched.states.values().filter(|s| s.unlocked).count()
    );
}
