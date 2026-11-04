//! Presence between two cores on loopback (#240): a game started on one
//! shows as playing there on the other within seconds, and goes when it
//! ends; reading presence never waits on the network, and a device that
//! can't be reached stops counting.

use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::{Bus, Event, GameSession};
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::model::{ExternalId, Game, GameEntry, InstallState, PlaySession, SourceId};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use caskade_core::sync::presence::Presence;

struct Core {
    store: LibraryStore,
    bus: Bus,
    devices: Devices,
    sync: SyncService,
    presence: Presence,
    game: String,
    entry: String,
    _dir: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    /// The game starts here, as the launch service records and announces it.
    async fn start(&self) -> PlaySession {
        let session =
            PlaySession::new(&self.game, &self.entry, caskade_core::sync::clock::now_ms());
        self.store.start_session(&session).await.unwrap();
        self.bus.emit(Event::GameStarted(event(&session)));
        session
    }

    async fn exit(&self, session: &PlaySession) {
        let now = caskade_core::sync::clock::now_ms();
        let ended = self
            .store
            .finish_session(&session.id, now, 1000)
            .await
            .unwrap();
        self.bus.emit(Event::GameExited(event(&ended)));
    }
}

fn event(session: &PlaySession) -> GameSession {
    GameSession {
        game_id: session.game_id.clone(),
        entry_id: session.entry_id.clone(),
        session_id: session.id.clone(),
        started_at_ms: session.started_at_ms,
        ended_at_ms: session.ended_at_ms,
        duration_ms: session.duration_ms,
    }
}

async fn core(name: &str) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let accounts = AccountService::open(store.clone(), bus.clone())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();
    // The same Steam game on both, so the devices know it's one game.
    let game = store
        .upsert_game(&Game::new("Steam Fixture"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "620");
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    entry.state = InstallState::Installed;
    let entry = store.upsert_entry(&entry).await.unwrap();
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    let presence = Presence::new(store.clone());
    presence.set_account(|| Some("player".into()));
    sync.set_presence(presence.clone());
    presence.watch(&bus, sync.clone(), devices.clone());
    let core = Core {
        store,
        bus,
        devices,
        sync,
        presence,
        game: game.id,
        entry: entry.id,
        _dir: dir,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    core
}

async fn pair(a: &Core, b: &Core) {
    let link = a.devices.pairing_link().await.unwrap();
    b.devices.accept_pairing_link(&link.url).await.unwrap();
    let b_id = b.id().await;
    within("the link pairs both", 20, || async {
        a.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == b_id)
    })
    .await;
}

async fn within<F, Fut>(what: &str, seconds: u64, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} within {seconds} s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_running_on_one_device_shows_on_the_other_within_seconds_and_goes_when_it_ends() {
    let desktop = core("Desktop").await;
    let deck = core("Deck").await;
    pair(&desktop, &deck).await;
    let (_a, _b) = (desktop.sync.start(), deck.sync.start());
    // They know the game is the same one once the library has synced.
    let desktop_id = desktop.id().await;
    within("the first sync", 20, || async {
        deck.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == desktop_id && d.last_sync_ms.is_some())
    })
    .await;
    assert!(deck.presence.playing(&deck.devices).await.is_empty());

    let session = desktop.start().await;
    let started = Instant::now();
    within("the deck sees the desktop playing", 10, || async {
        deck.presence
            .playing(&deck.devices)
            .await
            .iter()
            .any(|playing| {
                playing.device_id == desktop_id
                    && playing.game_id == deck.game
                    && playing.account_id == "player"
                    && playing.started_at_ms == session.started_at_ms
            })
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(10));
    // The desktop doesn't list its own game.
    assert!(desktop.presence.playing(&desktop.devices).await.is_empty());

    desktop.exit(&session).await;
    within("the deck sees it end", 10, || async {
        deck.presence.playing(&deck.devices).await.is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn presence_never_waits_on_a_device_out_of_reach_and_stops_counting_it() {
    let desktop = core("Desktop").await;
    let deck = core("Deck").await;
    pair(&desktop, &deck).await;
    let (_a, _b) = (desktop.sync.start(), deck.sync.start());
    let desktop_id = desktop.id().await;
    desktop.start().await;
    within("the deck sees the desktop playing", 20, || async {
        deck.presence
            .playing(&deck.devices)
            .await
            .iter()
            .any(|playing| playing.device_id == desktop_id)
    })
    .await;

    // The desktop is switched off mid-game.
    desktop.devices.shutdown().await;
    let asked = Instant::now();
    deck.presence.playing(&deck.devices).await;
    assert!(
        asked.elapsed() < Duration::from_millis(500),
        "{:?}",
        asked.elapsed()
    );
    // One nudge: each starts a dial of its own. Dialling a device that's gone
    // takes its time on a loaded machine.
    deck.sync.sync_now(&desktop_id);
    within("the deck stops counting the desktop", 60, || async {
        let asked = Instant::now();
        let playing = deck.presence.playing(&deck.devices).await;
        assert!(asked.elapsed() < Duration::from_millis(500));
        playing.is_empty()
    })
    .await;
}
