//! Settings that travel (#273): `core.appearance` and `core.accounts`'
//! rating board, as `policy::SETTINGS` declares them, field by field.
//!
//! Only a field the user moved away from its default is in the snapshot: a
//! new device's defaults would otherwise win over a household's choices,
//! being captured later. A field set back to its default leaves the snapshot,
//! and the tombstone that follows sets it back to the default everywhere.
//!
//! Applying goes through `SettingsService::set`, so validation and change
//! events run as for an edit here; a value this device's schema refuses is
//! left out, and this device keeps its own.

use std::collections::HashMap;

use serde_json::Value;

use super::change::{Change, Entity};
use super::policy;
use crate::Result;
use crate::db::{FieldKey, LibraryStore};
use crate::settings::SettingsService;

/// Each synced section and the entity that carries it.
pub const SECTIONS: [(Entity, &str); 2] = [
    (Entity::Appearance, "core.appearance"),
    (Entity::AccountSettings, "core.accounts"),
];

/// The section `entity` carries.
pub fn namespace(entity: Entity) -> Option<&'static str> {
    SECTIONS
        .iter()
        .find(|(carrier, _)| *carrier == entity)
        .map(|(_, namespace)| *namespace)
}

/// The top-level fields of `namespace` that policy declares synced.
fn fields(namespace: &str) -> Vec<&'static str> {
    policy::setting(namespace)
        .filter(|rule| rule.category.is_some())
        .map(|rule| {
            rule.fields
                .iter()
                .filter(|field| !field.contains('.'))
                .copied()
                .collect()
        })
        .unwrap_or_default()
}

/// The synced fields here that differ from their defaults.
pub async fn snapshot(settings: &SettingsService, values: &mut HashMap<FieldKey, Value>) {
    for (entity, namespace) in SECTIONS {
        let (Ok(section), Some(defaults)) =
            (settings.get(namespace).await, settings.defaults(namespace))
        else {
            continue;
        };
        for field in fields(namespace) {
            if let Some(value) = section.value.get(field)
                && defaults.get(field) != Some(value)
            {
                values.insert(
                    (entity, namespace.to_owned(), field.to_owned()),
                    value.clone(),
                );
            }
        }
    }
}

/// Applies a synced field here: the value, or the default for a tombstone.
/// `true` when written; a superseded change, or one this device's schema
/// refuses, is marked applied without a write.
pub async fn apply(
    settings: &SettingsService,
    store: &LibraryStore,
    change: &Change,
    value: Option<Value>,
) -> Result<bool> {
    let Some(namespace) = namespace(change.entity) else {
        return Ok(false);
    };
    if let Some(current) = store
        .sync_field(change.entity, namespace, &change.field)
        .await?
        && current.stamp >= change.stamp()
    {
        store.sync_mark_applied(change, Some(namespace)).await?;
        return Ok(false);
    }
    let field = change.field.as_str();
    let (Ok(section), Some(defaults)) =
        (settings.get(namespace).await, settings.defaults(namespace))
    else {
        return Ok(false);
    };
    if !fields(namespace).contains(&field) {
        store.sync_mark_applied(change, Some(namespace)).await?;
        return Ok(false);
    }
    let mut next = section.value;
    let Some(object) = next.as_object_mut() else {
        return Ok(false);
    };
    match value.clone().or_else(|| defaults.get(field).cloned()) {
        Some(value) => object.insert(field.to_owned(), value),
        None => object.remove(field),
    };
    if let Err(error) = settings.set(namespace, next, None).await {
        tracing::warn!(%error, namespace, field, "a setting from another device wasn't applied");
        store.sync_mark_applied(change, Some(namespace)).await?;
        return Ok(false);
    }
    store.sync_record_applied(change, namespace, value).await?;
    Ok(true)
}
