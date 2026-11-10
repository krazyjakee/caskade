//! Controller operations, with no client-authored discovery records.
use crate::discovery::{DiscoveryController, DiscoverySnapshot};
use crate::error::CommandError;
use caskade_app::commands::discovery as app;
use tauri::State;
#[tauri::command]
#[specta::specta]
pub async fn discovery_status(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    app::discovery_status(&controller).await
}

#[tauri::command]
#[specta::specta]
pub async fn discovery_launchers(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    app::discovery_launchers(&controller).await
}

#[tauri::command]
#[specta::specta]
pub async fn discovery_scan(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    app::discovery_scan(&controller).await
}

#[tauri::command]
#[specta::specta]
pub async fn discovery_cancel(
    controller: State<'_, DiscoveryController>,
) -> Result<DiscoverySnapshot, CommandError> {
    app::discovery_cancel(&controller).await
}

#[tauri::command]
#[specta::specta]
pub async fn discovery_review(
    controller: State<'_, DiscoveryController>,
    ids: Vec<String>,
    accept: bool,
    revision: u32,
) -> Result<DiscoverySnapshot, CommandError> {
    app::discovery_review(&controller, ids, accept, revision).await
}
