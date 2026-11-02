use super::{
    Context, LocationKind, LocationSpec, MatchMethod, SaveLocation, SaveMatch, SavePreferences,
    SaveState,
};
use crate::{
    Result,
    db::LibraryStore,
    ludusavi::{self, Ludusavi, StoreId},
    model::{ExternalId, Game, GameEntry, InstallLocation},
};
use caskade_discovery::{HostEnv, OperatingSystem};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct Saves {
    pub(crate) store: LibraryStore,
    pub(crate) manifest: Ludusavi,
    pub(crate) environment: HostEnv,
    pub(crate) catalog: Arc<crate::emulation::Catalog>,
}

impl Saves {
    pub fn new(store: LibraryStore, manifest: Ludusavi) -> Self {
        Self::with_environment(store, manifest, HostEnv::live())
    }
    pub fn with_environment(store: LibraryStore, manifest: Ludusavi, environment: HostEnv) -> Self {
        Self {
            store,
            manifest,
            environment,
            catalog: Arc::default(),
        }
    }

    pub fn with_catalog(mut self, catalog: Arc<crate::emulation::Catalog>) -> Self {
        self.catalog = catalog;
        self
    }

    pub async fn state(&self, entry_id: &str) -> Result<SaveState> {
        Ok(self.resolve(entry_id).await?.0)
    }

    /// The release's save state and this device's named folders for it
    /// (#239): where its save files are, and what they're relative to.
    pub(crate) async fn resolve(
        &self,
        entry_id: &str,
    ) -> Result<(SaveState, super::versions::Roots)> {
        let entry = self
            .store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| crate::db::missing("entry", entry_id))?;
        let game = self
            .store
            .game(&entry.game_id)
            .await?
            .ok_or_else(|| crate::db::missing("game", &entry.game_id))?;
        let (revision, preferences) = self.store.save_preferences(entry_id).await?;
        let paths = self.release_paths(&entry).await?;
        let mut warnings = Vec::new();
        let secondary = if let Some((base, _)) = &paths {
            match self.manifest.load_secondary(base).await {
                Ok(games) => games,
                Err(error) => {
                    warnings.push(error.to_string());
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let (mut matched, definition) = self
            .match_game(&entry, &game, &preferences, &secondary)
            .await?;
        let emulated = matches!(
            entry.launch,
            Some(crate::model::LaunchSpec::Emulator { .. })
        );
        let emulator = if emulated && preferences.manifest_title.is_none() {
            super::emulator::plan(self, &entry, &game).await?
        } else {
            None
        };
        if let Some(plan) = &emulator {
            matched = SaveMatch::Matched {
                title: plan.definition.name.clone(),
                method: MatchMethod::Emulator,
            };
        }
        let ids = ids(&entry);
        let store = store_name(&entry, &ids);
        let store_id = ids.iter().find_map(|id| match (store.as_deref(), id) {
            (
                Some("steam"),
                ExternalId::Steam { app_id } | ExternalId::SteamShortcut { app_id },
            ) => Some(app_id.to_string()),
            (Some("gog"), ExternalId::Gog { product_id }) => Some(product_id.to_string()),
            _ => None,
        });
        let environment = self.environment.clone();
        let manual = preferences.locations.clone();
        let (automatic, locations, scan_warnings, roots) = tokio::task::spawn_blocking(move || {
            let mut context = paths
                .as_ref()
                .map(|(base, root)| Context::for_release(&environment, &entry, base, root))
                .unwrap_or_else(|| Context::new(environment.os()));
            context.store = store;
            if let Some(id) = store_id {
                context.values.insert("storeGameId".into(), id);
            }
            let mut warnings = std::mem::take(&mut context.warnings);
            let mut roots = context_roots(&context);
            let automatic = if let Some(plan) = emulator {
                roots = super::emulator::roots(&plan);
                super::emulator::resolve(plan, &mut warnings)
            } else if paths.is_some() {
                definition
                    .as_ref()
                    .map(|definition| resolve_manifest(definition, &context, &mut warnings))
                    .unwrap_or_default()
            } else {
                if !emulated {
                    warnings.push(crate::tr!("This release is not installed on this device."));
                }
                Vec::new()
            };
            let locations = manual
                .map(|locations| resolve_manual(&locations, &context, &mut warnings))
                .unwrap_or_else(|| automatic.clone());
            (automatic, locations, warnings, roots)
        })
        .await
        .map_err(|error| crate::Error::Io(std::io::Error::other(error)))?;
        warnings.extend(scan_warnings);
        warnings.sort();
        warnings.dedup();
        Ok((
            SaveState {
                entry_id: entry_id.into(),
                revision,
                preferences,
                matched,
                automatic,
                locations,
                warnings,
            },
            roots,
        ))
    }

    pub async fn edit(
        &self,
        entry_id: &str,
        preferences: SavePreferences,
        revision: u32,
    ) -> Result<SaveState> {
        self.store
            .set_save_preferences(entry_id, &preferences, revision)
            .await?;
        self.state(entry_id).await
    }

    async fn release_paths(&self, entry: &GameEntry) -> Result<Option<(PathBuf, PathBuf)>> {
        Ok(match &entry.location {
            Some(InstallLocation::Managed { root_id, rel_dir }) => {
                let root = self
                    .store
                    .get_root(root_id)
                    .await?
                    .ok_or_else(|| crate::db::missing("storage root", root_id))?;
                Some((root.path.join(rel_dir), root.path))
            }
            Some(InstallLocation::External { path }) => {
                let base = if path.is_file() {
                    path.parent().unwrap_or(path).to_owned()
                } else {
                    path.clone()
                };
                let root = base
                    .ancestors()
                    .find(|path| path.file_name().is_some_and(|name| name == "steamapps"))
                    .and_then(Path::parent)
                    .or_else(|| base.parent())
                    .unwrap_or(&base)
                    .to_owned();
                Some((base, root))
            }
            None => None,
        })
    }

    async fn match_game(
        &self,
        entry: &GameEntry,
        game: &Game,
        preferences: &SavePreferences,
        secondary: &[ludusavi::Game],
    ) -> Result<(SaveMatch, Option<ludusavi::Game>)> {
        if let Some(title) = &preferences.manifest_title {
            return Ok(select(
                self.match_title(title, secondary).await?,
                MatchMethod::Manual,
            ));
        }
        if matches!(
            entry.launch,
            Some(crate::model::LaunchSpec::Emulator { .. })
        ) {
            // Console releases use emulator save definitions, not similarly
            // titled PC games from the Ludusavi manifest.
            return Ok((SaveMatch::Unknown, None));
        }
        let ids = ids(entry);
        // A release's own identity precedes imported cross-store identities.
        for steam in [true, false] {
            for id in &ids {
                let id = match (steam, id) {
                    (true, ExternalId::Steam { app_id }) => StoreId::Steam(u64::from(*app_id)),
                    (false, ExternalId::Gog { product_id }) => StoreId::Gog(*product_id),
                    _ => continue,
                };
                let mut candidates = self.manifest.by_id(id).await?;
                for local in secondary.iter().filter(|local| local.alias.is_none()) {
                    let found = match id {
                        StoreId::Steam(id) => {
                            local.steam.id == Some(id) || local.id.steam_extra.contains(&id)
                        }
                        StoreId::Gog(id) => {
                            local.gog.id == Some(id) || local.id.gog_extra.contains(&id)
                        }
                    };
                    if found {
                        candidates.retain(|game| game.title != local.title);
                        candidates.push(local.clone());
                    }
                }
                overlay(&mut candidates, secondary);
                if !candidates.is_empty() {
                    return Ok(select(
                        candidates,
                        if steam {
                            MatchMethod::Steam
                        } else {
                            MatchMethod::Gog
                        },
                    ));
                }
            }
        }
        for (index, title) in std::iter::once(&game.title)
            .chain(&game.aliases)
            .enumerate()
        {
            let candidates = self.match_title(title, secondary).await?;
            if !candidates.is_empty() {
                let method = if index == 0 {
                    MatchMethod::Title
                } else {
                    MatchMethod::Alias
                };
                return Ok(select(candidates, method));
            }
        }
        Ok((SaveMatch::Unknown, None))
    }

    async fn match_title(
        &self,
        title: &str,
        secondary: &[ludusavi::Game],
    ) -> Result<Vec<ludusavi::Game>> {
        let mut title = title.to_owned();
        let mut seen = BTreeSet::new();
        while seen.insert(title.clone()) {
            let local = secondary
                .iter()
                .find(|game| title_key(&game.title) == title_key(&title));
            if let Some(alias) = local.and_then(|game| game.alias.as_ref()) {
                title.clone_from(alias);
                continue;
            }
            let mut games = self.manifest.by_title(&title).await?;
            if let Some(local) = local {
                games.retain(|game| game.title != local.title);
                games.push(local.clone());
            }
            overlay(&mut games, secondary);
            // A hand-entered exact title disambiguates normalized collisions.
            if let Some(exact) = games.iter().find(|game| game.title == title) {
                return Ok(vec![exact.clone()]);
            }
            return Ok(games);
        }
        Ok(Vec::new())
    }
}

fn ids(entry: &GameEntry) -> Vec<ExternalId> {
    let mut ids = entry
        .external_id
        .parse()
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    for id in &entry.ids {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}
fn store_name(entry: &GameEntry, ids: &[ExternalId]) -> Option<String> {
    let owner = match &entry.runtime {
        crate::model::GameRuntime::Windows {
            prefix: Some(prefix),
            ..
        } => prefix.owner.as_ref().map(|owner| owner.as_str()),
        _ => None,
    };
    for store in ["steam", "gog", "epic", "uplay", "origin", "lutris"] {
        if owner
            .into_iter()
            .chain([entry.source_id.as_str()])
            .flat_map(|source| source.split(['.', ':']))
            .any(|part| part == store)
        {
            return Some(store.into());
        }
    }
    ids.first().map(|id| id.kind().to_owned())
}
fn title_key(title: &str) -> String {
    icu_normalizer::ComposingNormalizer::new_nfkc()
        .normalize(title)
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}
fn overlay(games: &mut [ludusavi::Game], secondary: &[ludusavi::Game]) {
    for game in games {
        if let Some(local) = secondary
            .iter()
            .find(|local| local.title == game.title && local.alias.is_none())
        {
            *game = local.clone();
        }
    }
}
fn select(games: Vec<ludusavi::Game>, method: MatchMethod) -> (SaveMatch, Option<ludusavi::Game>) {
    let games: BTreeMap<_, _> = games
        .into_iter()
        .map(|game| (game.title.clone(), game))
        .collect();
    if games.is_empty() {
        return (SaveMatch::Unknown, None);
    }
    if games.len() == 1 {
        let (title, game) = games.into_iter().next().expect("one game");
        (SaveMatch::Matched { title, method }, Some(game))
    } else {
        (
            SaveMatch::Ambiguous {
                titles: games.into_keys().collect(),
            },
            None,
        )
    }
}

/// The manifest's placeholders as folders on this device, Windows ones
/// inside the release's prefix where it has one.
fn context_roots(context: &Context) -> super::versions::Roots {
    let mut roots = super::versions::Roots::new(context.case_sensitive);
    for name in context.values.keys() {
        if matches!(
            name.as_str(),
            "game" | "osUserName" | "storeUserId" | "storeGameId"
        ) {
            continue;
        }
        if let Ok(paths) = super::expand(&format!("<{name}>"), context)
            && let Some(path) = paths.first()
        {
            roots.insert(format!("<{name}>"), path.clone());
        }
    }
    roots
}

fn resolve_manifest(
    game: &ludusavi::Game,
    context: &Context,
    warnings: &mut Vec<String>,
) -> Vec<SaveLocation> {
    let mut found = Vec::new();
    for (pattern, entry) in &game.files {
        if !context.applies(entry) {
            continue;
        }
        let resolved = super::expand(pattern, context)
            .map_err(crate::Error::Invalid)
            .and_then(|patterns| {
                let mut locations = Vec::new();
                for pattern in patterns {
                    for path in super::expand_glob(&pattern, context.case_sensitive)? {
                        if let Some(location) = super::scan_path(&path, entry.tags.clone())? {
                            locations.push(location);
                        }
                    }
                }
                Ok(locations)
            });
        match resolved {
            Ok(locations) => found.extend(locations),
            Err(error) => warnings.push(error.to_string()),
        }
    }
    if context.os == OperatingSystem::Windows
        && let Some(windows) = &context.windows
    {
        for (pattern, entry) in &game.registry {
            if context.applies(entry) {
                match super::registry::resolve(windows.registry(), pattern, entry.tags.clone()) {
                    Ok(locations) => found.extend(locations),
                    Err(error) => warnings.push(error.to_string()),
                }
            }
        }
    }
    deduplicate(found)
}

fn resolve_manual(
    specs: &[LocationSpec],
    context: &Context,
    warnings: &mut Vec<String>,
) -> Vec<SaveLocation> {
    let mut locations = Vec::new();
    for spec in specs {
        let result = if spec.kind == LocationKind::Registry {
            context
                .windows
                .as_ref()
                .map(|windows| super::registry::resolve(windows.registry(), &spec.path, Vec::new()))
                .unwrap_or_else(|| Ok(Vec::new()))
        } else {
            super::scan_path(Path::new(&spec.path), Vec::new())
                .map(|location| location.into_iter().collect())
        };
        match result {
            Ok(found) if !found.is_empty() => locations.extend(found),
            Ok(_) => locations.push(SaveLocation {
                spec: spec.clone(),
                tags: Vec::new(),
                files: Vec::new(),
                registry: Vec::new(),
            }),
            Err(error) => {
                warnings.push(error.to_string());
                locations.push(SaveLocation {
                    spec: spec.clone(),
                    tags: Vec::new(),
                    files: Vec::new(),
                    registry: Vec::new(),
                });
            }
        }
    }
    deduplicate(locations)
}

pub(super) fn deduplicate(locations: Vec<SaveLocation>) -> Vec<SaveLocation> {
    let mut found = BTreeMap::<_, SaveLocation>::new();
    for location in locations {
        let key = (location.spec.kind, location.spec.path.clone());
        if let Some(existing) = found.get_mut(&key) {
            existing.tags.extend(location.tags);
            existing.tags.sort();
            existing.tags.dedup();
            existing.registry.extend(location.registry);
        } else {
            found.insert(key, location);
        }
    }
    found.into_values().collect()
}
