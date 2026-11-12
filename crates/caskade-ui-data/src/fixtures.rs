//! Fixtures mode (#89): the real core and app layer over an in-memory
//! library of synthetic games, for UI development and tests. It replaces
//! the webview's `VITE_CASKADE_FIXTURES`; the games are the ones its
//! `apps/desktop/src/api/fixtures.ts` shows (`createFixtureGames` and
//! `createEmulatedFixtureGames`), so both UIs can be compared on the same
//! library. No contributor's library is copied.
//!
//! Filters, sorts and search are the core's own here, not a re-implementation:
//! where `fixtures.ts` approximates the core (its Installed filter leaves
//! emulated games out; the core counts their installed releases), fixtures
//! mode answers as the app does.

use caskade_app::events::{Emit, Event, Events};
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::{LibraryStore, PlaytimeImport, RemoteTotal},
    emulation::{ArgsOverride, EmulatorTarget},
    launch::LaunchService,
    model::{Game, GameEntry, InstallState, LaunchSpec, SourceId},
};
use tokio::sync::broadcast;

use crate::addons::{AddonParts, Addons};
use crate::jobs::{FakeQueue, FakeRefresh, Jobs, requests::FakeInbox};
use crate::plugins::Plugins;
use crate::stores::Stores;
use crate::{Discovery, Library, Settings, Themes};

/// How many plain games fixtures mode makes unless asked for another number
/// (`createFixtureGames`'s default).
pub const DEFAULT_COUNT: usize = 24;

/// The emulated games' platforms, how many games each has, and how many days
/// ago its first game was played (`None`: never), as `fixtures.ts` has them.
pub const EMULATED: [(&str, usize, Option<u32>); 8] = [
    ("Super Nintendo Entertainment System", 6, Some(2)),
    ("Nintendo 64", 4, Some(9)),
    ("Sony PlayStation", 5, Some(30)),
    ("Sega Genesis", 3, None),
    ("Nintendo Game Boy Advance", 4, Some(1)),
    ("Sega Dreamcast", 2, None),
    ("Nintendo GameCube", 3, Some(60)),
    ("Arcade", 2, None),
];

const DAY_MS: i64 = 86_400_000;

/// One synthetic game, as `fixtures.ts` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureGame {
    pub id: String,
    pub entry_id: String,
    pub title: String,
    pub genre: &'static str,
    pub platform: String,
    pub installed: bool,
    pub emulated: bool,
    pub favorite: bool,
    pub playtime_ms: u64,
    /// When it was last played, in days before the library was made.
    pub last_played_days_ago: Option<u32>,
    pub release_date: Option<&'static str>,
}

/// `count` plain PC games (`createFixtureGames(count)`).
pub fn games(count: usize) -> Vec<FixtureGame> {
    (0..count)
        .map(|index| FixtureGame {
            id: format!("fixture-{index}"),
            entry_id: format!("entry-{index}"),
            title: format!("Fixture Game {:04}", index + 1),
            genre: if index % 2 == 1 {
                "Adventure"
            } else {
                "Strategy"
            },
            platform: "PC".into(),
            installed: index % 5 != 0,
            emulated: false,
            favorite: index % 3 == 0,
            playtime_ms: index as u64 * 1_200_000,
            last_played_days_ago: None,
            release_date: (index % 4 != 0).then_some("2024-02-29"),
        })
        .collect()
}

/// The emulated games on several consoles (`createEmulatedFixtureGames`).
pub fn emulated_games() -> Vec<FixtureGame> {
    EMULATED
        .iter()
        .enumerate()
        .flat_map(|(p, &(platform, count, days_ago))| {
            let short = platform.rsplit(' ').next().unwrap_or(platform);
            (0..count).map(move |index| FixtureGame {
                id: format!("fixture-emulated-{p}-{index}"),
                entry_id: format!("fixture-emulated-entry-{p}-{index}"),
                title: format!("{short} Fixture {}", index + 1),
                // The template's: the first plain game's.
                genre: "Strategy",
                platform: platform.into(),
                installed: true,
                emulated: true,
                favorite: false,
                playtime_ms: if days_ago.is_some() {
                    (index as u64 + 1) * 2_700_000
                } else {
                    0
                },
                last_played_days_ago: days_ago.filter(|_| index == 0),
                release_date: None,
            })
        })
        .collect()
}

/// What fixtures mode shows: `count` plain games and the emulated ones, as
/// the webview's fixtures mode does.
pub fn library_games(count: usize) -> Vec<FixtureGame> {
    let mut games = games(count);
    games.extend(emulated_games());
    games
}

/// Writes `games` into `store`, played as of `now_ms`. Play time is a total
/// the "fixture" launcher reported, so a game can have play time without a
/// last play, as `fixtures.ts`'s can.
pub async fn seed(
    store: &LibraryStore,
    games: &[FixtureGame],
    now_ms: i64,
) -> caskade_core::Result<()> {
    let mut totals = Vec::new();
    for fixture in games {
        let mut game = Game::new(&fixture.title);
        game.id = fixture.id.clone();
        game.description = "Original development fixture. No personal library data.".into();
        game.genres = vec![fixture.genre.into()];
        game.tags = vec!["Fixture".into()];
        game.platforms = vec![fixture.platform.clone()];
        game.favorite = fixture.favorite;
        game.release_date = fixture.release_date.map(str::to_owned);
        store.upsert_game(&game).await?;
        let mut entry = GameEntry::new(&fixture.id, SourceId::new("fixture"), &fixture.id);
        entry.id = fixture.entry_id.clone();
        entry.state = if fixture.installed {
            InstallState::Installed
        } else {
            // Not installed: the core has no other state for a release
            // that isn't here.
            InstallState::Partial
        };
        entry.size = Some(1024 * 1024 * 1024);
        entry.launch = fixture.emulated.then(|| LaunchSpec::Emulator {
            platform_id: fixture.platform.clone(),
            target: EmulatorTarget::default(),
            roms: Vec::new(),
            args: ArgsOverride::default(),
        });
        store.upsert_entry(&entry).await?;
        if fixture.playtime_ms > 0 || fixture.last_played_days_ago.is_some() {
            totals.push(RemoteTotal {
                entry_id: fixture.entry_id.clone(),
                total_ms: Some(fixture.playtime_ms),
                last_played_ms: fixture
                    .last_played_days_ago
                    .map(|days| now_ms - i64::from(days) * DAY_MS),
            });
        }
    }
    store
        .record_remote_playtime("fixture", "", &totals, now_ms, PlaytimeImport::Always)
        .await?;
    Ok(())
}

/// The app layer's events in fixtures mode, as the native shell has them.
struct Broadcast(broadcast::Sender<Event>);

impl Emit for Broadcast {
    fn emit(&self, event: Event) -> Result<(), String> {
        // Nobody listening isn't a failure: views read state when they start.
        let _ = self.0.send(event);
        Ok(())
    }
}

/// An in-memory app with the fixture library.
pub struct Fixtures {
    pub library: Library,
    pub store: LibraryStore,
    pub bus: Bus,
    pub accounts: AccountService,
    /// Every app event, as the native shell's `shell::events()`.
    pub events: broadcast::Sender<Event>,
    /// The app's settings sections over the same store, at their defaults.
    pub settings: Settings,
    /// The built-in themes, over the same settings (Appearance).
    pub themes: Themes,
    /// Library discovery over the discovery crate's `linux-mixed` fixture
    /// machine (Steam, Lutris and a few games), never this device; `None`
    /// where the fixture isn't there (a build moved away from its sources).
    pub discovery: Option<Discovery>,
    /// Store accounts in memory, with no store: tests add fake ones.
    pub stores: Stores,
    /// Jobs over a fake queue, details refresh and install requests, empty
    /// until a test puts some there ([`Fixtures::job_queue`],
    /// [`Fixtures::refresh`], [`Fixtures::requests`]).
    pub jobs: Jobs,
    pub job_queue: std::sync::Arc<FakeQueue>,
    pub refresh: std::sync::Arc<FakeRefresh>,
    pub requests: std::sync::Arc<FakeInbox>,
    /// The add-on catalogue, empty until a test gives it one, at an
    /// address nothing answers: a refresh or a download fails at once.
    pub addons: Addons,
    /// A plugin host over this process's plugins folder
    /// ([`plugins_folder`]): none installed until a test adds one.
    pub plugins: Plugins,
    /// Plugins' drawn UI (#109) over a fake, with nothing drawn until a test
    /// puts plugins there ([`Fixtures::plugin_fake`]).
    pub plugin_ui: crate::plugin_ui::PluginUi,
    pub plugin_fake: std::sync::Arc<crate::plugin_ui::FakePluginUi>,
}

/// Where the fixture library's plugins are, one folder per process.
pub fn plugins_folder() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("caskade-fixture-plugins-{}", std::process::id()))
}

/// Where the fixture library's installed themes are: none until a test
/// adds one. One folder per process, so a test's theme reaches no other
/// run.
pub fn themes_folder() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("caskade-fixture-themes-{}", std::process::id()))
}

/// Opens the fixture library with `count` plain games (`DEFAULT_COUNT` is
/// what the webview shows) and the emulated ones, its bus bridged to
/// [`Fixtures::events`] as the app's is.
pub async fn open(count: usize) -> caskade_core::Result<Fixtures> {
    let store = LibraryStore::in_memory().await?;
    let bus = Bus::default();
    let accounts = AccountService::open(store.clone(), bus.clone()).await?;
    seed(&store, &library_games(count), caskade_core::now_ms()).await?;
    let launch = LaunchService::new(store.clone()).with_bus(bus.clone());
    let (events, _) = broadcast::channel(1024);
    caskade_app::events::bridge(Events::new(Broadcast(events.clone())), &bus);
    // The core's sections; nothing is written under these folders.
    let service = caskade_core::settings::SettingsService::with_bus(store.clone(), bus.clone());
    service
        .register_core(&caskade_core::AppPaths::under(
            &std::env::temp_dir().join("caskade-fixtures"),
        ))
        .map_err(|error| caskade_core::Error::Invalid(error.to_string()))?;
    let theme_service = caskade_core::themes::ThemeService::new(themes_folder());
    let themes = Themes::new(
        theme_service.clone(),
        service.clone(),
        None,
        Events::new(Broadcast(events.clone())),
    );
    // Play history import's section, as the app registers it.
    caskade_sources::register_settings(&service)
        .map_err(|error| caskade_core::Error::Invalid(error.to_string()))?;
    let discovery = caskade_discovery::env::HostEnv::fixture(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../caskade-discovery/tests/fixtures/machines/linux-mixed"
    ))
    .ok()
    .map(|env| {
        let controller = caskade_app::discovery::DiscoveryController::new(
            caskade_sources::DiscoveryService::new(store.clone(), bus.clone(), env),
            service.clone(),
        );
        controller.bridge(Events::new(Broadcast(events.clone())));
        Discovery::new(controller)
    });
    caskade_core::store_accounts::register_settings(&service)
        .map_err(|error| caskade_core::Error::Invalid(error.to_string()))?;
    let store_accounts =
        caskade_core::store_accounts::StoreAccounts::new(store.clone(), bus.clone());
    caskade_app::commands::store_accounts::setup(
        Events::new(Broadcast(events.clone())),
        &store_accounts,
    );
    let stores = Stores::new(store_accounts, Some(accounts.clone()));
    let plugins = {
        let folder = plugins_folder();
        std::fs::create_dir_all(folder.join("installed"))?;
        caskade_plugin_host::PluginHost::new(
            caskade_plugin_host::PluginServices {
                store: store.clone(),
                bus: Some(bus.clone()),
                settings: Some(service.clone()),
            },
            caskade_plugin_host::HostOptions {
                folders: vec![folder.join("installed")],
                data: folder.join("data"),
                limits: caskade_plugin_host::Limits::default(),
            },
        )
        .map_err(|error| tracing::warn!(%error, "the fixtures' plugin host didn't start"))
        .ok()
    };
    let addon_dir = plugins_folder().join("addons");
    let addons = Addons::new(AddonParts {
        catalogue: caskade_addons::CatalogueService::new(
            addon_dir.join("catalogue"),
            "http://127.0.0.1:1/",
            None,
        )
        .ok(),
        plugins: plugins.clone(),
        themes: theme_service,
        settings: service.clone(),
        store: store.clone(),
        bus: bus.clone(),
        events: Events::new(Broadcast(events.clone())),
        folders: caskade_app::commands::addon_packages::AddonFolders {
            plugins: plugins_folder().join("installed"),
            themes: themes_folder(),
            work: addon_dir.join("work"),
        },
        media: caskade_app::commands::addons::AddonMedia(None),
        sync: None,
        devices: None,
    });
    let plugins = Plugins::new(plugins);
    let job_queue = std::sync::Arc::new(FakeQueue::new(Events::new(Broadcast(events.clone()))));
    let refresh = std::sync::Arc::new(FakeRefresh::new(Events::new(Broadcast(events.clone()))));
    let requests = std::sync::Arc::new(FakeInbox::new(
        Events::new(Broadcast(events.clone())),
        job_queue.clone(),
    ));
    let jobs = Jobs::new(
        Some(job_queue.clone() as std::sync::Arc<dyn crate::jobs::JobQueue>),
        Some(refresh.clone() as std::sync::Arc<dyn crate::jobs::DetailsRefresh>),
    )
    .with_requests(requests.clone());
    let plugin_fake = std::sync::Arc::new(crate::plugin_ui::FakePluginUi::new(Events::new(
        Broadcast(events.clone()),
    )));
    let plugin_ui = crate::plugin_ui::PluginUi::new(Some(
        plugin_fake.clone() as std::sync::Arc<dyn crate::plugin_ui::PluginUiSource>
    ));
    let settings = Settings::new(service, accounts.clone());
    Ok(Fixtures {
        settings,
        themes,
        discovery,
        stores,
        plugins,
        plugin_ui,
        plugin_fake,
        addons,
        jobs,
        job_queue,
        refresh,
        requests,
        library: Library::new(store.clone(), accounts.clone(), launch, bus.clone()),
        store,
        bus,
        accounts,
        events,
    })
}

#[cfg(test)]
mod tests;
