//! Controller operations, with no client-authored discovery records.
use crate::discovery::{DiscoveryController, DiscoverySnapshot};
use crate::error::CommandError;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn discovery_status(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    Ok(controller.snapshot().await)
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_launchers(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    controller.launchers().await
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_scan(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    controller.scan().await
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_cancel(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    Ok(controller.cancel().await)
}
#[tauri::command]
#[specta::specta]
pub async fn discovery_review(
    controller: State<'_, DiscoveryController>,
    ids: Vec<String>,
    accept: bool,
    revision: u32,
) -> Result<DiscoverySnapshot, CommandError> {
    controller.review(ids, accept, revision).await
}
