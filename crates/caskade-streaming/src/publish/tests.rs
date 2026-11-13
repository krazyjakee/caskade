//! Publishing against a fake host (Sunshine's app list behind TLS): each
//! library change reaches it in one batch, and only Caskade's apps change.

use super::*;
use crate::{
    secrets::Credentials,
    testing::{Apps, cert, tls_front},
};
use caskade_core::{
    AppPaths,
    bus::BusOptions,
    model::{Game, GameEntry, SourceId},
};
use std::sync::atomic::{AtomicBool, Ordering};
use wiremock::{Mock, MockServer, matchers::path_regex};

struct Fake {
    api: HostApi,
    ready: AtomicBool,
    /// The app list file Caskade may write whole: its own host's.
    list: Option<PathBuf>,
}

#[async_trait::async_trait]
impl Target for Fake {
    fn ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    async fn api(&self) -> Result<HostApi> {
        Ok(self.api.clone())
    }
    async fn app_list(&self) -> Option<PathBuf> {
        self.list.clone()
    }
    async fn flatpak(&self) -> bool {
        false
    }
}

struct Fixture {
    store: LibraryStore,
    bus: Bus,
    apps: Apps,
    publisher: Publisher,
    target: Arc<Fake>,
    _server: MockServer,
    _temp: tempfile::TempDir,
}

const PROGRAM: &str = "/opt/caskade/caskade";

/// A client for `server`'s API, behind TLS.
async fn client(server: &MockServer) -> HostApi {
    let served = cert();
    let front = tls_front(*server.address(), &served).await;
    HostApi::new(
        format!("https://{front}/").parse().unwrap(),
        Credentials {
            user: "u".into(),
            password: "p".into(),
        },
        served.der.clone(),
    )
    .unwrap()
}

async fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let apps = Apps::default();
    // A user's own app is on the host already.
    caskade_core::lock(&apps.0)
        .push(serde_json::json!({ "name": "Desktop", "image-path": "desktop.png" }));
    let server = MockServer::start().await;
    Mock::given(path_regex("^/api/apps(/[0-9]+)?$"))
        .respond_with(apps.clone())
        .mount(&server)
        .await;
    let target = Arc::new(Fake {
        api: client(&server).await,
        ready: AtomicBool::new(true),
        list: None,
    });
    let publisher = Publisher::new(
        store.clone(),
        target.clone(),
        None,
        None,
        &AppPaths::under(temp.path()),
    )
    .unwrap()
    .with(PROGRAM.into(), Duration::from_millis(200));
    Fixture {
        store,
        bus: Bus::new(BusOptions::default()),
        apps,
        publisher,
        target,
        _server: server,
        _temp: temp,
    }
}

fn names(apps: &Apps) -> Vec<String> {
    caskade_core::lock(&apps.0)
        .iter()
        .map(|app| app["name"].as_str().unwrap().to_owned())
        .collect()
}

async fn installed(store: &LibraryStore, title: &str) -> (Game, GameEntry) {
    let game = Game::new(title);
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), title);
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Uri {
        uri: "steam://rungameid/1".into(),
    });
    store.upsert_entry(&entry).await.unwrap();
    (game, entry)
}

/// Waits until `batches` batches went out.
async fn after(f: &Fixture, batches: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while f.publisher.batches() < batches {
        assert!(
            std::time::Instant::now() < deadline,
            "batch {batches} never went"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_library_change_reaches_the_host_in_one_batch() {
    let f = fixture().await;
    let (hades, mut hades_entry) = installed(&f.store, "Hades").await;
    let watch = f.publisher.watch(&f.bus);
    after(&f, 1).await;
    assert_eq!(names(&f.apps), ["Caskade", "Desktop", "Hades"]);
    let hades_app = caskade_core::lock(&f.apps.0)
        .iter()
        .find(|app| app["name"] == "Hades")
        .cloned()
        .unwrap();
    assert_eq!(
        hades_app["cmd"],
        format!("\"{PROGRAM}\" --launch {}", hades.id)
    );
    assert_eq!(hades_app["auto-detach"], false);
    assert_eq!(hades_app["wait-all"], true);

    // Installing a game, with several events: one batch.
    let (celeste, _) = installed(&f.store, "Celeste").await;
    for _ in 0..3 {
        f.bus.emit(Event::LibraryGameAdded {
            game_id: celeste.id.clone(),
        });
    }
    after(&f, 2).await;
    assert_eq!(names(&f.apps), ["Caskade", "Celeste", "Desktop", "Hades"]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(f.publisher.batches(), 2, "one batch for the burst");

    // Renaming.
    let mut renamed = f.store.game(&celeste.id).await.unwrap().unwrap();
    renamed.title = "Celeste Classic".into();
    f.store.upsert_game(&renamed).await.unwrap();
    f.bus.emit(Event::LibraryGameUpdated {
        game_id: celeste.id.clone(),
    });
    after(&f, 3).await;
    assert_eq!(
        names(&f.apps),
        ["Caskade", "Celeste Classic", "Desktop", "Hades"]
    );

    // Hiding.
    let mut hidden = f.store.game(&celeste.id).await.unwrap().unwrap();
    hidden.hidden = true;
    f.store.upsert_game(&hidden).await.unwrap();
    f.bus.emit(Event::LibraryGameUpdated {
        game_id: celeste.id.clone(),
    });
    after(&f, 4).await;
    assert_eq!(names(&f.apps), ["Caskade", "Desktop", "Hades"]);

    // Uninstalling.
    hades_entry.state = InstallState::Partial;
    f.store.upsert_entry(&hades_entry).await.unwrap();
    f.bus.emit(Event::UninstallCompleted {
        job_id: "j".into(),
        game_id: Some(hades.id.clone()),
    });
    after(&f, 5).await;
    assert_eq!(names(&f.apps), ["Caskade", "Desktop"]);

    // Off: only Caskade's apps go.
    watch.abort();
    assert_eq!(f.publisher.withdraw().await.unwrap(), 1);
    assert_eq!(names(&f.apps), ["Desktop"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_sent_until_the_host_answers_then_it_catches_up() {
    let f = fixture().await;
    f.target.ready.store(false, Ordering::SeqCst);
    installed(&f.store, "Hades").await;
    let watch = f.publisher.watch(&f.bus);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(f.publisher.batches(), 0);
    assert_eq!(names(&f.apps), ["Desktop"]);
    f.target.ready.store(true, Ordering::SeqCst);
    // The watch looks again within its 5 s wait.
    after(&f, 1).await;
    assert_eq!(names(&f.apps), ["Caskade", "Desktop", "Hades"]);
    watch.abort();
}

#[tokio::test]
async fn a_game_the_hosts_account_may_not_play_is_left_off() {
    let f = fixture().await;
    let accounts = AccountService::open(f.store.clone(), Bus::default())
        .await
        .unwrap();
    let admin = accounts.active_id().unwrap();
    accounts.set_pin(&admin, Some("2468".into())).await.unwrap();
    let kid = accounts
        .create(caskade_core::accounts::NewAccount {
            name: "Kid".into(),
            avatar: caskade_core::accounts::DEFAULT_AVATAR.into(),
            access: caskade_core::accounts::AccountAccess {
                role: caskade_core::accounts::AccountRole::Standard,
                permissions: caskade_core::accounts::Permissions::default(),
                max_age: Some(12),
                unrated_needs_pin: false,
            },
            pin: None,
        })
        .await
        .unwrap()
        .id;
    accounts.sign_in(&kid, None).await.unwrap();
    let (adult, _) = installed(&f.store, "Grown-up Game").await;
    f.store
        .set_game_age_ratings(
            &adult.id,
            &[caskade_core::igdb::IgdbAgeRating {
                board: "pegi".into(),
                rating: "18".into(),
            }],
        )
        .await
        .unwrap();
    installed(&f.store, "Family Game").await;
    let publisher = Publisher::new(
        f.store.clone(),
        f.target.clone(),
        Some(accounts),
        None,
        &AppPaths::under(f._temp.path()),
    )
    .unwrap()
    .with(PROGRAM.into(), Duration::from_millis(10));
    let titles: Vec<String> = publisher
        .offers()
        .await
        .unwrap()
        .into_iter()
        .filter_map(|offer| match offer {
            Offer::Game { title, .. } => Some(title),
            Offer::Console => None,
        })
        .collect();
    assert_eq!(titles, ["Family Game"]);
}

#[tokio::test]
async fn a_cover_goes_to_the_host_as_a_png() {
    let f = fixture().await;
    let cache = ArtworkCache::new(f._temp.path().join("artwork")).unwrap();
    let (hades, _) = installed(&f.store, "Hades").await;
    // A 900×1200 cover, as the library keeps it.
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(900, 1200, image::Rgb([200, 40, 40]))
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    let cached = cache.import_bytes(jpeg.into_inner()).await.unwrap();
    f.store
        .upsert_artwork(&cached.record(&hades.id, ArtworkKind::Cover, "igdb:co1"))
        .await
        .unwrap();
    let publisher = Publisher::new(
        f.store.clone(),
        f.target.clone(),
        None,
        Some(cache),
        &AppPaths::under(f._temp.path()),
    )
    .unwrap()
    .with(PROGRAM.into(), Duration::from_millis(10));
    publisher.publish().await.unwrap();
    let app = caskade_core::lock(&f.apps.0)
        .iter()
        .find(|app| app["name"] == "Hades")
        .cloned()
        .unwrap();
    let path = std::path::PathBuf::from(app["image-path"].as_str().unwrap());
    let png = image::open(&path).unwrap();
    assert_eq!(
        image::ImageFormat::from_path(&path).unwrap(),
        image::ImageFormat::Png
    );
    assert_eq!((png.width(), png.height()), (600, 800));
}

/// Sunshine's saves as it makes them: an added app goes at the end of the
/// list file, which is sorted by name and written again (and every app's
/// cover read again, which is what makes a save slow).
#[derive(Clone)]
struct SavingHost {
    list: PathBuf,
    saves: Arc<std::sync::atomic::AtomicUsize>,
}

impl wiremock::Respond for SavingHost {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.list).unwrap()).unwrap();
        if request.method.as_str() != "POST" {
            return wiremock::ResponseTemplate::new(405);
        }
        let mut app: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        app.as_object_mut().unwrap().remove("index");
        let apps = file["apps"].as_array_mut().unwrap();
        apps.push(app);
        apps.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        std::fs::write(&self.list, serde_json::to_vec(&file).unwrap()).unwrap();
        self.saves.fetch_add(1, Ordering::SeqCst);
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": true }))
    }
}

/// Caskade's own host gets a batch in one save, however many games it
/// holds (#145), and none when nothing changed; its list is Caskade's, so
/// an earlier build's apps and a second copy go.
#[tokio::test]
async fn its_own_host_gets_a_batch_in_one_save() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let list = temp.path().join("apps.json");
    let earlier = Caskade {
        program: "/usr/bin/caskade".into(),
        flatpak_host: false,
    };
    let this = Caskade {
        program: PROGRAM.into(),
        flatpak_host: false,
    };
    let (hades, _) = installed(&store, "Hades").await;
    let stale = Offer::Game {
        title: "Hades".into(),
        game_id: hades.id.clone(),
        image: None,
    };
    let start = serde_json::json!({
        "env": { "PATH": "$(PATH)" },
        "apps": [earlier.app(&stale), this.app(&stale), this.app(&stale)],
    });
    std::fs::write(&list, serde_json::to_vec(&start).unwrap()).unwrap();
    let host = SavingHost {
        list: list.clone(),
        saves: Arc::default(),
    };
    let server = MockServer::start().await;
    Mock::given(path_regex("^/api/apps(/[0-9]+)?$"))
        .respond_with(host.clone())
        .mount(&server)
        .await;
    let target = Arc::new(Fake {
        api: client(&server).await,
        ready: AtomicBool::new(true),
        list: Some(list.clone()),
    });
    for title in ["Celeste", "Balatro", "Tunic"] {
        installed(&store, title).await;
    }
    let publisher = Publisher::new(
        store.clone(),
        target,
        None,
        None,
        &AppPaths::under(temp.path()),
    )
    .unwrap()
    .with(PROGRAM.into(), Duration::from_millis(10));
    let listed = || -> Vec<(String, Option<String>)> {
        let file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&list).unwrap()).unwrap();
        file["apps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|app| {
                let app: App = serde_json::from_value(app.clone()).unwrap();
                (app.name.clone(), app.cmd.clone())
            })
            .collect()
    };

    let published = publisher.publish().await.unwrap().unwrap();
    assert_eq!(host.saves.load(Ordering::SeqCst), 1);
    assert_eq!(
        published,
        Published {
            added: 4,
            removed: 2
        }
    );
    let names: Vec<String> = listed().into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["Balatro", "Caskade", "Celeste", "Hades", "Tunic"]);
    assert!(
        listed().iter().all(|(_, cmd)| cmd
            .as_deref()
            .is_none_or(|cmd| cmd.starts_with(&format!("\"{PROGRAM}\"")))),
        "{:?}",
        listed()
    );
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(&list).unwrap()).unwrap();
    assert_eq!(file["env"]["PATH"], "$(PATH)", "the rest of the file stays");

    // Nothing changed: nothing is sent.
    assert_eq!(
        publisher.publish().await.unwrap(),
        Some(Published::default())
    );
    assert_eq!(host.saves.load(Ordering::SeqCst), 1);

    // A game more: one more save.
    installed(&store, "Outer Wilds").await;
    publisher.publish().await.unwrap();
    assert_eq!(host.saves.load(Ordering::SeqCst), 2);
    assert!(listed().iter().any(|(name, _)| name == "Outer Wilds"));
    assert_eq!(listed().len(), 6);
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "POST"),
        "only the saves reach the host"
    );
}

/// An AppImage start's entries start the AppImage, not the mount it runs
/// from, which is gone once Caskade quits (#116).
#[test]
fn an_appimage_start_publishes_the_appimage_file() {
    let env = |name: &str| match name {
        "APPIMAGE" => Some("/home/p/Apps/Caskade.AppImage".into()),
        "APPDIR" => Some("/tmp/.mount_CaskadXyz".into()),
        _ => None,
    };
    let caskade = Caskade {
        program: caskade_core::program::from(
            std::path::PathBuf::from("/tmp/.mount_CaskadXyz/usr/bin/caskade"),
            env,
        ),
        flatpak_host: false,
    };
    assert_eq!(
        caskade.launch_command("g1"),
        "\"/home/p/Apps/Caskade.AppImage\" --launch g1"
    );
    assert_eq!(
        caskade.console_command(),
        "\"/home/p/Apps/Caskade.AppImage\" --console"
    );
    // A Caskade another AppImage started publishes itself.
    let inherited = Caskade {
        program: caskade_core::program::from(std::path::PathBuf::from("/usr/bin/caskade"), env),
        flatpak_host: true,
    };
    assert_eq!(
        inherited.launch_command("g1"),
        "flatpak-spawn --host \"/usr/bin/caskade\" --launch g1"
    );
}

#[test]
fn caskade_names_this_program() {
    let own = Caskade::own(false).unwrap();
    assert_eq!(own.program, caskade_core::program::current().unwrap());
}

#[test]
fn caskades_apps_are_told_by_their_command() {
    use crate::{
        api::App,
        publish::{Caskade, Offer},
    };
    let caskade = Caskade {
        program: "/opt/Caskade Games/caskade".into(),
        flatpak_host: false,
    };
    let ours = caskade.app(&Offer::Game {
        title: "Hades".into(),
        game_id: "g1".into(),
        image: None,
    });
    assert_eq!(
        ours.cmd.as_deref(),
        Some("\"/opt/Caskade Games/caskade\" --launch g1")
    );
    assert!(matches!(caskade.owns(&ours), Some(Offer::Game { game_id, .. }) if game_id == "g1"));
    let console = caskade.app(&Offer::Console);
    assert_eq!(console.cmd, None);
    assert_eq!(caskade.owns(&console), Some(Offer::Console));
    let theirs = App::command("Steam", "setsid steam steam://open/bigpicture");
    assert_eq!(caskade.owns(&theirs), None);
    // Another Caskade's (another install) isn't this one's.
    let other = Caskade {
        program: "/usr/bin/caskade".into(),
        flatpak_host: false,
    };
    assert_eq!(caskade.owns(&other.app(&Offer::Console)), None);
    // A Flatpak host's commands leave its sandbox.
    let flatpak = Caskade {
        flatpak_host: true,
        ..caskade
    };
    assert_eq!(
        flatpak.launch_command("g1"),
        "flatpak-spawn --host \"/opt/Caskade Games/caskade\" --launch g1"
    );
}
