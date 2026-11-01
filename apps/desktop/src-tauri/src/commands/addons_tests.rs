use super::*;
use crate::commands::test_ipc::invoke;
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

const CATALOGUE: &[u8] =
    include_bytes!("../../../../../crates/caskade-core/src/addons/testdata/catalogue.json");
const SIGNATURE: &[u8] =
    include_bytes!("../../../../../crates/caskade-core/src/addons/testdata/catalogue.json.sig");
const KEY: &str =
    include_str!("../../../../../crates/caskade-core/src/addons/testdata/test-signing.key.pub");

#[test]
fn the_catalogue_reaches_the_webview_typed_and_offline() {
    let temp = tempfile::tempdir().unwrap();
    let service = CatalogueService::new(
        temp.path(),
        "https://example.org/addons/catalogue.json",
        Some(KEY),
    )
    .unwrap();
    tauri::async_runtime::block_on(service.accept(CATALOGUE, Some(SIGNATURE), 42)).unwrap();
    let app = mock_builder()
        .manage(service)
        .manage(AddonMedia(None))
        .invoke_handler(tauri::generate_handler![addons_catalogue, addons_media])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let view = invoke(&window, "addons_catalogue", json!({"mode": "console"})).unwrap();
    assert_eq!(view["status"]["fetchedAtMs"], 42);
    assert_eq!(view["status"]["verified"], true);
    let items = view["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let stats = &items[0];
    assert_eq!(stats["entry"]["kind"], "plugin");
    assert_eq!(stats["compatibility"]["state"], "available");
    assert_eq!(stats["compatibility"]["release"]["version"], "0.1.0");
    assert_eq!(stats["compatibility"]["reasons"][0]["code"], "mode");
    // Without an image cache, images just don't load.
    let media = invoke(
        &window,
        "addons_media",
        json!({"id": "dev.caskade.playtime-stats", "index": 0}),
    )
    .unwrap();
    assert!(media.is_null());
}
