//! Sync engines exchanging changes directly, without a network.

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::sync::SyncEngine;

/// A library and its engine.
pub struct Device {
    pub engine: SyncEngine,
    pub store: LibraryStore,
}

/// A device with an empty library, its id `name` repeated.
pub async fn device(name: char) -> Device {
    let store = LibraryStore::in_memory().await.unwrap();
    AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    Device {
        engine: SyncEngine::new(store.clone(), name.to_string().repeat(64)),
        store,
    }
}

/// Two rounds of each engine capturing its changes and sending the other
/// what it hasn't seen.
pub async fn exchange(a: &SyncEngine, b: &SyncEngine) {
    for _ in 0..2 {
        for (from, to) in [(a, b), (b, a)] {
            from.capture().await.unwrap();
            loop {
                let cursor = to.seen().await.unwrap();
                let changes = from.changes_for(&cursor, 100).await.unwrap();
                if changes.is_empty() {
                    break;
                }
                to.receive(&changes).await.unwrap();
            }
        }
    }
}
