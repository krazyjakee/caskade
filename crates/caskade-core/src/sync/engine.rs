//! The change log's engine (#228, #229): captures this device's changes,
//! takes in other devices', and applies them by last writer wins.
//!
//! **Capture** compares a snapshot of every synced field (`sync_snapshot`)
//! with the winners recorded here (`sync_fields`) and records each
//! difference as a change stamped with a new clock reading; a value that's
//! gone becomes a tombstone. Remote changes update the winners as they
//! apply, so they're never captured back.
//!
//! **Apply** maps each change's keys from the other device's ids to this
//! device's (games through their identity, #229), then writes it if its
//! stamp beats the field's winner. A change waits, unapplied, while its
//! category is "This device only" here or what it refers to hasn't arrived;
//! turning the category back on applies the waiting changes from the log.
//!
//! **Order doesn't matter.** Every field ends at the change with the highest
//! stamp, whatever order changes arrived in, so devices that have the same
//! changes have the same values. A row's tombstone is a value like any
//! other, so a device that never saw the deletion can't bring the row back
//! with an older value.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tokio::sync::Mutex;

use super::change::{Change, EXISTS, Entity, join_key, key_parts};
use super::clock::{Clock, Hlc, Stamp};
use super::policy::{Category, SyncSettings};
use crate::db::{Applied, FieldKey, FieldState, LibraryStore, Seen};
use crate::settings::SettingsService;
use crate::{Result, now_ms};

/// What an engine is handed from outside, each set once. A
/// [`SyncService`](super::service::SyncService) shares one among the engines
/// of each identity this device has.
#[derive(Default)]
pub(super) struct Hooks {
    settings: OnceLock<SettingsService>,
    /// Called with a RomM server's id when another device removed it, to
    /// remove its downloaded ROMs here (the policy's `Receive::RommServer`).
    romm_removed: OnceLock<Arc<dyn Fn(String) + Send + Sync>>,
    /// Called with an untouched default admin and the admin from another
    /// device it joined (`LibraryStore::sync_join_stand_in`).
    account_joined: OnceLock<Arc<dyn Fn(String, String) + Send + Sync>>,
    /// Where artwork files are: an `artwork` change applies once its file
    /// is here (#273).
    artwork: OnceLock<crate::artwork::ArtworkCache>,
    /// Called after games arrived here or a hand-set IGDB match did, so
    /// this device fills them in itself: their metadata and artwork stay on
    /// each device (#75).
    games_arrived: OnceLock<Arc<dyn Fn() + Send + Sync>>,
    /// Files that travel after the changes (save files, #239).
    #[cfg(feature = "net")]
    blobs: super::blobs::Registry,
    /// What's running on this device and the others (#240).
    #[cfg(feature = "net")]
    presence: OnceLock<super::presence::Presence>,
}

impl Hooks {
    /// Reads the category choices (`core.sync`) from `settings`.
    pub(super) fn set_settings(&self, settings: SettingsService) {
        let _ = self.settings.set(settings);
    }

    pub(super) fn settings(&self) -> Option<&SettingsService> {
        self.settings.get()
    }

    pub(super) fn on_romm_removed(&self, removed: impl Fn(String) + Send + Sync + 'static) {
        let _ = self.romm_removed.set(Arc::new(removed));
    }

    pub(super) fn on_account_joined(
        &self,
        joined: impl Fn(String, String) + Send + Sync + 'static,
    ) {
        let _ = self.account_joined.set(Arc::new(joined));
    }

    pub(super) fn set_artwork_cache(&self, cache: crate::artwork::ArtworkCache) {
        let _ = self.artwork.set(cache);
    }

    pub(super) fn on_games_arrived(&self, arrived: impl Fn() + Send + Sync + 'static) {
        let _ = self.games_arrived.set(Arc::new(arrived));
    }

    #[cfg(feature = "net")]
    pub(super) fn blobs(&self) -> &super::blobs::Registry {
        &self.blobs
    }

    #[cfg(feature = "net")]
    pub(super) fn set_presence(&self, presence: super::presence::Presence) {
        let _ = self.presence.set(presence);
    }
}

/// This device's change log.
pub struct SyncEngine {
    store: LibraryStore,
    device: String,
    state: Mutex<State>,
    hooks: Arc<Hooks>,
    /// The wall clock: tests set one that's wrong.
    time: std::sync::RwLock<Arc<dyn Fn() -> i64 + Send + Sync>>,
    /// Games or matches arrived since [`Hooks::games_arrived`] was last called.
    arrived: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct State {
    loaded: bool,
    clock: Clock,
    fields: HashMap<FieldKey, FieldState>,
    /// The categories last seen turned on, to notice one turned back on.
    on: Option<BTreeSet<Category>>,
}

/// What a change maps to here.
enum Mapped {
    /// Its key and value in this device's ids.
    To(String, Option<Value>),
    /// Done without a write (an identity: the match is the result).
    Done(String),
    Wait,
}

/// Capture order: what others depend on first (accounts, then games and
/// their identities, then what refers to them), and each row's changes
/// together, its existence and identity first, so a page boundary splits
/// at most one row.
fn entity_rank(entity: Entity) -> u8 {
    match entity {
        Entity::Account => 0,
        Entity::Game => 1,
        Entity::KeptApart
        | Entity::IgdbMatch
        | Entity::ArtworkChoice
        | Entity::Artwork
        | Entity::GameScript
        | Entity::Action
        | Entity::AchievementLink => 2,
        // After the actions its default names.
        Entity::ActionSettings => 3,
        Entity::Collection => 3,
        Entity::Member | Entity::Approval => 4,
        Entity::Session => 5,
        Entity::RommServer => 6,
        Entity::SaveVersion => 7,
        Entity::Appearance | Entity::AccountSettings => 8,
        Entity::Addon | Entity::Copy | Entity::PreferredEmulator => 9,
    }
}

fn field_rank(field: &str) -> u8 {
    match field {
        EXISTS => 0,
        "identity" => 1,
        _ => 2,
    }
}

impl SyncEngine {
    /// The log of `store`, for this device (its endpoint id).
    pub fn new(store: LibraryStore, device: impl Into<String>) -> Self {
        Self::with_hooks(store, device, Arc::default())
    }

    pub(super) fn with_hooks(
        store: LibraryStore,
        device: impl Into<String>,
        hooks: Arc<Hooks>,
    ) -> Self {
        Self {
            store,
            device: device.into(),
            state: Mutex::default(),
            hooks,
            time: std::sync::RwLock::new(Arc::new(now_ms)),
            arrived: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The kinds of blob exchanged after the changes (#239).
    #[cfg(feature = "net")]
    pub fn blobs(&self) -> &super::blobs::Registry {
        self.hooks.blobs()
    }

    /// What to do when this device's default admin joined one from another
    /// device (the app hands the signed-in account over).
    pub fn on_account_joined(&self, joined: impl Fn(String, String) + Send + Sync + 'static) {
        self.hooks.on_account_joined(joined);
    }

    /// The artwork cache, where `artwork` changes' files arrive.
    pub fn set_artwork_cache(&self, cache: crate::artwork::ArtworkCache) {
        self.hooks.set_artwork_cache(cache);
    }

    /// Presence exchanged at the end of each exchange (#240).
    #[cfg(feature = "net")]
    pub fn presence(&self) -> Option<super::presence::Presence> {
        self.hooks.presence.get().cloned()
    }

    #[cfg(feature = "net")]
    pub fn set_presence(&self, presence: super::presence::Presence) {
        self.hooks.set_presence(presence);
    }

    /// What to do when another device removed a RomM server: the app removes
    /// its downloaded ROMs and library games through `RommService`.
    pub fn on_romm_removed(&self, removed: impl Fn(String) + Send + Sync + 'static) {
        self.hooks.on_romm_removed(removed);
    }

    /// What to do after games or hand-set IGDB matches arrived from another
    /// device: the app looks them up (`Metadata::wake`). Called once per
    /// apply that brought any.
    pub fn on_games_arrived(&self, arrived: impl Fn() + Send + Sync + 'static) {
        self.hooks.on_games_arrived(arrived);
    }

    /// Replaces the wall clock this device reads (tests: a clock set wrong).
    pub fn set_wall_clock(&self, clock: impl Fn() -> i64 + Send + Sync + 'static) {
        *crate::write(&self.time) = Arc::new(clock);
    }

    fn now(&self) -> i64 {
        (crate::read(&self.time))()
    }

    /// Reads the category choices (`core.sync`) from `settings`.
    pub fn set_settings(&self, settings: SettingsService) {
        self.hooks.set_settings(settings);
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    pub fn store(&self) -> &LibraryStore {
        &self.store
    }

    /// The category choices.
    pub async fn categories(&self) -> SyncSettings {
        SyncSettings::read(self.hooks.settings()).await
    }

    async fn load(&self, state: &mut State) -> Result<()> {
        if state.loaded {
            return Ok(());
        }
        state.fields = self.store.sync_fields().await?;
        let last = self
            .store
            .sync_seen()
            .await?
            .into_values()
            .max()
            .unwrap_or_default();
        state.clock = Clock::resume(last);
        state.loaded = true;
        Ok(())
    }

    /// Records this device's changes since the last capture. Returns how
    /// many there were.
    pub async fn capture(&self) -> Result<usize> {
        let mut state = self.state.lock().await;
        self.load(&mut state).await?;
        // The categories as they start, so the first one turned on is noticed.
        if state.on.is_none() {
            let categories = self.categories().await;
            state.on = Some(
                Category::ALL
                    .into_iter()
                    .filter(|category| categories.allows_whole(*category))
                    .collect(),
            );
        }
        let mut snapshot = self.store.sync_snapshot().await?;
        // Add-ons and installed copies are each device's own: keyed by it.
        snapshot.values = std::mem::take(&mut snapshot.values)
            .into_iter()
            .map(|((entity, key, field), value)| match entity {
                Entity::Addon | Entity::Copy => {
                    ((entity, join_key(&[&self.device, &key]), field), value)
                }
                _ => ((entity, key, field), value),
            })
            .collect();
        self.settings_snapshot(&mut snapshot.values).await;
        let mut changes = Vec::new();
        for ((entity, key, field), value) in &snapshot.values {
            let current = state
                .fields
                .get(&(*entity, key.clone(), field.clone()))
                .and_then(|state| state.value.as_ref());
            if current != Some(value) {
                changes.push(self.draft(*entity, key, field, Some(value.clone())));
            }
        }
        for ((entity, key, field), current) in &state.fields {
            if current.value.is_none()
                || snapshot
                    .values
                    .contains_key(&(*entity, key.clone(), field.clone()))
            {
                continue;
            }
            // Sessions and save versions are a union: one that's gone here
            // (pruned, #239) stays elsewhere.
            if matches!(*entity, Entity::Session | Entity::SaveVersion) {
                continue;
            }
            // A stand-in's marker isn't an account to delete: it joined one,
            // or it's another device's.
            if *entity == Entity::Account
                && field == EXISTS
                && current
                    .value
                    .as_ref()
                    .is_some_and(crate::db::is_stand_in_marker)
            {
                continue;
            }
            // Another device's add-ons and copies are that device's to remove.
            if matches!(*entity, Entity::Addon | Entity::Copy)
                && key_parts(key).first() != Some(&self.device.as_str())
            {
                continue;
            }
            // A game removed here isn't a game deleted everywhere: its
            // values stay as they are on other devices.
            let parts = key_parts(key);
            let game_gone = entity.game_parts().iter().any(|&part| {
                parts
                    .get(part)
                    .is_some_and(|game| !snapshot.games.contains(*game))
            });
            if game_gone {
                continue;
            }
            // A row's fields go with it: its tombstone says so.
            if entity.has_rows()
                && field != EXISTS
                && !snapshot
                    .values
                    .contains_key(&(*entity, key.clone(), EXISTS.to_owned()))
            {
                continue;
            }
            changes.push(self.draft(*entity, key, field, None));
        }
        changes.sort_by(|a, b| {
            entity_rank(a.entity)
                .cmp(&entity_rank(b.entity))
                .then_with(|| a.key.cmp(&b.key))
                .then_with(|| field_rank(&a.field).cmp(&field_rank(&b.field)))
                .then_with(|| a.field.cmp(&b.field))
        });
        let now = self.now();
        for change in &mut changes {
            change.clock = state.clock.tick(now);
        }
        self.store.sync_record_local(&changes).await?;
        for change in &changes {
            state.fields.insert(
                (change.entity, change.key.clone(), change.field.clone()),
                FieldState {
                    value: change.value.clone(),
                    stamp: change.stamp(),
                },
            );
        }
        Ok(changes.len())
    }

    fn draft(&self, entity: Entity, key: &str, field: &str, value: Option<Value>) -> Change {
        Change {
            origin: self.device.clone(),
            clock: Hlc::default(),
            entity,
            key: key.to_owned(),
            field: field.to_owned(),
            value,
        }
    }

    /// This device's version vector.
    pub async fn seen(&self) -> Result<Seen> {
        self.store.sync_seen().await
    }

    /// Up to `limit` changes a device that has `cursor` hasn't seen, leaving
    /// out categories that are "This device only" here.
    pub async fn changes_for(&self, cursor: &Seen, limit: usize) -> Result<Vec<Change>> {
        let skip = self.held_back().await;
        self.store.sync_changes_after(cursor, &skip, limit).await
    }

    /// Entities of categories that are "This device only" here.
    pub async fn held_back(&self) -> HashSet<Entity> {
        let categories = self.categories().await;
        Entity::ALL
            .into_iter()
            .filter(|entity| !categories.allows_entity(*entity))
            .collect()
    }

    /// Categories turned on since the last call (or the first capture):
    /// their changes go to each device again in full.
    pub async fn turned_on(&self) -> Vec<Category> {
        let categories = self.categories().await;
        let on: BTreeSet<Category> = Category::ALL
            .into_iter()
            .filter(|category| categories.allows_whole(*category))
            .collect();
        let mut state = self.state.lock().await;
        let turned = match &state.on {
            Some(before) => on.difference(before).copied().collect(),
            None => Vec::new(),
        };
        state.on = Some(on);
        turned
    }

    /// Takes in another device's changes and applies what can apply.
    pub async fn receive(&self, changes: &[Change]) -> Result<usize> {
        let added = self.store.sync_import(changes).await?;
        {
            let mut state = self.state.lock().await;
            self.load(&mut state).await?;
            for change in changes {
                state.clock.observe(change.clock);
            }
        }
        self.apply_pending().await?;
        Ok(added)
    }

    /// Applies every waiting change that can apply now. Returns how many
    /// applied.
    pub async fn apply_pending(&self) -> Result<usize> {
        let mut state = self.state.lock().await;
        self.load(&mut state).await?;
        let categories = self.categories().await;
        let mut applied = 0;
        loop {
            let pending = self.store.sync_pending(1_000_000).await?;
            let mut progress = false;
            for change in &pending {
                if !categories.allows_entity(change.entity) {
                    continue;
                }
                state.clock.observe(change.clock);
                let creates = categories.allows(Category::Library);
                match self.map(change, creates).await? {
                    Mapped::Wait => {}
                    Mapped::Done(target) => {
                        self.store.sync_mark_applied(change, Some(&target)).await?;
                        progress = true;
                    }
                    Mapped::To(target, value)
                        if super::settings::namespace(change.entity).is_some() =>
                    {
                        let Some(settings) = self.hooks.settings() else {
                            continue;
                        };
                        if super::settings::apply(settings, &self.store, change, value.clone())
                            .await?
                        {
                            state.fields.insert(
                                (change.entity, target, change.field.clone()),
                                FieldState {
                                    value,
                                    stamp: change.stamp(),
                                },
                            );
                            applied += 1;
                        }
                        progress = true;
                    }
                    Mapped::To(target, value) if change.entity == Entity::Artwork => {
                        // Its file first: until it's here, the change waits.
                        let cached = match value
                            .as_ref()
                            .and_then(crate::db::SyncedArtwork::from_value)
                        {
                            Some(synced) => {
                                match self
                                    .hooks
                                    .artwork
                                    .get()
                                    .and_then(|cache| cache.cached(&synced.hash).ok().flatten())
                                {
                                    Some(cached) => Some(cached),
                                    None => continue,
                                }
                            }
                            None => None,
                        };
                        match self
                            .store
                            .sync_apply_artwork(change, &target, value.clone(), cached)
                            .await?
                        {
                            Applied::Wait => {}
                            Applied::Superseded => progress = true,
                            Applied::Written => {
                                state.fields.insert(
                                    (change.entity, target, change.field.clone()),
                                    FieldState {
                                        value,
                                        stamp: change.stamp(),
                                    },
                                );
                                applied += 1;
                                progress = true;
                            }
                        }
                    }
                    Mapped::To(target, value) if change.entity == Entity::RommServer => {
                        if self.apply_romm(change, &target, value.clone()).await? {
                            state.fields.insert(
                                (change.entity, target, change.field.clone()),
                                FieldState {
                                    value,
                                    stamp: change.stamp(),
                                },
                            );
                            applied += 1;
                        }
                        progress = true;
                    }
                    Mapped::To(target, value) => {
                        match self
                            .store
                            .sync_apply(change, &target, value.clone())
                            .await?
                        {
                            Applied::Wait => {}
                            Applied::Superseded => progress = true,
                            Applied::Written => {
                                if change.entity == Entity::IgdbMatch {
                                    self.arrived
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                                state.fields.insert(
                                    (change.entity, target, change.field.clone()),
                                    FieldState {
                                        value,
                                        stamp: change.stamp(),
                                    },
                                );
                                applied += 1;
                                progress = true;
                            }
                        }
                    }
                }
            }
            if !progress {
                break;
            }
        }
        if self
            .arrived
            .swap(false, std::sync::atomic::Ordering::Relaxed)
            && let Some(arrived) = self.hooks.games_arrived.get()
        {
            arrived();
        }
        if applied > 0
            && let Some((from, into)) = self.store.sync_join_stand_in(&self.device).await?
            && let Some(joined) = self.hooks.account_joined.get()
        {
            joined(from, into);
        }
        Ok(applied)
    }

    /// Drops what every paired device has seen and no longer needs.
    pub async fn compact(&self) -> Result<u64> {
        let mut peers = Vec::new();
        for device in self.store.paired_devices().await? {
            if device.forgotten_at_ms.is_none() {
                peers.push(self.store.sync_peer_seen(&device.endpoint_id).await?);
            }
        }
        self.store.sync_compact(&peers).await
    }

    /// A change's key and value in this device's ids.
    async fn map(&self, change: &Change, creates: bool) -> Result<Mapped> {
        if change.origin == self.device {
            return Ok(Mapped::To(change.key.clone(), change.value.clone()));
        }
        if change.entity == Entity::Game && change.field == "identity" {
            return Ok(
                match self.game(&change.origin, &change.key, creates).await? {
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
            match self.game(&change.origin, &origin_game, creates).await? {
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
            match self.game(&change.origin, &origin_game, creates).await? {
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
        if let Some((game, method)) = self.store.sync_find_game(&identity).await? {
            self.store
                .set_sync_mapped_game(origin, id, &game, method)
                .await?;
            return Ok(Some(game));
        }
        if !creates {
            return Ok(None);
        }
        let game = self.store.sync_create_game(&identity).await?;
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

    /// Synced settings' fields: appearance and the rating board where they
    /// aren't the defaults (`sync::settings`), and each RomM server's shared
    /// definition, as `policy` projects `core.romm` (no secrets, no cache, no
    /// emulator or profile choices).
    async fn settings_snapshot(&self, values: &mut HashMap<crate::db::FieldKey, Value>) {
        let Some(settings) = self.hooks.settings() else {
            return;
        };
        super::settings::snapshot(settings, values).await;
        let Ok(romm) = settings.get(crate::romm::NAMESPACE).await else {
            return;
        };
        let Some(rule) = super::policy::setting(crate::romm::NAMESPACE) else {
            return;
        };
        let everything = SyncSettings {
            categories: Category::ALL
                .into_iter()
                .map(|category| (category, super::policy::CategoryState::Sync))
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
    async fn apply_romm(
        &self,
        change: &Change,
        target: &str,
        value: Option<Value>,
    ) -> Result<bool> {
        let Some(settings) = self.hooks.settings() else {
            return Ok(false);
        };
        if let Some(current) = self
            .store
            .sync_field(change.entity, target, &change.field)
            .await?
            && current.stamp >= change.stamp()
        {
            self.store.sync_mark_applied(change, Some(target)).await?;
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
                    None => {
                        serde_json::to_value(crate::romm::RommServerConfig::new(target, "", ""))?
                    }
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
            self.store.sync_mark_applied(change, Some(target)).await?;
            return Ok(false);
        }
        self.store
            .sync_record_applied(change, target, value.clone())
            .await?;
        if value.is_none()
            && existing.is_some()
            && let Some(removed) = self.hooks.romm_removed.get()
        {
            removed(target.to_owned());
        }
        Ok(true)
    }

    /// The field's winner here, for tests and status.
    pub async fn winner(&self, entity: Entity, key: &str, field: &str) -> Option<Stamp> {
        let state = self.state.lock().await;
        state
            .fields
            .get(&(entity, key.to_owned(), field.to_owned()))
            .map(|state| state.stamp.clone())
    }
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
