//! What the user's other devices have that this one could have too (#245).
//!
//! Add-ons travel as each device's own inventory (`Entity::Addon`, keyed by
//! the device, #273): nothing installs or turns on by itself. This reads
//! those inventories and offers what this device lacks, while Plugins and
//! themes syncs here. Installing one goes through the catalogue, and a
//! plugin still asks for its permissions on this device.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::change::{EXISTS, Entity};
use super::engine::SyncEngine;
use super::policy::Category;
use crate::Result;

/// An add-on another device has and this one doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonOffer {
    /// The device that has it.
    pub device_id: String,
    /// The add-on's id, as the catalogue knows it.
    pub addon_id: String,
    /// Its version there, when the catalogue installed it.
    pub version: Option<String>,
}

/// Every add-on paired devices have and this device lacks, by device then
/// id; nothing while Plugins and themes is This device only here.
pub async fn addon_offers(engine: &SyncEngine) -> Result<Vec<AddonOffer>> {
    if !engine.categories().await.allows(Category::Addons) {
        return Ok(Vec::new());
    }
    let pool = &engine.store().pool;
    let mine: HashSet<String> = installed_here(pool).await?;
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT key,value_json FROM sync_fields WHERE entity=? AND field=? AND value_json IS NOT NULL",
    )
    .bind(Entity::Addon.as_str())
    .bind(EXISTS)
    .fetch_all(pool)
    .await?;
    let mut offers = BTreeSet::new();
    for (key, value) in rows {
        let Some((device, addon)) = key.split_once('|') else {
            continue;
        };
        if device == engine.device() || addon.is_empty() || mine.contains(addon) {
            continue;
        }
        let version = value
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value.get("version")?.as_str().map(str::to_owned));
        offers.insert((device.to_owned(), addon.to_owned(), version));
    }
    Ok(offers
        .into_iter()
        .map(|(device_id, addon_id, version)| AddonOffer {
            device_id,
            addon_id,
            version,
        })
        .collect())
}

/// The add-ons this device has: its plugins and what the catalogue
/// installed (themes included).
async fn installed_here(pool: &sqlx::SqlitePool) -> Result<HashSet<String>> {
    let mut ids: HashSet<String> = sqlx::query_scalar("SELECT id FROM plugins")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    let records: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='addons.records'")
            .fetch_optional(pool)
            .await?;
    if let Some(records) = records.and_then(|text| serde_json::from_str::<Value>(&text).ok())
        && let Some(addons) = records.get("addons").and_then(Value::as_object)
    {
        ids.extend(addons.keys().cloned());
    }
    Ok(ids)
}
