//! Achievements against scripted providers and accounts (#185).

use super::*;
use caskade_core::{
    accounts::{
        AccountAccess as LocalAccess, AccountRole, AccountService, NewAccount, Permissions,
    },
    model::{Game, GameEntry, SourceId},
    store_accounts::{StoreCredentials, SyncState},
};
use caskade_discovery::ExternalId;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU32, Ordering},
};

/// A service that answers each fetch with the next scripted answer, and
/// knows games by one kind of store id.
struct Fake {
    id: &'static str,
    answers: Mutex<VecDeque<std::result::Result<Fetched, FetchError>>>,
    calls: AtomicU32,
}
impl Fake {
    fn new(id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            id,
            answers: Mutex::default(),
            calls: AtomicU32::new(0),
        })
    }
    fn then(&self, answer: std::result::Result<Fetched, FetchError>) {
        caskade_core::lock(&self.answers).push_back(answer);
    }
    fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}
#[async_trait::async_trait]
impl AchievementProvider for Fake {
    fn id(&self) -> String {
        self.id.into()
    }
    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities {
            unlock_times: true,
            hardcore: self.id == "retroachievements",
            ..Default::default()
        }
    }
    fn product(&self, id: &ExternalId) -> Option<String> {
        match (self.id, id) {
            ("steam", ExternalId::Steam { app_id }) => Some(app_id.to_string()),
            ("gog", ExternalId::Gog { product_id }) => Some(product_id.to_string()),
            _ => None,
        }
    }
    async fn fetch(
        &self,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        _product: &str,
        _cancel: &CancellationToken,
    ) -> std::result::Result<Fetched, FetchError> {
        assert!(credentials.is_some(), "a fetch gets the sign-in");
        self.calls.fetch_add(1, Ordering::SeqCst);
        caskade_core::lock(&self.answers)
            .pop_front()
            .unwrap_or_else(|| {
                Err(FetchError::Provider(ProviderError::Other(
                    "no answer".into(),
                )))
            })
    }
}

/// Store accounts from the library, each signed in unless listed.
struct Access {
    store: LibraryStore,
    signed_out: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl AccountAccess for Access {
    async fn accounts(&self) -> Result<Vec<StoreAccount>> {
        self.store.store_accounts().await
    }
    async fn credentials(
        &self,
        account: &StoreAccount,
    ) -> std::result::Result<Option<StoreCredentials>, ProviderError> {
        if caskade_core::lock(&self.signed_out).contains(&account.id) {
            return Err(ProviderError::Expired("signed out".into()));
        }
        Ok(Some(StoreCredentials {
            access_token: "token".into(),
            refresh_token: None,
            expires_at_ms: None,
            extra: Default::default(),
        }))
    }
    fn provider_name(&self, provider: &str) -> Option<String> {
        Some(provider.to_uppercase())
    }
}

struct Setup {
    store: LibraryStore,
    bus: Bus,
    service: Achievements,
    access: Arc<Access>,
}
async fn setup(providers: &[Arc<Fake>]) -> Setup {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let access = Arc::new(Access {
        store: store.clone(),
        signed_out: Mutex::default(),
    });
    let service = Achievements::new(store.clone(), bus.clone(), access.clone());
    for provider in providers {
        service.register_provider(provider.clone());
    }
    Setup {
        store,
        bus,
        service,
        access,
    }
}
async fn account(
    store: &LibraryStore,
    provider: &str,
    external: &str,
    owner: Option<&str>,
) -> StoreAccount {
    let account = StoreAccount {
        id: caskade_core::model::new_id(),
        provider: provider.into(),
        external_id: external.into(),
        display_name: format!("{provider} {external}"),
        owner_id: owner.map(str::to_owned),
        enabled: true,
        sync: SyncState::default(),
        created_at_ms: 0,
    };
    store.save_store_account(&account).await.unwrap();
    account
}
/// A game with one release per store id.
async fn game(store: &LibraryStore, title: &str, ids: &[ExternalId]) -> String {
    let game = Game::new(title);
    let entries: Vec<GameEntry> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let mut entry = GameEntry::new(
                &game.id,
                SourceId::new("fixture"),
                format!("{title}-{index}"),
            );
            entry.ids = vec![id.clone()];
            entry
        })
        .collect();
    store
        .import_group(&game, &entries, None)
        .await
        .unwrap()
        .game_id
}
fn definition(id: &str, hidden: bool) -> AchievementDefinition {
    AchievementDefinition {
        id: id.into(),
        name: format!("Name {id}"),
        description: Some(format!("Do {id}")),
        hidden,
        icon_url: None,
        locked_icon_url: None,
        progress_max: None,
        rarity_percent: None,
        points: None,
    }
}
fn unlocked(at: i64) -> AchievementState {
    AchievementState {
        unlocked: true,
        unlocked_at_ms: Some(at),
        ..Default::default()
    }
}
fn answer(
    ids: &[(&str, bool)],
    unlocks: &[(&str, i64)],
    complete: bool,
) -> std::result::Result<Fetched, FetchError> {
    Ok(Fetched {
        definitions: ids
            .iter()
            .map(|(id, hidden)| definition(id, *hidden))
            .collect(),
        states: unlocks
            .iter()
            .map(|(id, at)| ((*id).to_owned(), unlocked(*at)))
            .collect(),
        complete,
    })
}
const STEAM: ExternalId = ExternalId::Steam { app_id: 620 };

#[tokio::test]
async fn a_new_unlock_after_the_baseline_is_announced_and_a_partial_answer_relocks_nothing() {
    let steam = Fake::new("steam");
    let s = setup(std::slice::from_ref(&steam)).await;
    let mut events = s.bus.stream();
    account(&s.store, "steam", "76561", None).await;
    let game = game(&s.store, "Portal 2", &[STEAM]).await;
    // Not fetched yet: a set with nothing, distinct from an empty list.
    let sets = s.service.sets(&game, None).await.unwrap();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].sync.status, SetStatus::NotFetched);
    assert_eq!(sets[0].product_id, "620");

    steam.then(answer(&[("a", false), ("b", true)], &[("a", 10)], true));
    let sets = s
        .service
        .refresh_game(&game, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(sets[0].sync.status, SetStatus::Ready);
    assert_eq!((sets[0].unlocked, sets[0].total), (1, 2));
    assert!(sets[0].achievements[1].definition.hidden);
    // The first fetch is history, never "new".
    let mut unlocked_events = 0;
    while let Ok(event) = events.try_recv() {
        unlocked_events += matches!(event, Event::AchievementsUnlocked(_)) as u32;
    }
    assert_eq!(unlocked_events, 0);

    // The same answer again changes nothing; then b unlocks.
    steam.then(answer(&[("a", false), ("b", true)], &[("a", 10)], true));
    s.service
        .refresh_game(&game, None, &CancellationToken::new())
        .await
        .unwrap();
    steam.then(answer(
        &[("a", false), ("b", true)],
        &[("a", 10), ("b", 20)],
        true,
    ));
    s.service
        .refresh_game(&game, None, &CancellationToken::new())
        .await
        .unwrap();
    let mut announced = vec![];
    while let Ok(event) = events.try_recv() {
        if let Event::AchievementsUnlocked(new) = event {
            announced.push(new.names.clone());
        }
    }
    assert_eq!(announced, [vec!["Name b".to_owned()]]);

    // An incomplete answer that only lists a, locked: nothing relocks or goes.
    steam.then(answer(&[("a", false)], &[], false));
    let sets = s
        .service
        .refresh_game(&game, None, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!((sets[0].unlocked, sets[0].total), (2, 2));
    assert_eq!(sets[0].achievements[0].state.unlocked_at_ms, Some(10));
    assert!(sets[0].sync.error.is_some(), "an incomplete answer says so");
    assert_eq!(steam.calls(), 4);
}

#[tokio::test]
async fn each_account_and_store_keeps_its_own_set_and_viewers_see_their_own() {
    let steam = Fake::new("steam");
    let gog = Fake::new("gog");
    let s = setup(&[steam.clone(), gog.clone()]).await;
    let local = AccountService::open(s.store.clone(), s.bus.clone())
        .await
        .unwrap();
    let admin = local.signed_in().unwrap();
    // Standard accounts need the admin to have a PIN.
    local.set_pin(&admin.id, Some("2468".into())).await.unwrap();
    let kid = local
        .create(NewAccount {
            name: "Kid".into(),
            avatar: caskade_core::accounts::DEFAULT_AVATAR.into(),
            access: LocalAccess {
                role: AccountRole::Standard,
                permissions: Permissions::default(),
                max_age: None,
                unrated_needs_pin: false,
            },
            pin: None,
        })
        .await
        .unwrap();
    let mine = account(&s.store, "steam", "1", Some(&admin.id)).await;
    let theirs = account(&s.store, "steam", "2", Some(&kid.id)).await;
    account(&s.store, "gog", "3", None).await;
    // One game with a Steam release and a GOG release.
    let game = game(
        &s.store,
        "Witcher",
        &[STEAM, ExternalId::Gog { product_id: 42 }],
    )
    .await;
    let all = s.service.sets(&game, None).await.unwrap();
    assert_eq!(all.len(), 3, "two Steam accounts and one GOG account");
    let admin_sees: Vec<_> = s
        .service
        .sets(&game, Some(&admin.id))
        .await
        .unwrap()
        .into_iter()
        .map(|set| (set.account.store_account_id, set.product_id))
        .collect();
    assert!(admin_sees.contains(&(mine.id.clone(), "620".into())));
    assert!(!admin_sees.iter().any(|(id, _)| *id == theirs.id));
    assert_eq!(admin_sees.len(), 2);
    // Fetching one account's set leaves the others alone.
    steam.then(answer(&[("a", false)], &[("a", 1)], true));
    gog.then(answer(&[("x", false), ("y", false)], &[], true));
    let fetched = s
        .service
        .refresh_game(&game, Some(&admin.id), &CancellationToken::new())
        .await
        .unwrap();
    let counts: Vec<_> = fetched
        .iter()
        .map(|set| (set.account.provider.clone(), set.unlocked, set.total))
        .collect();
    assert!(counts.contains(&("steam".into(), 1, 1)));
    assert!(counts.contains(&("gog".into(), 0, 2)));
    let kid_set = s.service.sets(&game, Some(&kid.id)).await.unwrap();
    let kid_steam = kid_set
        .iter()
        .find(|set| set.account.provider == "steam")
        .unwrap();
    assert_eq!(kid_steam.sync.status, SetStatus::NotFetched);
    let summary = s.service.summary(&game, Some(&admin.id)).await.unwrap();
    assert_eq!((summary.sets, summary.unlocked, summary.total), (2, 1, 3));
}

#[tokio::test]
async fn failures_keep_what_was_known_and_say_why() {
    let steam = Fake::new("steam");
    let s = setup(std::slice::from_ref(&steam)).await;
    let steam_account = account(&s.store, "steam", "1", None).await;
    let game = game(&s.store, "Portal", &[STEAM]).await;
    let cancel = CancellationToken::new();
    let refresh = || s.service.refresh_game(&game, None, &cancel);
    steam.then(answer(&[("a", false)], &[("a", 5)], true));
    refresh().await.unwrap();
    // Offline: the last good answer stays, marked stale.
    steam.then(Err(ProviderError::Offline("no network".into()).into()));
    let set = refresh().await.unwrap().remove(0);
    assert_eq!(set.sync.status, SetStatus::Ready);
    assert_eq!((set.unlocked, set.total), (1, 1));
    assert_eq!(set.sync.failures, 1);
    assert!(set.sync.error.is_some());
    // Rate-limited: no fetch until the service's wait is over.
    steam.then(Err(FetchError::Provider(ProviderError::RateLimited {
        retry_after: Some(Duration::from_secs(600)),
    })));
    let set = refresh().await.unwrap().remove(0);
    assert!(
        set.sync
            .retry_at_ms
            .is_some_and(|at| at > caskade_core::now_ms())
    );
    let calls = steam.calls();
    refresh().await.unwrap();
    assert_eq!(steam.calls(), calls, "the wait is kept");

    // Private, unsupported, no achievements and signed out are distinct.
    let cases: [(std::result::Result<Fetched, FetchError>, SetStatus); 3] = [
        (
            Err(ProviderError::Unavailable("private".into()).into()),
            SetStatus::Private,
        ),
        (
            Err(FetchError::Unsupported("no stats".into())),
            SetStatus::Unsupported,
        ),
        (answer(&[], &[], true), SetStatus::NoAchievements),
    ];
    for (index, (result, status)) in cases.into_iter().enumerate() {
        let product = 1000 + index as u32;
        let other = game_with(&s.store, product).await;
        steam.then(result);
        let set = s
            .service
            .refresh_game(&other, None, &CancellationToken::new())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(set.sync.status, status, "case {index}");
    }
    let signed_out = game_with(&s.store, 2000).await;
    caskade_core::lock(&s.access.signed_out).push(steam_account.id.clone());
    let set = s
        .service
        .refresh_game(&signed_out, None, &CancellationToken::new())
        .await
        .unwrap()
        .remove(0);
    assert_eq!(set.sync.status, SetStatus::SignInNeeded);
    // A disconnected account isn't asked at all.
    let mut disconnected = steam_account.clone();
    disconnected.enabled = false;
    s.store.save_store_account(&disconnected).await.unwrap();
    let calls = steam.calls();
    let set = s
        .service
        .refresh_game(&signed_out, None, &CancellationToken::new())
        .await
        .unwrap()
        .remove(0);
    assert_eq!(set.sync.status, SetStatus::SignInNeeded);
    assert_eq!(steam.calls(), calls);
    // A cancelled fetch records nothing.
    caskade_core::lock(&s.access.signed_out).clear();
    s.store.save_store_account(&steam_account).await.unwrap();
    let fresh = game_with(&s.store, 3000).await;
    steam.then(Err(ProviderError::Cancelled.into()));
    let set = s
        .service
        .refresh_game(&fresh, None, &CancellationToken::new())
        .await
        .unwrap()
        .remove(0);
    assert_eq!(set.sync.status, SetStatus::NotFetched);
    assert_eq!(set.sync.failures, 0);
}

async fn game_with(store: &LibraryStore, app_id: u32) -> String {
    game(
        store,
        &format!("Game {app_id}"),
        &[ExternalId::Steam { app_id }],
    )
    .await
}

#[tokio::test]
async fn sets_follow_releases_through_merges_separations_and_restarts() {
    let steam = Fake::new("steam");
    let ra = Fake::new("retroachievements");
    let s = setup(&[steam.clone(), ra.clone()]).await;
    account(&s.store, "steam", "1", None).await;
    account(&s.store, "retroachievements", "me", None).await;
    let pc = game(&s.store, "Doom", &[STEAM]).await;
    let console = game(
        &s.store,
        "Doom (SNES)",
        &[ExternalId::Gog { product_id: 7 }],
    )
    .await;
    // The SNES release is linked to its RetroAchievements game.
    let snes = s.store.entries_for_game(&console).await.unwrap().remove(0);
    s.store
        .save_achievement_link(&AchievementLink {
            entry_id: snes.id.clone(),
            provider: "retroachievements".into(),
            product_id: "1234".into(),
            method: LinkMethod::Hash { hash: "abc".into() },
            linked_at_ms: 0,
        })
        .await
        .unwrap();
    steam.then(answer(&[("a", false)], &[("a", 1)], true));
    ra.then(Ok(Fetched {
        definitions: vec![definition("1", false)],
        states: [(
            "1".to_owned(),
            AchievementState {
                unlocked: true,
                unlocked_at_ms: Some(2),
                hardcore: Some(HardcoreUnlock {
                    unlocked: false,
                    unlocked_at_ms: None,
                }),
                ..Default::default()
            },
        )]
        .into(),
        complete: true,
    }));
    s.service
        .refresh_game(&pc, None, &CancellationToken::new())
        .await
        .unwrap();
    let ra_set = s
        .service
        .refresh_game(&console, None, &CancellationToken::new())
        .await
        .unwrap()
        .remove(0);
    assert_eq!(ra_set.hardcore_unlocked, Some(0));
    assert_eq!(ra_set.unlocked, 1);
    // Merged: one game, both sets, still apart.
    s.store.merge_games(&pc, &console).await.unwrap();
    let merged = s.service.sets(&pc, None).await.unwrap();
    assert_eq!(merged.len(), 2);
    assert!(merged.iter().all(|set| set.total == 1 && set.unlocked == 1));
    // Separated: the SNES release takes its set along.
    let separated = s.store.separate_entry(&snes.id, &[], &[]).await.unwrap().id;
    let back = s.service.sets(&separated, None).await.unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].account.provider, "retroachievements");
    assert_eq!(s.service.sets(&pc, None).await.unwrap().len(), 1);
    // A restart (a new service on the same library) answers from the cache.
    let again = Achievements::new(s.store.clone(), s.bus.clone(), s.access.clone());
    again.register_provider(steam.clone());
    let calls = steam.calls();
    let cached = again.sets(&pc, None).await.unwrap().remove(0);
    assert_eq!(
        (cached.unlocked, cached.total, cached.sync.status),
        (1, 1, SetStatus::Ready)
    );
    assert_eq!(steam.calls(), calls);
    // Removing the link removes the set from the game.
    assert!(
        s.store
            .delete_achievement_link(&snes.id, "retroachievements")
            .await
            .unwrap()
    );
    assert!(s.service.sets(&separated, None).await.unwrap().is_empty());
}
