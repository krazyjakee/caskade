//! Smaller things shared once (#248): a save version, a web game's offline
//! copy, a ROM with its emulator profile, or a collection, through the same
//! links as games (`commands::shares`). A share link's `share_inspect`
//! answers what it holds (`ShareInfo.contents`); `share_receive` receives
//! all but a collection, which `share_receive_collection` adds to the
//! signed-in account's collections without downloading anything.
//!
//! Forwarded to `caskade_app::commands::share_items`.
use caskade_app::commands::share_items as app;
use caskade_core::accounts::AccountService;
use caskade_core::model::Collection;
use caskade_share::{ShareExpiry, ShareItem, Shares};
use tauri::State;

use super::shares::ShareCreated;
use crate::error::CommandError;

/// Shares `item` until `expiry`. A collection is the signed-in account's.
#[tauri::command]
#[specta::specta]
pub async fn share_item(
    shares: State<'_, Shares>,
    accounts: State<'_, AccountService>,
    item: ShareItem,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    let (share, link) = app::share_item(&shares, &accounts, item, expiry).await?;
    Ok(ShareCreated { share, link })
}

/// Adds the collection `link` shares to the signed-in account's
/// collections: the games here, and a title-only record of each one that
/// isn't. Nothing downloads.
#[tauri::command]
#[specta::specta]
pub async fn share_receive_collection(
    shares: State<'_, Shares>,
    accounts: State<'_, AccountService>,
    link: String,
) -> Result<Collection, CommandError> {
    app::share_receive_collection(&shares, &accounts, &link).await
}
