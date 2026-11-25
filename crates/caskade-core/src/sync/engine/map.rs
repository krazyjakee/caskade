use std::collections::HashSet;

use serde_json::Value;

use super::SyncEngine;
use crate::Result;
use crate::sync::change::{Change, EXISTS, Entity, join_key, key_parts};

/// What a change maps to here.
pub(super) enum Mapped {
    /// Its key and value in this device's ids.
    To(String, Option<Value>),
    /// Done without a write (an identity: the match is the result).
    Done(String),
    Wait,
}

impl SyncEngine {
    /// A change's key and value in this device's ids.
    pub(super) async fn map_with_index(
        &self,
        change: &Change,
        creates: bool,
        index: &mut Option<crate::db::SyncGameIndex>,
    ) -> Result<Mapped> {
        if change.origin == self.device {
            return Ok(Mapped::To(change.key.clone(), change.value.clone()));
        }
        if change.entity == Entity::Game && change.field == "identity" {
            return Ok(
                match self
                    .game_with_index(&change.origin, &change.key, creates, index)
                    .await?
                {
                    Some(game) => Mapped::Done(game),
                    None => Mapped::Wait,
                },
            );
        }
        let mut parts: Vec<String> = key_parts(&change.key)
            .into_iter()
            .map(str::to_owned)
            .collect();
        for &part in change.entity.game_parts() {
            let Some(origin_game) = parts.get(part).cloned() else {
                return Ok(Mapped::Wait);
            };
            match self
                .game_with_index(&change.origin, &origin_game, creates, index)
                .await?
            {
                Some(game) => parts[part] = game,
                None => return Ok(Mapped::Wait),
            }
        }
        let mut value = change.value.clone();
        // An account another device named: the same here, or this device's
        // household admin for another device's untouched default admin.
        match (change.entity, change.field.as_str(), value.as_mut()) {
            (Entity::Collection, "account", Some(account)) => {
                if let Some(local) = account.as_str().map(str::to_owned)
                    && let Some(local) = self.store.sync_local_account(&local).await?
                {
                    *account = Value::String(local);
                }
            }
            (Entity::SaveVersion, _, Some(record)) => {
                if let Some(account) = record.get("account").and_then(Value::as_str)
                    && !account.is_empty()
                    && let Some(local) = self.store.sync_local_account(account).await?
                {
                    record["account"] = Value::String(local);
                }
            }
            _ => {}
        }
        // A record names its game; a session's `device` field doesn't.
        if matches!(change.entity, Entity::Session | Entity::SaveVersion)
            && change.field == EXISTS
            && let Some(record) = value.as_mut()
        {
            let origin_game = record
                .get("game")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            match self
                .game_with_index(&change.origin, &origin_game, creates, index)
                .await?
            {
                Some(game) => record["game"] = Value::String(game),
                None => return Ok(Mapped::Wait),
            }
        }
        if change.entity == Entity::KeptApart
            && change.field == EXISTS
            && change.value.is_some()
            && let Some(separated) = self
                .keep_apart(&change.origin, &change.key, &parts[0])
                .await?
        {
            // Separating changes entries/games; drop any cached index so subsequent
            // lookups reflect the separated game cleanly.
            *index = None;
            parts[0] = separated;
        }
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        Ok(Mapped::To(join_key(&refs), value))
    }

    /// The game here that `origin`'s game `id` is (#229): matched once and
    /// remembered, by the ids the identity names, its IGDB id, store ids,
    /// then title; a title-only game when nothing matches and `creates`.
    /// `None` until the game's identity has arrived.
    pub async fn game(&self, origin: &str, id: &str, creates: bool) -> Result<Option<String>> {
        self.game_with_index(origin, id, creates, &mut None).await
    }

    async fn game_with_index(
        &self,
        origin: &str,
        id: &str,
        creates: bool,
        index: &mut Option<crate::db::SyncGameIndex>,
    ) -> Result<Option<String>> {
        if origin == self.device {
            return Ok(self
                .store
                .sync_game_exists(id)
                .await?
                .then(|| id.to_owned()));
        }
        if let Some(game) = self.store.sync_mapped_game(origin, id).await? {
            return Ok(Some(game));
        }
        let Some(identity) = self.store.sync_identity(origin, id).await? else {
            return Ok(None);
        };
        let aliases = identity
            .get("same")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for alias in aliases.iter().filter_map(Value::as_str) {
            let parts = key_parts(alias);
            let [alias_origin, alias_id] = parts[..] else {
                continue;
            };
            let found = if alias_origin == self.device {
                self.store
                    .sync_game_exists(alias_id)
                    .await?
                    .then(|| alias_id.to_owned())
            } else {
                self.store.sync_mapped_game(alias_origin, alias_id).await?
            };
            if let Some(game) = found {
                self.store
                    .set_sync_mapped_game(origin, id, &game, "same")
                    .await?;
                return Ok(Some(game));
            }
        }
        let found = if let Some(cached) = index.as_ref() {
            cached.find(&identity)
        } else {
            let built = self.store.sync_game_index().await?;
            let found = built.find(&identity);
            *index = Some(built);
            found
        };
        if let Some((game, method)) = found {
            self.store
                .set_sync_mapped_game(origin, id, &game, method)
                .await?;
            return Ok(Some(game));
        }
        if !creates {
            return Ok(None);
        }
        let game = self.store.sync_create_game(&identity).await?;
        if let Some(cached) = index.as_mut() {
            cached.record_created(game.clone(), &identity);
        }
        self.store
            .set_sync_mapped_game(origin, id, &game, "created")
            .await?;
        self.arrived
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(game))
    }

    /// `origin` kept its game `id` apart, and here it's part of `local`
    /// with other entries: separate the entries it has, as `origin` did.
    /// Returns the separated game.
    async fn keep_apart(&self, origin: &str, id: &str, local: &str) -> Result<Option<String>> {
        let Some(identity) = self.store.sync_identity(origin, id).await? else {
            return Ok(None);
        };
        let stores: HashSet<&str> = identity
            .get("stores")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if stores.is_empty() {
            return Ok(None);
        }
        let entries = self.store.sync_game_entry_stores(local).await?;
        let (theirs, others): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .partition(|(_, keys)| keys.iter().any(|key| stores.contains(key.as_str())));
        if theirs.is_empty() || others.is_empty() {
            return Ok(None);
        }
        let ids: Vec<String> = theirs.into_iter().map(|(entry, _)| entry).collect();
        let Some(game) = self.store.sync_separate(&ids).await? else {
            return Ok(None);
        };
        self.store
            .set_sync_mapped_game(origin, id, &game, "keptApart")
            .await?;
        Ok(Some(game))
    }
}
