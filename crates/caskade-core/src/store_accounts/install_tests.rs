//! Installing owned games through a fake launcher (#170).

use super::super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec},
};
use caskade_discovery::ExternalId;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex as StdMutex},
};

/// A store whose games install through the `fake` launcher.
struct Store;

#[async_trait]
impl AccountProvider for Store {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "fake".into(),
            name: "Fake Store".into(),
            auth: AuthKind::Launcher,
            library: true,
            playtime: false,
            install: true,
        }
    }
    fn install_target(&self, product: &OwnedProduct) -> Option<InstallTarget> {
        Some(InstallTarget {
            launcher: "fake".into(),
            name: "Fake Launcher".into(),
            uri: format!("steam://install/{}", product.product_id),
        })
    }
    async fn begin_auth(&self, _: &str) -> std::result::Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge::default())
    }
    async fn complete_auth(
        &self,
        _: &str,
        _: &AuthInput,
    ) -> std::result::Result<SignedIn, ProviderError> {
        Err(ProviderError::Other("unused".into()))
    }
    async fn library(
        &self,
        _: &StoreAccount,
        _: Option<&StoreCredentials>,
        _: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError> {
        Ok(LibrarySnapshot {
            products: vec![
                product("620", "Portal 2", vec![]),
                product("9", "Windows Elsewhere", vec!["plan9".into()]),
            ],
            complete: true,
        })
    }
}

fn product(id: &str, title: &str, systems: Vec<String>) -> OwnedProduct {
    OwnedProduct {
        product_id: id.into(),
        title: title.into(),
        ids: vec![ExternalId::Steam {
            app_id: id.parse().unwrap(),
        }],
        platforms: vec![],
        playtime_minutes: None,
        last_played_ms: None,
        systems,
    }
}

struct World {
    store: LibraryStore,
    accounts: StoreAccounts,
    opened: Arc<StdMutex<Vec<String>>>,
    launchers: BTreeSet<String>,
    game: String,
    account: StoreAccount,
}

async fn world() -> World {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(Arc::new(Store));
    let opened = Arc::new(StdMutex::new(Vec::new()));
    accounts.set_opener(Arc::new({
        let opened = opened.clone();
        move |uri: &str| {
            opened.lock().unwrap().push(uri.to_owned());
            Ok(())
        }
    }));
    let account = accounts.connect("fake", "me", "Me", None).await.unwrap();
    accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    let game = store
        .entitlements(&account.id)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.product.product_id == "620")
        .unwrap()
        .game_id
        .unwrap();
    World {
        store,
        accounts,
        opened,
        launchers: BTreeSet::from(["fake".to_owned()]),
        game,
        account,
    }
}

impl World {
    async fn release(&self) -> OwnedRelease {
        self.release_of(&self.game).await
    }
    async fn release_of(&self, game: &str) -> OwnedRelease {
        self.accounts
            .owned_releases(game, &self.launchers)
            .await
            .unwrap()
            .remove(0)
    }
    /// The launcher finished: Steam discovery finds the copy.
    async fn discovered(&self, state: InstallState) -> GameEntry {
        let game = Game::new("Portal 2");
        let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "620");
        entry.ids = vec![ExternalId::Steam { app_id: 620 }];
        entry.state = state;
        entry.location = Some(InstallLocation::External {
            path: "/games/portal2".into(),
        });
        entry.launch = Some(LaunchSpec::Uri {
            uri: "steam://rungameid/620".into(),
        });
        self.store
            .import_group(&game, &[entry.clone()], None)
            .await
            .unwrap();
        self.accounts.reconcile().await.unwrap();
        self.store
            .entry_by_source("steam", "620")
            .await
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn an_owned_game_installs_through_its_launcher_and_is_done_when_discovery_says() {
    let w = world().await;
    let release = w.release().await;
    assert_eq!(
        release.state,
        OwnedInstallState::Available {
            launcher: "Fake Launcher".into()
        }
    );
    assert_eq!(
        (
            release.provider_name.as_str(),
            release.account_name.as_str()
        ),
        ("Fake Store", "Me")
    );
    assert!(release.refreshed_at_ms.is_some());

    let handed = w
        .accounts
        .install(&release.entry_id, &w.launchers)
        .await
        .unwrap();
    assert!(matches!(handed.state, OwnedInstallState::Installing { .. }));
    assert_eq!(*w.opened.lock().unwrap(), ["steam://install/620"]);
    // Nothing is installed until discovery finds it.
    assert!(matches!(
        w.release().await.state,
        OwnedInstallState::Installing { .. }
    ));
    let installed = w.discovered(InstallState::Installed).await;
    assert_eq!(installed.game_id, w.game);
    assert_eq!(w.release().await.state, OwnedInstallState::Installed);
    // Launchable now; the owned release isn't.
    let entries = w.store.entries_for_game(&w.game).await.unwrap();
    assert_eq!(entries.iter().filter(|e| e.launch.is_some()).count(), 1);

    // Uninstalled through the launcher: discovery marks the copy absent.
    let mut gone = installed.clone();
    gone.state = InstallState::Partial;
    w.store.upsert_entry(&gone).await.unwrap();
    assert_eq!(
        w.release().await.state,
        OwnedInstallState::Available {
            launcher: "Fake Launcher".into()
        }
    );
    assert_eq!(w.store.entitlements(&w.account.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_missing_launcher_an_unsupported_system_and_lost_access_are_distinct() {
    let w = world().await;
    let none = BTreeSet::new();
    assert_eq!(
        w.accounts.owned_releases(&w.game, &none).await.unwrap()[0].state,
        OwnedInstallState::LauncherMissing {
            launcher: "Fake Launcher".into()
        }
    );
    assert!(
        w.accounts
            .install(&w.release().await.entry_id, &none)
            .await
            .is_err()
    );
    let elsewhere = w
        .store
        .entitlements(&w.account.id)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.product.product_id == "9")
        .unwrap()
        .game_id
        .unwrap();
    assert_eq!(
        w.release_of(&elsewhere).await.state,
        OwnedInstallState::UnsupportedSystem {
            systems: vec!["plan9".into()]
        }
    );
    // Disconnected, then connected again while the page is open.
    w.accounts.disconnect(&w.account.id).await.unwrap();
    assert_eq!(
        w.release().await.state,
        OwnedInstallState::Unavailable {
            reason: UnavailableReason::Disconnected
        }
    );
    w.accounts.connect("fake", "me", "Me", None).await.unwrap();
    assert!(matches!(
        w.release().await.state,
        OwnedInstallState::Available { .. }
    ));
    assert!(w.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_or_cancelled_handoff_leaves_ownership_as_it_was() {
    let w = world().await;
    w.accounts.set_opener(Arc::new(|_: &str| {
        Err(crate::Error::Invalid("no handler for steam:".into()))
    }));
    let release = w.release().await;
    assert!(
        w.accounts
            .install(&release.entry_id, &w.launchers)
            .await
            .is_err()
    );
    assert!(matches!(
        w.release().await.state,
        OwnedInstallState::Available { .. }
    ));
    w.accounts.set_opener(Arc::new(|_: &str| Ok(())));
    w.accounts
        .install(&release.entry_id, &w.launchers)
        .await
        .unwrap();
    w.accounts.cancel_install(&release.entry_id);
    assert!(matches!(
        w.release().await.state,
        OwnedInstallState::Available { .. }
    ));
    assert_eq!(w.store.entitlements(&w.account.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn each_account_offers_its_release_and_the_default_stays() {
    let w = world().await;
    let other = w
        .accounts
        .connect("fake", "sibling", "Sibling", None)
        .await
        .unwrap();
    w.accounts
        .refresh(&other.id, &CancellationToken::new())
        .await
        .unwrap();
    let releases = w
        .accounts
        .owned_releases(&w.game, &w.launchers)
        .await
        .unwrap();
    let names: Vec<_> = releases.iter().map(|r| r.account_name.as_str()).collect();
    assert_eq!(names, ["Me", "Sibling"]);
    // The user's default release isn't moved by installs.
    let installed = w.discovered(InstallState::Installed).await;
    let mut game = w.store.game(&w.game).await.unwrap().unwrap();
    game.default_entry = Some(installed.id.clone());
    w.store.upsert_game(&game).await.unwrap();
    w.accounts
        .refresh(&w.account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        w.store.game(&w.game).await.unwrap().unwrap().default_entry,
        Some(installed.id)
    );
}

#[cfg(feature = "net")]
#[test]
fn launcher_uris_open_the_install_in_each_store() {
    use crate::store_accounts::{epic::EpicProvider, gog::GogProvider, steam::SteamProvider};
    let target = |provider: &dyn AccountProvider, product: OwnedProduct| {
        provider.install_target(&product).unwrap()
    };
    assert_eq!(
        target(&SteamProvider::new(), product("620", "P", vec![])).uri,
        "steam://install/620"
    );
    let mut gog = product("1", "G", vec![]);
    gog.product_id = "1207666393".into();
    assert_eq!(
        target(&GogProvider::new(), gog).uri,
        "goggalaxy://openGameView/1207666393"
    );
    let mut epic = product("1", "E", vec![]);
    epic.ids = vec![ExternalId::Epic {
        namespace: "fn".into(),
        catalog_item: "4fe7".into(),
        app_name: "Fortnite".into(),
    }];
    let uri = target(&EpicProvider::new(), epic).uri;
    assert_eq!(
        uri,
        "com.epicgames.launcher://apps/fn%3A4fe7%3AFortnite?action=install"
    );
    // Every one of them is a launcher URI the system handler may open.
    for uri in [
        uri.as_str(),
        "steam://install/620",
        "goggalaxy://openGameView/1",
    ] {
        crate::launch::uri_command(uri).unwrap();
    }
}

#[tokio::test]
async fn ready_to_install_lists_owned_games_until_a_copy_is_installed() {
    use crate::model::{GameFilter, ListGames};
    let w = world().await;
    let owned = |w: &World| {
        let store = w.store.clone();
        async move {
            store
                .list_games(&ListGames {
                    filter: GameFilter::Owned,
                    ..ListGames::default()
                })
                .await
                .unwrap()
                .total
        }
    };
    assert_eq!(owned(&w).await, 2);
    assert_eq!(w.store.library_stats().await.unwrap().owned, 2);
    w.discovered(InstallState::Installed).await;
    assert_eq!(owned(&w).await, 1);
    assert_eq!(w.store.library_stats().await.unwrap().owned, 1);
}
