use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    devices::{MemoryKeyStore, Reach},
};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

/// Nothing runs elsewhere with no paired devices, and the answer comes from
/// memory at once.
#[test]
fn devices_playing_answers_from_memory_through_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let accounts =
        tauri::async_runtime::block_on(AccountService::open(store.clone(), Bus::default()))
            .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        std::sync::Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    let app = mock_builder()
        .manage(store.clone())
        .manage(devices.clone())
        .invoke_handler(tauri::generate_handler![devices_playing])
        .build(mock_context(noop_assets()))
        .unwrap();
    setup(app.handle(), Presence::new(store.clone()), devices);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        invoke(&window, "devices_playing", json!({})).unwrap(),
        json!([])
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    tauri::async_runtime::block_on(store.close());
}
