//! RomM server definitions that travel (#273): each server's shared
//! definition as `policy` projects `core.romm` (no secrets, no cache, no
//! emulator or profile choices).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::change::{Change, EXISTS, Entity};
use super::policy::{self, Category, CategoryState, SyncSettings};
use crate::Result;
use crate::db::{FieldKey, LibraryStore};
use crate::settings::SettingsService;

/// Each RomM server's shared definition, as `policy` projects `core.romm`
/// (no secrets, no cache, no emulator or profile choices).
pub async fn snapshot(settings: &SettingsService, values: &mut HashMap<FieldKey, Value>) {
    let Ok(romm) = settings.get(crate::romm::NAMESPACE).await else {
        return;
    };
    let Some(rule) = policy::setting(crate::romm::NAMESPACE) else {
        return;
    };
    let everything = SyncSettings {
        categories: Category::ALL
            .into_iter()
            .map(|category| (category, CategoryState::Sync))
            .collect(),
        ..SyncSettings::default()
    };
    let Some(projected) = rule.project(&romm.value, &everything) else {
        return;
    };
    for server in projected
        .get("servers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = server.get("id").and_then(Value::as_str) {
            values.insert(
                (Entity::RommServer, id.to_owned(), EXISTS.to_owned()),
                server.clone(),
            );
        }
    }
}

/// Applies a RomM server's definition to `core.romm` here, keeping this
/// device's own choices for it (cache, emulator and profile per platform,
/// sync schedule). A new server arrives without a password: this device
/// asks for its own. `true` when written; a superseded change, or one
/// this device can't hold, is marked applied without a write.
pub async fn apply(
    settings: &SettingsService,
    store: &LibraryStore,
    romm_removed: Option<&Arc<dyn Fn(String) + Send + Sync>>,
    change: &Change,
    target: &str,
    value: Option<Value>,
) -> Result<bool> {
    if let Some(current) = store
        .sync_field(change.entity, target, &change.field)
        .await?
        && current.stamp >= change.stamp()
    {
        store.sync_mark_applied(change, Some(target)).await?;
        return Ok(false);
    }
    let Ok(snapshot) = settings.get(crate::romm::NAMESPACE).await else {
        return Ok(false);
    };
    let mut section = snapshot.value;
    let mut servers: Vec<Value> = section
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let existing = servers
        .iter()
        .position(|server| server.get("id").and_then(Value::as_str) == Some(target));
    match &value {
        Some(incoming) => {
            let mut server = match existing {
                Some(index) => servers[index].clone(),
                None => serde_json::to_value(crate::romm::RommServerConfig::new(target, "", ""))?,
            };
            for (key, field) in incoming.as_object().into_iter().flatten() {
                if key == "platforms" {
                    server[key.as_str()] = merge_platforms(server.get(key), field);
                } else {
                    server[key.as_str()] = field.clone();
                }
            }
            match existing {
                Some(index) => servers[index] = server,
                None => servers.push(server),
            }
        }
        None => {
            if let Some(index) = existing {
                servers.remove(index);
            }
        }
    }
    section["servers"] = Value::Array(servers);
    if let Err(error) = settings.set(crate::romm::NAMESPACE, section, None).await {
        // A definition this device can't hold: keep its own.
        tracing::warn!(%error, "a RomM server from another device wasn't applied");
        store.sync_mark_applied(change, Some(target)).await?;
        return Ok(false);
    }
    store
        .sync_record_applied(change, target, value.clone())
        .await?;
    if value.is_none()
        && existing.is_some()
        && let Some(removed) = romm_removed
    {
        removed(target.to_owned());
    }
    Ok(true)
}

/// A server's platform mappings from another device, each keeping this
/// device's emulator and profile for it.
fn merge_platforms(local: Option<&Value>, incoming: &Value) -> Value {
    let local: Vec<Value> = local.and_then(Value::as_array).cloned().unwrap_or_default();
    let merged = incoming
        .as_array()
        .into_iter()
        .flatten()
        .map(|platform| {
            let slug = platform.get("slug");
            let mut item = local
                .iter()
                .find(|mine| mine.get("slug") == slug)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "emulatorId": null, "profileId": null }));
            for (key, field) in platform.as_object().into_iter().flatten() {
                item[key.as_str()] = field.clone();
            }
            item
        })
        .collect();
    Value::Array(merged)
}
