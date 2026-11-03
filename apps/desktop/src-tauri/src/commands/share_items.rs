//! Smaller things shared once (#248): a save version, a web game's offline
//! copy, a ROM with its emulator profile, or a collection, through the same
//! links as games (`commands::shares`). A share link's `share_inspect`
//! answers what it holds (`ShareInfo.contents`); `share_receive` receives
//! all but a collection, which `share_receive_collection` adds to the
//! signed-in account's collections without downloading anything.

use caskade_core::accounts::AccountService;
use caskade_core::model::Collection;
use caskade_core::share::{ShareExpiry, ShareItem, Shares};
use tauri::State;

use super::shares::ShareCreated;
use crate::error::{CommandError, report};

/// Shares `item` until `expiry`. A collection is the signed-in account's.
#[tauri::command]
#[specta::specta]
pub async fn share_item(
    shares: State<'_, Shares>,
    accounts: State<'_, AccountService>,
    item: ShareItem,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    let account = accounts.require_active().map_err(report)?;
    let share = shares
        .share_item(item, expiry, &account)
        .await
        .map_err(report)?;
    let link = shares.link(&share.id).await.map_err(report)?;
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
    let account = accounts.require_active().map_err(report)?;
    shares
        .receive_collection(&link, &account)
        .await
        .map_err(report)
}
