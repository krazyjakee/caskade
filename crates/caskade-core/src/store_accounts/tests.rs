//! Store accounts against scripted fake providers (#164).

use super::*;
use crate::model::{Game, GameEntry, GameFilter, InstallLocation, InstallState, ListGames};
use caskade_discovery::ExternalId;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

/// A provider that answers each refresh with the next scripted snapshot.
struct Fake {
    id: &'static str,
    answers: Mutex<VecDeque<std::result::Result<LibrarySnapshot, ProviderError>>>,
    calls: AtomicU32,
    delay: Duration,
}

impl Fake {
    fn new(id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            id,
            answers: Mutex::default(),
            calls: AtomicU32::new(0),
            delay: Duration::ZERO,
        })
    }
    fn slow(id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            delay: Duration::from_millis(200),
            ..Arc::into_inner(Self::new(id)).unwrap()
        })
    }
    fn then(&self, answer: std::result::Result<LibrarySnapshot, ProviderError>) {
        crate::lock(&self.answers).push_back(answer);
    }
    fn owns(&self, products: &[OwnedProduct]) {
        self.then(Ok(complete(products)));
    }
}

#[async_trait]
impl AccountProvider for Fake {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: self.id.into(),
            name: self.id.to_uppercase(),
            auth: AuthKind::Launcher,
            library: true,
            playtime: false,
            install: false,
        }
    }
    async fn begin_auth(
        &self,
        _flow_id: &str,
    ) -> std::result::Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge::default())
    }
    async fn complete_auth(
        &self,
        _flow_id: &str,
        _input: &AuthInput,
    ) -> std::result::Result<SignedIn, ProviderError> {
        Err(ProviderError::Other("not used".into()))
    }
    async fn library(
        &self,
        _account: &StoreAccount,
        _credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::select! {
            () = tokio::time::sleep(self.delay) => {}
            () = cancel.cancelled() => return Err(ProviderError::Cancelled),
        }
        crate::lock(&self.answers)
            .pop_front()
            .unwrap_or(Err(ProviderError::Other("no answer scripted".into())))
    }
}

fn complete(products: &[OwnedProduct]) -> LibrarySnapshot {
    LibrarySnapshot {
        products: products.to_vec(),
        complete: true,
    }
}

fn steam(app_id: u32, title: &str) -> OwnedProduct {
    OwnedProduct {
        product_id: app_id.to_string(),
        title: title.into(),
        ids: vec![ExternalId::Steam { app_id }],
        platforms: vec![],
        playtime_minutes: None,
        last_played_ms: None,
        systems: Vec::new(),
    }
}

struct World {
    store: LibraryStore,
    accounts: StoreAccounts,
    steam: Arc<Fake>,
}

async fn world() -> World {
    world_with(Fake::new("steam")).await
}

async fn world_with(steam: Arc<Fake>) -> World {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(steam.clone());
    World {
        store,
        accounts,
        steam,
    }
}

async fn connect(world: &World, external: &str) -> StoreAccount {
    world
        .accounts
        .connect("steam", external, external, None)
        .await
        .unwrap()
}

async fn refresh(world: &World, account: &StoreAccount) -> RefreshOutcome {
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap()
}

async fn titles(store: &LibraryStore, filter: GameFilter) -> Vec<String> {
    let mut titles: Vec<String> = store
        .list_games(&ListGames {
            filter,
            ..ListGames::default()
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|game| game.title)
        .collect();
    titles.sort();
    titles
}

/// An installed copy discovery found, as its import saves it.
async fn discovered(store: &LibraryStore, app_id: u32, title: &str) -> String {
    let game = Game::new(title);
    let mut entry = GameEntry::new(
        &game.id,
        SourceId::from("steam".to_owned()),
        app_id.to_string(),
    );
    entry.ids = vec![ExternalId::Steam { app_id }];
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: PathBuf::from(format!("/games/{app_id}")),
    });
    store
        .import_group(&game, &[entry], None)
        .await
        .unwrap()
        .game_id
}

#[tokio::test]
async fn owned_games_are_library_items_that_are_not_installed() {
    let world = world().await;
    let account = connect(&world, "76561198000000001").await;
    world
        .steam
        .owns(&[steam(620, "Portal 2"), steam(400, "Portal")]);
    let outcome = refresh(&world, &account).await;
    assert_eq!(
        (outcome.added, outcome.lost, outcome.partial),
        (2, 0, false)
    );
    assert_eq!(
        titles(&world.store, GameFilter::All).await,
        ["Portal", "Portal 2"]
    );
    assert!(titles(&world.store, GameFilter::Installed).await.is_empty());
    let entitlements = world.store.entitlements(&account.id).await.unwrap();
    let game_id = entitlements[0].game_id.clone().unwrap();
    let entries = world.store.entries_for_game(&game_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, InstallState::Partial);
    assert!(entries[0].launch.is_none() && entries[0].location.is_none());
    assert_eq!(entries[0].source_id, source_id(&account.id));

    // The user's changes survive; a second refresh adds nothing.
    let mut game = world.store.game(&game_id).await.unwrap().unwrap();
    game.title = "My Portal".into();
    game.favorite = true;
    world.store.upsert_game(&game).await.unwrap();
    world
        .steam
        .owns(&[steam(620, "Portal 2"), steam(400, "Portal")]);
    let again = refresh(&world, &account).await;
    assert_eq!((again.added, again.lost), (0, 0));
    let kept = world.store.game(&game_id).await.unwrap().unwrap();
    assert!(kept.favorite && kept.title == "My Portal");
    assert_eq!(
        world
            .store
            .list_games(&ListGames::default())
            .await
            .unwrap()
            .total,
        2
    );

    let summary = &world.accounts.accounts().await.unwrap()[0];
    assert_eq!((summary.owned, summary.lost, summary.installed), (2, 0, 0));
    assert!(summary.account.sync.last_ok_ms.is_some() && summary.account.sync.error.is_none());
    assert_eq!(summary.capabilities.as_ref().unwrap().name, "STEAM");
    let owners = world.accounts.ownership(&game_id).await.unwrap();
    assert_eq!(owners[0].product_id, entitlements[0].product.product_id);
}

#[tokio::test]
async fn an_installed_copy_found_before_or_after_is_the_same_game() {
    let world = world().await;
    // Before: the refresh joins the game discovery made.
    let installed = discovered(&world.store, 620, "Portal 2").await;
    let account = connect(&world, "a").await;
    world
        .steam
        .owns(&[steam(620, "Portal 2"), steam(400, "Portal")]);
    let outcome = refresh(&world, &account).await;
    assert_eq!(outcome.added, 2);
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert_eq!(
        owned
            .iter()
            .find(|e| e.product.product_id == "620")
            .unwrap()
            .game_id,
        Some(installed.clone())
    );
    assert_eq!(
        world
            .store
            .entries_for_game(&installed)
            .await
            .unwrap()
            .len(),
        2
    );

    // After: discovery's own import makes a second game; reconciling
    // merges it into the owned one, keeping both releases.
    let portal = owned
        .iter()
        .find(|e| e.product.product_id == "400")
        .unwrap()
        .game_id
        .clone()
        .unwrap();
    let copy = discovered(&world.store, 400, "Portal").await;
    assert_ne!(copy, portal);
    assert_eq!(world.accounts.reconcile().await.unwrap(), 1);
    assert!(world.store.game(&copy).await.unwrap().is_none());
    assert_eq!(
        world.store.entries_for_game(&portal).await.unwrap().len(),
        2
    );
    assert_eq!(
        titles(&world.store, GameFilter::Installed).await,
        ["Portal", "Portal 2"]
    );
    let summary = &world.accounts.accounts().await.unwrap()[0];
    assert_eq!(summary.installed, 2);
    // Nothing more to merge.
    assert_eq!(world.accounts.reconcile().await.unwrap(), 0);
}

#[tokio::test]
async fn hidden_and_separated_games_stay_apart() {
    let world = world().await;
    let account = connect(&world, "a").await;
    world
        .steam
        .owns(&[steam(620, "Portal 2"), steam(400, "Portal")]);
    refresh(&world, &account).await;
    // A hidden installed copy isn't merged into the owned game.
    let hidden = discovered(&world.store, 620, "Portal 2").await;
    let mut game = world.store.game(&hidden).await.unwrap().unwrap();
    game.hidden = true;
    world.store.upsert_game(&game).await.unwrap();
    assert_eq!(world.accounts.reconcile().await.unwrap(), 0);
    assert!(world.store.game(&hidden).await.unwrap().is_some());
    // One the user separated stays separate.
    let copy = discovered(&world.store, 400, "Portal").await;
    world.accounts.reconcile().await.unwrap();
    let merged = world
        .store
        .entry_by_source("steam", "400")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(merged.game_id, copy);
    let apart = world
        .store
        .separate_entry(&merged.id, &[], &[])
        .await
        .unwrap();
    assert_eq!(world.accounts.reconcile().await.unwrap(), 0);
    assert_eq!(
        world
            .store
            .entry_by_source("steam", "400")
            .await
            .unwrap()
            .unwrap()
            .game_id,
        apart.id
    );
}

#[tokio::test]
async fn accounts_owning_the_same_game_keep_their_own_records() {
    let world = world().await;
    let (a, b) = (connect(&world, "a").await, connect(&world, "b").await);
    world.steam.owns(&[steam(620, "Portal 2")]);
    refresh(&world, &a).await;
    world.steam.owns(&[steam(620, "Portal 2")]);
    refresh(&world, &b).await;
    let game = world.store.entitlements(&a.id).await.unwrap()[0]
        .game_id
        .clone()
        .unwrap();
    assert_eq!(
        world.store.entitlements(&b.id).await.unwrap()[0].game_id,
        Some(game.clone())
    );
    assert_eq!(world.accounts.ownership(&game).await.unwrap().len(), 2);
    // A's library no longer lists it: only A lost it.
    world.steam.owns(&[]);
    assert_eq!(refresh(&world, &a).await.lost, 1);
    assert!(
        world.store.entitlements(&a.id).await.unwrap()[0]
            .lost_at_ms
            .is_some()
    );
    assert!(
        world.store.entitlements(&b.id).await.unwrap()[0]
            .lost_at_ms
            .is_none()
    );
    // Lost, it stays in the library; owned again, it's regained.
    assert_eq!(titles(&world.store, GameFilter::All).await, ["Portal 2"]);
    world.steam.owns(&[steam(620, "Portal 2")]);
    assert_eq!(refresh(&world, &a).await.regained, 1);
    // The same account connected again is the same record.
    assert_eq!(connect(&world, "a").await.id, a.id);
}

#[tokio::test]
async fn only_a_complete_refresh_marks_games_lost() {
    let world = world().await;
    let account = connect(&world, "a").await;
    world
        .steam
        .owns(&[steam(620, "Portal 2"), steam(400, "Portal")]);
    refresh(&world, &account).await;
    let before = world
        .accounts
        .account(&account.id)
        .await
        .unwrap()
        .sync
        .last_ok_ms;
    // A page failed: what came is recorded, nothing is lost.
    world.steam.then(Ok(LibrarySnapshot {
        products: vec![steam(620, "Portal 2")],
        complete: false,
    }));
    let partial = refresh(&world, &account).await;
    assert!(partial.partial && partial.lost == 0);
    let synced = world.accounts.account(&account.id).await.unwrap().sync;
    assert_eq!(synced.last_ok_ms, before);
    assert!(synced.error.is_some());
    // Offline, expired or cancelled: nothing changes but the error.
    world
        .steam
        .then(Err(ProviderError::Offline("no route".into())));
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        ErrorReport::from_error(&error).code,
        crate::error::ErrorCode::Offline
    );
    world
        .steam
        .then(Err(ProviderError::Expired("token revoked".into())));
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    let report = ErrorReport::from_error(&error);
    assert_eq!(report.code, crate::error::ErrorCode::Auth);
    assert!(report.hints.contains(&crate::error::RecoveryHint::SignIn));
    assert_eq!(
        world
            .accounts
            .account(&account.id)
            .await
            .unwrap()
            .sync
            .error
            .unwrap()
            .code,
        crate::error::ErrorCode::Auth
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        world.accounts.refresh(&account.id, &cancel).await,
        Err(RefreshError::Provider(ProviderError::Cancelled))
    ));
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert!(owned.iter().all(|e| e.lost_at_ms.is_none()) && owned.len() == 2);
}

#[tokio::test]
async fn refreshes_of_one_account_share_one_flight() {
    let world = world_with(Fake::slow("steam")).await;
    let account = connect(&world, "a").await;
    world.steam.owns(&[steam(620, "Portal 2")]);
    let (first_cancel, second_cancel) = (CancellationToken::new(), CancellationToken::new());
    let (first, second) = tokio::join!(world.accounts.refresh(&account.id, &first_cancel), async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        world.accounts.refresh(&account.id, &second_cancel).await
    });
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(world.steam.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_refresh_and_a_discovery_import_at_once_end_as_one_game() {
    let world = world_with(Fake::slow("steam")).await;
    let account = connect(&world, "a").await;
    world.steam.owns(&[steam(620, "Portal 2")]);
    let cancel = CancellationToken::new();
    let (refreshed, copy) = tokio::join!(world.accounts.refresh(&account.id, &cancel), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        discovered(&world.store, 620, "Portal 2").await
    });
    refreshed.unwrap();
    world.accounts.reconcile().await.unwrap();
    assert_eq!(titles(&world.store, GameFilter::All).await, ["Portal 2"]);
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert_eq!(owned[0].game_id, Some(copy));
}

#[tokio::test]
async fn disconnecting_keeps_the_library_and_removing_previews_first() {
    let world = world().await;
    let (a, b) = (connect(&world, "a").await, connect(&world, "b").await);
    world.steam.owns(&[
        steam(620, "Portal 2"),
        steam(400, "Portal"),
        steam(70, "Half-Life"),
    ]);
    refresh(&world, &a).await;
    world.steam.owns(&[steam(70, "Half-Life")]);
    refresh(&world, &b).await;
    discovered(&world.store, 620, "Portal 2").await;
    world.accounts.reconcile().await.unwrap();

    world.accounts.disconnect(&a.id).await.unwrap();
    assert_eq!(titles(&world.store, GameFilter::All).await.len(), 3);
    assert!(
        world
            .accounts
            .refresh(&a.id, &CancellationToken::new())
            .await
            .is_err()
    );

    let preview = world.accounts.removal_preview(&a.id).await.unwrap();
    let names = |games: &[GameRef]| games.iter().map(|g| g.title.clone()).collect::<Vec<_>>();
    assert_eq!(names(&preview.removed), ["Portal"]);
    assert_eq!(names(&preview.kept), ["Half-Life", "Portal 2"]);
    assert_eq!(
        titles(&world.store, GameFilter::All).await.len(),
        3,
        "a preview changes nothing"
    );
    world.accounts.remove(&a.id).await.unwrap();
    assert_eq!(
        titles(&world.store, GameFilter::All).await,
        ["Half-Life", "Portal 2"]
    );
    assert_eq!(
        titles(&world.store, GameFilter::Installed).await,
        ["Portal 2"]
    );
    assert!(world.store.entitlements(&a.id).await.unwrap().is_empty());
    assert_eq!(world.store.entitlements(&b.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_game_the_user_removed_isnt_brought_back() {
    let world = world().await;
    let account = connect(&world, "a").await;
    world.steam.owns(&[steam(620, "Portal 2")]);
    refresh(&world, &account).await;
    let game = world.store.entitlements(&account.id).await.unwrap()[0]
        .game_id
        .clone()
        .unwrap();
    world.store.delete_game(&game).await.unwrap();
    world.steam.owns(&[steam(620, "Portal 2")]);
    refresh(&world, &account).await;
    assert!(titles(&world.store, GameFilter::All).await.is_empty());
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert_eq!((owned.len(), owned[0].entry_id.clone()), (1, None));
}

/// An achievements-only service: connected like a store, no library.
struct AchievementsOnly;

#[async_trait]
impl AccountProvider for AchievementsOnly {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "ra".into(),
            name: "RA".into(),
            auth: AuthKind::Launcher,
            library: false,
            playtime: false,
            install: false,
        }
    }
    async fn begin_auth(
        &self,
        _flow_id: &str,
    ) -> std::result::Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge::default())
    }
    async fn complete_auth(
        &self,
        _flow_id: &str,
        _input: &AuthInput,
    ) -> std::result::Result<SignedIn, ProviderError> {
        Err(ProviderError::Other("not used".into()))
    }
    async fn library(
        &self,
        _account: &StoreAccount,
        _credentials: Option<&StoreCredentials>,
        _cancel: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError> {
        panic!("an account without a library is never listed")
    }
}

/// Answers each refresh with the next scripted answer.
#[derive(Default)]
struct Refreshes(Mutex<VecDeque<std::result::Result<(), ProviderError>>>);

#[async_trait]
impl AccountRefresher for Refreshes {
    async fn refresh_account(
        &self,
        _account: &StoreAccount,
        _cancel: &CancellationToken,
    ) -> std::result::Result<(), ProviderError> {
        crate::lock(&self.0)
            .pop_front()
            .expect("a scripted refresh")
    }
}

#[tokio::test]
async fn an_account_without_a_library_refreshes_through_the_refresher() {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(Arc::new(AchievementsOnly));
    let account = accounts.connect("ra", "1", "Player", None).await.unwrap();
    let cancel = CancellationToken::new();
    // Without a refresher there is nothing to do.
    assert_eq!(
        accounts.refresh(&account.id, &cancel).await.unwrap(),
        RefreshOutcome::default()
    );
    assert_eq!(
        accounts.account(&account.id).await.unwrap().sync.last_ok_ms,
        None
    );

    let refreshes = Arc::new(Refreshes::default());
    accounts.set_refresher(refreshes.clone());
    crate::lock(&refreshes.0).push_back(Err(ProviderError::Offline("no network".into())));
    assert!(matches!(
        accounts.refresh(&account.id, &cancel).await,
        Err(RefreshError::Provider(ProviderError::Offline(_)))
    ));
    let sync = accounts.account(&account.id).await.unwrap().sync;
    assert_eq!((sync.failures, sync.last_ok_ms), (1, None));
    assert!(sync.error.is_some());

    crate::lock(&refreshes.0).push_back(Ok(()));
    accounts.refresh(&account.id, &cancel).await.unwrap();
    let sync = accounts.account(&account.id).await.unwrap().sync;
    assert_eq!(sync.failures, 0);
    assert!(sync.last_ok_ms.is_some() && sync.error.is_none());
}
