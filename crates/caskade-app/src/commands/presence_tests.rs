use super::*;
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    devices::{MemoryKeyStore, Reach},
};

/// Drops what reaches the UI.
struct Ignore;

impl crate::events::Emit for Ignore {
    fn emit(&self, _event: Event) -> Result<(), String> {
        Ok(())
    }
}

/// Nothing runs elsewhere with no paired devices, and the answer comes from
/// memory at once.
#[tokio::test]
async fn devices_playing_answers_from_memory() {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        std::sync::Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    // What's running elsewhere goes to the UI as the app sends it
    // (`crate::start`).
    let presence = Presence::new(store.clone());
    setup(
        Events::new(Ignore),
        &presence,
        devices.clone(),
        store.clone(),
    );
    let started = std::time::Instant::now();
    assert_eq!(
        devices_playing(&presence, &devices, &store).await.unwrap(),
        Vec::new()
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    store.close().await;
}
