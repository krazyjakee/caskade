use super::*;
use crate::{
    install::{InstallOptions, STORAGE_SOURCE},
    model::{Game, GameEntry, StorageRoot},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

struct Fixture {
    store: LibraryStore,
    updates: Updates,
    server: MockServer,
    root: StorageRoot,
    _temp: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-updates").unwrap();
    identity.write(temp.path()).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = StorageRoot::from_manifest(temp.path().to_path_buf(), &identity);
    store.upsert_root(&root).await.unwrap();
    let server = MockServer::start().await;
    let web = Url::parse(&format!("{}/", server.uri())).unwrap();
    let tracker = Tracker::new(store.clone(), vec![Arc::new(GitHub::new(web))]);
    let bus = Bus::default();
    let install = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    Fixture {
        updates: Updates::new(store.clone(), bus, tracker, install, None),
        store,
        server,
        root,
        _temp: temp,
    }
}

/// A managed install of a new game, in folder `dir`.
async fn entry(store: &LibraryStore, root: &StorageRoot, dir: &str) -> GameEntry {
    let game = store.upsert_game(&Game::new(dir)).await.unwrap();
    let mut entry = GameEntry::new(&game.id, STORAGE_SOURCE, uuid::Uuid::now_v7().to_string());
    entry.state = caskade_discovery::InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: root.id.clone(),
        rel_dir: dir.into(),
    });
    store.upsert_entry(&entry).await.unwrap()
}

async fn follow(store: &LibraryStore, entry: &GameEntry, project: &str, version: Option<&str>) {
    store
        .follow_updates(
            &entry.id,
            "github",
            &Followed {
                project: project.into(),
                version: version.map(Into::into),
                asset: version.map(|_| "Game-1.0-linux-x64.zip".into()),
            },
        )
        .await
        .unwrap();
}

async fn redirect(server: &MockServer, from: &str, to: &str) {
    Mock::given(method("HEAD"))
        .and(path(from))
        .respond_with(ResponseTemplate::new(302).insert_header("location", to))
        .mount(server)
        .await;
}

#[tokio::test]
async fn checks_find_newer_releases_and_respect_the_user_s_choices() {
    let f = fixture().await;
    let game = entry(&f.store, &f.root, "Game").await;
    follow(&f.store, &game, "owner/game", Some("v1.0")).await;
    redirect(
        &f.server,
        "/owner/game/releases/latest",
        "/owner/game/releases/tag/v1.1",
    )
    .await;

    let update = f.updates.check(&game.id).await.unwrap();
    assert_eq!(update.installed.as_deref(), Some("v1.0"));
    assert_eq!(update.latest.as_deref(), Some("v1.1"));
    assert!(update.available);
    assert_eq!(
        update.latest_url.as_deref(),
        Some(format!("{}/owner/game/releases/tag/v1.1", f.server.uri()).as_str())
    );
    assert_eq!(
        update.project_url.as_deref(),
        Some(format!("{}/owner/game/releases", f.server.uri()).as_str())
    );
    // Just checked: Play offers it without asking GitHub again.
    let offered = f.updates.before_play(&game.id).await.unwrap().unwrap();
    assert_eq!(offered.checked_at_ms, update.checked_at_ms);

    let skipped = f.updates.skip(&game.id, Some("v1.1".into())).await.unwrap();
    assert!(!skipped.available);
    assert_eq!(f.updates.before_play(&game.id).await.unwrap(), None);

    // Prereleases come from the release feed, newest first.
    Mock::given(method("GET"))
        .and(path("/owner/game/releases.atom"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<feed><entry><link rel="alternate" type="text/html" href="{0}/owner/game/releases/tag/v1.2-beta"/></entry>
               <entry><link rel="alternate" type="text/html" href="{0}/owner/game/releases/tag/v1.1"/></entry></feed>"#,
            f.server.uri()
        )))
        .mount(&f.server)
        .await;
    let update = f.updates.set_prereleases(&game.id, true).await.unwrap();
    assert!(update.prereleases);
    assert_eq!(update.latest.as_deref(), Some("v1.2-beta"));
    assert!(update.available);
}

#[tokio::test]
async fn unknown_installs_start_at_the_newest_release_and_failures_keep_the_last_answer() {
    let f = fixture().await;
    let unknown = entry(&f.store, &f.root, "Unknown").await;
    follow(&f.store, &unknown, "owner/unknown", None).await;
    redirect(
        &f.server,
        "/owner/unknown/releases/latest",
        "/owner/unknown/releases/tag/2.0",
    )
    .await;
    let update = f.updates.check(&unknown.id).await.unwrap();
    assert_eq!(update.installed.as_deref(), Some("2.0"));
    assert!(!update.available);

    let flaky = entry(&f.store, &f.root, "Flaky").await;
    follow(&f.store, &flaky, "owner/flaky", Some("1.0")).await;
    let fail = Mock::given(method("HEAD"))
        .and(path("/owner/flaky/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "/owner/flaky/releases/tag/1.1"),
        )
        .up_to_n_times(1)
        .mount_as_scoped(&f.server)
        .await;
    assert!(f.updates.check(&flaky.id).await.unwrap().available);
    drop(fail);
    Mock::given(method("HEAD"))
        .and(path("/owner/flaky/releases/latest"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&f.server)
        .await;
    let update = f.updates.check(&flaky.id).await.unwrap();
    assert!(update.error.is_some());
    assert_eq!(update.latest.as_deref(), Some("1.1"));
    assert!(update.available);

    // A project without a release has nothing to offer.
    let none = entry(&f.store, &f.root, "None").await;
    follow(&f.store, &none, "owner/none", Some("1.0")).await;
    let update = f.updates.check(&none.id).await.unwrap();
    assert_eq!((update.latest, update.error), (None, None));
}

#[tokio::test]
async fn play_asks_github_only_when_the_background_check_is_overdue() {
    let f = fixture().await;
    let game = entry(&f.store, &f.root, "Game").await;
    follow(&f.store, &game, "owner/game", Some("v1.0")).await;
    redirect(
        &f.server,
        "/owner/game/releases/latest",
        "/owner/game/releases/tag/v1.1",
    )
    .await;
    f.updates.check(&game.id).await.unwrap();
    let requests = || async { f.server.received_requests().await.unwrap().len() };
    let asked = requests().await;
    let checked_ago = |hours: i64| {
        let (store, id) = (&f.store, game.id.clone());
        async move {
            let mut row = store.game_update(&id).await.unwrap().unwrap();
            row.checked_at_ms = now_ms() - hours * 60 * 60 * 1000;
            store.save_update_check(&row).await.unwrap();
        }
    };

    // Two hours old, within the six-hour interval: the watch's answer stands.
    checked_ago(2).await;
    assert!(f.updates.before_play(&game.id).await.unwrap().is_some());
    assert_eq!(requests().await, asked);

    // Seven hours old, overdue: Play checks first.
    checked_ago(7).await;
    assert!(f.updates.before_play(&game.id).await.unwrap().is_some());
    assert!(requests().await > asked);
}

#[tokio::test]
async fn a_failed_check_keeps_the_last_good_one_and_is_retried_sooner() {
    let f = fixture().await;
    let game = entry(&f.store, &f.root, "Game").await;
    follow(&f.store, &game, "owner/game", Some("v1.0")).await;
    redirect(
        &f.server,
        "/owner/game/releases/latest",
        "/owner/game/releases/tag/v1.1",
    )
    .await;
    let good = f.updates.check(&game.id).await.unwrap();
    let mut row = f.store.game_update(&game.id).await.unwrap().unwrap();
    // Due again.
    row.checked_at_ms = 1;
    f.store.save_update_check(&row).await.unwrap();
    f.server.reset().await;
    Mock::given(method("HEAD"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&f.server)
        .await;
    let heads = || async { f.server.received_requests().await.unwrap().len() };

    assert_eq!(f.updates.check_due().await.unwrap(), 1);
    let failed = f.updates.game(&game.game_id).await.unwrap().remove(0);
    assert!(failed.error.is_some());
    // The time of the last check that worked stays, and so does its answer.
    assert_eq!(failed.checked_at_ms, 1);
    assert_eq!(failed.latest, good.latest);
    // Neither the watch nor Play asks again straight away...
    assert_eq!(f.updates.check_due().await.unwrap(), 0);
    assert!(f.updates.before_play(&game.id).await.unwrap().is_some());
    assert_eq!(heads().await, 1);
    // ...but half an hour on, not a whole interval.
    f.updates.inner.retry_at.lock().unwrap().clear();
    assert_eq!(f.updates.check_due().await.unwrap(), 1);
    assert_eq!(heads().await, 2);
}

#[tokio::test]
async fn updates_install_the_file_matching_the_one_installed() {
    let f = fixture().await;
    let game = entry(&f.store, &f.root, "Game").await;
    follow(&f.store, &game, "owner/game", Some("v1.0")).await;
    let row = f.store.game_update(&game.id).await.unwrap().unwrap();
    let github = GitHub::new(Url::parse(&format!("{}/", f.server.uri())).unwrap());
    redirect(
        &f.server,
        "/owner/game/releases/download/v1.1/Game-1.1-linux-x64.zip",
        "https://objects.example/1",
    )
    .await;
    assert_eq!(
        github.download(&row, "v1.1").await.unwrap(),
        format!(
            "{}/owner/game/releases/download/v1.1/Game-1.1-linux-x64.zip",
            f.server.uri()
        )
    );
    // The release names its files differently: the installer picks one.
    assert_eq!(
        github.download(&row, "v2.0").await.unwrap(),
        format!("{}/owner/game/releases/tag/v2.0", f.server.uri())
    );
}

#[tokio::test]
async fn installs_are_followed_from_their_urls_and_the_file_downloaded() {
    let f = fixture().await;
    let tracker = Tracker::builtin(f.store.clone());
    let game = entry(&f.store, &f.root, "Game").await;
    let manifest = |input: &str| {
        let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../caskade-installer/tests/manifests/install.v1.posix.json"
        ))
        .unwrap();
        value["source"]["input"] = serde_json::json!({ "type": "urls", "value": [input] });
        InstallManifest::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    };
    let download =
        Url::parse("https://github.com/owner/game/releases/download/v1.0/Game-1.0.zip").unwrap();
    let row = tracker
        .track(
            &game.id,
            &manifest("https://github.com/owner/game"),
            Some(&download),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            row.provider.as_str(),
            row.project.as_str(),
            row.installed.as_deref(),
            row.asset.as_deref(),
            row.prereleases
        ),
        (
            "github",
            "owner/game",
            Some("v1.0"),
            Some("Game-1.0.zip"),
            false
        )
    );
    // The user's choices stay while the project does.
    let mut chosen = row.clone();
    chosen.skipped = Some("v1.1".into());
    f.store.save_update_choices(&chosen).await.unwrap();
    let row = tracker
        .track(&game.id, &manifest("https://github.com/owner/game"), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.installed.as_deref(), row.skipped.as_deref()),
        (Some("v1.0"), Some("v1.1"))
    );

    // Anything else isn't followed, and isn't looked at again.
    let other = entry(&f.store, &f.root, "Other").await;
    assert_eq!(
        tracker
            .track(&other.id, &manifest("https://example.com/game.zip"), None)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        f.store
            .game_update(&other.id)
            .await
            .unwrap()
            .unwrap()
            .provider,
        NOT_FOLLOWED
    );
    assert!(f.updates.game(&other.game_id).await.unwrap().is_empty());

    // Installs from before updates were followed are picked up from their
    // manifests.
    let old = entry(&f.store, &f.root, "Old").await;
    let dir = f.root.path.join("Old");
    std::fs::create_dir_all(&dir).unwrap();
    manifest(&format!("{}/owner/old/releases/tag/v3", f.server.uri()))
        .write(&dir)
        .unwrap();
    assert_eq!(f.updates.track_missing().await.unwrap(), 1);
    let row = f.store.game_update(&old.id).await.unwrap().unwrap();
    assert_eq!(
        (row.project.as_str(), row.installed.as_deref()),
        ("owner/old", Some("v3"))
    );
    assert_eq!(f.updates.track_missing().await.unwrap(), 0);
}

/// Against github.com: `cargo test -p caskade-core updates::tests::live -- --ignored`.
#[tokio::test]
#[ignore = "needs github.com"]
async fn live() {
    let github = GitHub::default();
    let stable = github.latest("ppy/osu", false).await.unwrap().unwrap();
    assert!(
        stable
            .url
            .ends_with(&format!("/releases/tag/{}", stable.version))
    );
    let newest = github.latest("ppy/osu", true).await.unwrap().unwrap();
    assert!(!newest.version.is_empty());
    assert_eq!(
        github
            .latest("ppy/no-such-repository-here", false)
            .await
            .unwrap(),
        None
    );
    // osu! names its AppImage the same in every release.
    let row = UpdateRow {
        entry_id: String::new(),
        game_id: String::new(),
        provider: "github".into(),
        project: "ppy/osu".into(),
        installed: Some("2020.1.1".into()),
        asset: Some("osu.AppImage".into()),
        prereleases: false,
        latest: None,
        latest_url: None,
        checked_at_ms: 0,
        skipped: None,
        error: None,
    };
    assert_eq!(
        github.download(&row, &stable.version).await.unwrap(),
        format!(
            "https://github.com/ppy/osu/releases/download/{}/osu.AppImage",
            stable.version
        )
    );
    println!("stable {} · newest {}", stable.version, newest.version);
}
