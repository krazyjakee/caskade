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

/// A platform's preferred emulator from another device that waits here
/// because this device has no emulator of that definition (#275).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorOffer {
    pub platform_id: String,
    /// The emulator definition the other device uses for it.
    pub definition_id: String,
    /// The device that chose it last.
    pub device_id: String,
}

/// Every preferred emulator waiting for its emulator here, newest choice
/// per platform; nothing while Preferred emulators is This device only.
/// Installs nothing: the offer is the user's to take.
pub async fn emulator_offers(engine: &SyncEngine) -> Result<Vec<EmulatorOffer>> {
    if !engine
        .categories()
        .await
        .allows(Category::PreferredEmulators)
    {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT key,origin,value_json FROM sync_changes WHERE applied=0 AND entity=? AND field='definition' \
         ORDER BY wall,counter,origin",
    )
    .bind(Entity::PreferredEmulator.as_str())
    .fetch_all(&engine.store().pool)
    .await?;
    // The newest choice per platform wins, a cleared one included.
    let mut latest = std::collections::BTreeMap::new();
    for (platform, origin, value) in rows {
        let definition = value
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value.as_str().map(str::to_owned));
        latest.insert(platform, (origin, definition));
    }
    Ok(latest
        .into_iter()
        .filter_map(|(platform_id, (device_id, definition))| {
            Some(EmulatorOffer {
                platform_id,
                definition_id: definition?,
                device_id,
            })
        })
        .collect())
}

/// Keeps this device's emulator for `platform_id`: the waiting choice from
/// other devices is set aside, so installing that emulator later changes
/// nothing either.
pub async fn keep_emulator(engine: &SyncEngine, platform_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE sync_changes SET applied=1,target=key WHERE applied=0 AND entity=? AND key=? \
         AND field='definition'",
    )
    .bind(Entity::PreferredEmulator.as_str())
    .bind(platform_id)
    .execute(&engine.store().pool)
    .await?;
    Ok(())
}

#[cfg(feature = "net")]
/// Applies waiting preferred emulators once emulators change here: one
/// installed from an offer becomes the platform's default at once, not at
/// the next exchange.
pub fn watch_emulators(
    bus: &crate::bus::Bus,
    sync: super::SyncService,
) -> tokio::task::JoinHandle<()> {
    let mut events = bus.stream();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(crate::bus::Event::EmulationChanged { scope }) if scope == "emulators" => {
                    let Some(engine) = sync.engine() else {
                        continue;
                    };
                    if let Err(error) = engine.apply_pending().await {
                        tracing::warn!(%error, "waiting preferred emulators not applied");
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// A game another device has installed that could be copied here (#276).
#[cfg(feature = "net")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGame {
    pub game_id: String,
    pub title: String,
    pub size_bytes: Option<u64>,
    /// A Windows build that runs here through Proton.
    pub through_proton: bool,
}

/// Every game `device_id` has installed whose copy can come here: not a
/// launcher's install, and a build that runs on this system. Hidden games
/// and games already installed here stay out. By title.
#[cfg(feature = "net")]
pub async fn device_games(
    store: &crate::db::LibraryStore,
    devices: &crate::devices::Devices,
    device_id: &str,
) -> Result<Vec<DeviceGame>> {
    let games: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT g.id,g.title FROM game_entries e JOIN games g ON g.id=e.game_id \
         WHERE e.source_id=? AND g.hidden=0 AND NOT EXISTS (SELECT 1 FROM game_entries h \
         WHERE h.game_id=g.id AND h.source_id NOT LIKE 'peer:%' \
         AND h.state_kind IN ('installed','updating')) \
         ORDER BY g.sort_title COLLATE NOCASE,g.id",
    )
    .bind(format!("peer:{device_id}"))
    .fetch_all(&store.pool)
    .await?;
    let mut found = Vec::new();
    for (game_id, title) in games {
        let copies = super::copies::peer_copies(store, devices, None, &game_id).await?;
        let Some(copy) = copies
            .into_iter()
            .find(|copy| copy.device_id == device_id && copy.blocked.is_none())
        else {
            continue;
        };
        found.push(DeviceGame {
            game_id,
            title,
            size_bytes: copy.size_bytes,
            through_proton: copy.through_proton,
        });
    }
    Ok(found)
}
