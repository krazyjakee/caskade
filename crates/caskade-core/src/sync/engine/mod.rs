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

mod map;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tokio::sync::Mutex;

use self::map::Mapped;
use super::change::{Change, EXISTS, Entity, key_parts};
use super::clock::{Clock, Hlc, Stamp};
use super::policy::{Category, SyncSettings};
use crate::db::{Applied, FieldKey, LibraryStore, Seen};
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

type EntityFields = HashMap<Box<str>, Vec<(Box<str>, Winner)>>;

#[derive(Default)]
struct State {
    loaded: bool,
    clock: Clock,
    /// Two-level map `(entity, key) -> [(field, winner)]` so lookups borrow `&str`
    /// without cloning ~58,000 `(key, field)` strings per capture.
    fields: HashMap<Entity, EntityFields>,
    /// Each origin once, shared by its fields.
    origins: HashSet<Arc<str>>,
    /// The categories last seen turned on, to notice one turned back on.
    on: Option<BTreeSet<Category>>,
}

/// A field's winner as the engine keeps it between captures: its value as
/// JSON text and its origin shared with the device's other fields. Parsed
/// values of a real library's ~29,000 fields held ~37 MB (#111).
struct Winner {
    /// `serde_json`'s text of the value: it writes object keys sorted, so
    /// equal values have equal text.
    value: Option<Box<str>>,
    clock: Hlc,
    origin: Arc<str>,
}

impl State {
    fn get(&self, entity: Entity, key: &str, field: &str) -> Option<&Winner> {
        self.fields
            .get(&entity)?
            .get(key)?
            .iter()
            .find(|(f, _)| f.as_ref() == field)
            .map(|(_, winner)| winner)
    }

    /// Records `key`'s winner.
    fn set(&mut self, key: FieldKey, value: Option<&Value>, stamp: &Stamp) {
        self.set_text(key, value.map(Value::to_string), stamp);
    }

    /// Records `key`'s winner, its value already as `serde_json`'s text.
    fn set_text(&mut self, (entity, key, field): FieldKey, value: Option<String>, stamp: &Stamp) {
        let origin = match self.origins.get(stamp.origin.as_str()) {
            Some(origin) => origin.clone(),
            None => {
                let origin: Arc<str> = stamp.origin.as_str().into();
                self.origins.insert(origin.clone());
                origin
            }
        };
        let winner = Winner {
            value: value.map(String::into_boxed_str),
            clock: stamp.clock,
            origin,
        };
        let by_key = self.fields.entry(entity).or_default();
        let entries = match by_key.get_mut(key.as_str()) {
            Some(entries) => entries,
            None => by_key.entry(key.into_boxed_str()).or_default(),
        };
        if let Some((_, slot)) = entries.iter_mut().find(|(f, _)| f.as_ref() == field) {
            *slot = winner;
        } else {
            entries.push((field.into_boxed_str(), winner));
        }
    }
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
        state.fields.clear();
        for (key, value, stamp) in self.store.sync_field_texts().await? {
            state.set_text(key, value, &stamp);
        }
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
        let mut snapshot = self
            .store
            .sync_snapshot_for_device(Some(&self.device))
            .await?;
        if let Some(settings) = self.hooks.settings() {
            super::settings::snapshot(settings, &mut snapshot.values).await;
            super::romm::snapshot(settings, &mut snapshot.values).await;
        }
        // Build a borrowed lookup index of snapshot keys so the tombstone loop
        // checks membership without cloning ~29,000 `(key, field)` strings.
        let mut present: HashMap<Entity, HashMap<&str, Vec<&str>>> = HashMap::new();
        let mut changes = Vec::new();
        for ((entity, key, field), value) in &snapshot.values {
            present
                .entry(*entity)
                .or_default()
                .entry(key.as_str())
                .or_default()
                .push(field.as_str());
            let current = state
                .get(*entity, key, field)
                .and_then(|winner| winner.value.as_deref());
            if current.is_none_or(|current| !is_text_of(current, value)) {
                changes.push(self.draft(*entity, key, field, Some(value.clone())));
            }
        }
        let has_snapshot_field = |entity: Entity, key: &str, field: &str| {
            present
                .get(&entity)
                .and_then(|by_key| by_key.get(key))
                .is_some_and(|fields| fields.contains(&field))
        };
        for (&entity, by_key) in &state.fields {
            for (key, entries) in by_key {
                let key = key.as_ref();
                for (field, current) in entries {
                    let field = field.as_ref();
                    if current.value.is_none() || has_snapshot_field(entity, key, field) {
                        continue;
                    }
                    // Sessions and save versions are a union: one that's gone here
                    // (pruned, #239) stays elsewhere.
                    if matches!(entity, Entity::Session | Entity::SaveVersion) {
                        continue;
                    }
                    // A stand-in's marker isn't an account to delete: it joined one,
                    // or it's another device's.
                    if entity == Entity::Account
                        && field == EXISTS
                        && current
                            .value
                            .as_deref()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .is_some_and(|value| crate::db::is_stand_in_marker(&value))
                    {
                        continue;
                    }
                    // Another device's add-ons and copies are that device's to remove.
                    if matches!(entity, Entity::Addon | Entity::Copy)
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
                        && !has_snapshot_field(entity, key, EXISTS)
                    {
                        continue;
                    }
                    changes.push(self.draft(entity, key, field, None));
                }
            }
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
            state.set(
                (change.entity, change.key.clone(), change.field.clone()),
                change.value.as_ref(),
                &change.stamp(),
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
        let mut game_index: Option<crate::db::SyncGameIndex> = None;
        loop {
            let pending = self.store.sync_pending(1_000_000).await?;
            let mut progress = false;
            for change in &pending {
                if !categories.allows_entity(change.entity) {
                    continue;
                }
                state.clock.observe(change.clock);
                let creates = categories.allows(Category::Library);
                match self
                    .map_with_index(change, creates, &mut game_index)
                    .await?
                {
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
                            state.set(
                                (change.entity, target, change.field.clone()),
                                value.as_ref(),
                                &change.stamp(),
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
                                let Some(cache) = self.hooks.artwork.get().cloned() else {
                                    continue;
                                };
                                let hash = synced.hash;
                                match tokio::task::spawn_blocking(move || cache.cached(&hash))
                                    .await
                                    .ok()
                                    .and_then(|r| r.ok())
                                    .flatten()
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
                                state.set(
                                    (change.entity, target, change.field.clone()),
                                    value.as_ref(),
                                    &change.stamp(),
                                );
                                applied += 1;
                                progress = true;
                            }
                        }
                    }
                    Mapped::To(target, value) if change.entity == Entity::RommServer => {
                        let Some(settings) = self.hooks.settings() else {
                            continue;
                        };
                        if super::romm::apply(
                            settings,
                            &self.store,
                            self.hooks.romm_removed.get(),
                            change,
                            &target,
                            value.clone(),
                        )
                        .await?
                        {
                            state.set(
                                (change.entity, target, change.field.clone()),
                                value.as_ref(),
                                &change.stamp(),
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
                                state.set(
                                    (change.entity, target, change.field.clone()),
                                    value.as_ref(),
                                    &change.stamp(),
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

    /// The field's winner here, for tests and status.
    pub async fn winner(&self, entity: Entity, key: &str, field: &str) -> Option<Stamp> {
        let state = self.state.lock().await;
        state.get(entity, key, field).map(|winner| Stamp {
            clock: winner.clock,
            origin: winner.origin.to_string(),
        })
    }
}

/// Whether `text` is `serde_json`'s text of `value`, without writing it out:
/// capture compares every synced field each time.
fn is_text_of(text: &str, value: &Value) -> bool {
    /// Takes the text as it's written, matching it against the rest.
    struct Matches<'a>(&'a [u8]);
    impl std::io::Write for Matches<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match self.0.strip_prefix(bytes) {
                Some(rest) => {
                    self.0 = rest;
                    Ok(bytes.len())
                }
                None => Err(std::io::ErrorKind::InvalidData.into()),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut rest = Matches(text.as_bytes());
    serde_json::to_writer(&mut rest, value).is_ok() && rest.0.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_value_matches_only_its_own_text() {
        let value = json!({"b": [1, 2.5, null], "a": "x\"y"});
        assert!(is_text_of(&value.to_string(), &value));
        assert!(is_text_of(r#"{"a":"x\"y","b":[1,2.5,null]}"#, &value));
        assert!(!is_text_of(r#"{"a":"x\"y","b":[1,2.5]}"#, &value));
        assert!(!is_text_of(r#"{"a":"x\"y","b":[1,2.5,null]} "#, &value));
        assert!(!is_text_of("", &value));
        assert!(!is_text_of("1", &json!(1.0)));
    }
}
