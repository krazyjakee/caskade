//! Controller operations, with no client-authored discovery records.
use crate::discovery::{DiscoveryController, DiscoverySnapshot};
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn discovery_status(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, String> {
    Ok(controller.snapshot().await)
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_launchers(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, String> {
    controller.launchers().await
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_scan(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, String> {
    controller.scan().await
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_cancel(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, String> {
    Ok(controller.cancel().await)
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_review(
    controller: State<'_, DiscoveryController>,
    ids: Vec<String>,
    accept: bool,
    revision: u32,
) -> Result<DiscoverySnapshot, String> {
    controller.review(ids, accept, revision).await
}
