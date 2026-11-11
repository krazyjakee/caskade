//! Smaller things shared once (#248): a save version, a web game's offline
//! copy, a ROM with its emulator profile, or a collection, through the same
//! links as games. A share link's `share_inspect` answers what it holds;
//! `share_receive` receives all but a collection, which
//! [`share_receive_collection`] adds to the signed-in account's collections
//! without downloading anything.
use caskade_core::{accounts::AccountService, model::Collection};
use caskade_share::{ShareExpiry, ShareItem, Shares};

use super::shares::ShareCreated;

use crate::error::{CommandError, report};

/// Shares `item` until `expiry`. A collection is the signed-in account's.
pub async fn share_item(
    shares: &Shares,
    accounts: &AccountService,
    item: ShareItem,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    crate::permissions::enforce("share_item", serde_json::Value::default)?;
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
pub async fn share_receive_collection(
    shares: &Shares,
    accounts: &AccountService,
    link: &str,
) -> Result<Collection, CommandError> {
    crate::permissions::enforce("share_receive_collection", serde_json::Value::default)?;
    let account = accounts.require_active().map_err(report)?;
    shares
        .receive_collection(link, &account)
        .await
        .map_err(report)
}
