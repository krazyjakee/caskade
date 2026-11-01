//! Syncing a server into the library: platforms are matched to Caskade's,
//! each ROM becomes (or updates) one game and entry, covers go to the
//! artwork cache (screenshots wait for the game's page,
//! [`RommService::page_images`]), and ROMs gone from the server leave the
//! library unless they are cached. A resync only rewrites what changed on the server and
//! never touches the user's own fields (favourite, hidden, rating, notes,
//! tags) or a title, description or emulator the user changed.
//!
//! A ROM the server lost on its disk (`missing_from_fs`, say its NAS isn't
//! mounted) stays, unavailable. When a sync finds a platform that had games
//! empty, or more than half of the server's games gone at once, it keeps
//! them and says so; a sync the user starts that finds the same games gone
//! removes them.
// Builds without `net` keep the shapes but never talk to a server.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_imports))]

use super::{
    Result, RommError, RommRom, RommServerConfig, RommService, RommSyncReport, api::RemotePlatform,
    keyed, lock, now_ms, settings::RommPlatformMapping, source_id,
};
use crate::{
    bus::Event,
    emulation::{ArgsOverride, Catalog, EmulatorTarget, GameRom},
    model::{ArtworkKind, Game, GameEntry, InstallLocation, InstallState, LaunchSpec},
};
use std::path::Path;

/// Who started a sync: only the user's confirms removals an earlier sync
/// held back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    User,
    Background,
}

/// The note on a server whose sync kept games it found gone.
fn held_note(count: usize) -> String {
    let games = if count == 1 { "game" } else { "games" };
    format!(
        "{count} {games} disappeared from the server at once, so they stay in the library \
         for now. Sync again to remove them."
    )
}

/// RomM slugs (IGDB's) whose ES-DE name differs.
const SLUG_ALIASES: &[(&str, &[&str])] = &[
    ("ngc", &["gc", "gamecube"]),
    ("sms", &["mastersystem"]),
    ("genesis-slash-megadrive", &["genesis", "megadrive"]),
    ("dc", &["dreamcast"]),
    ("ps", &["psx"]),
    ("psx", &["psx"]),
    ("nds", &["nds"]),
    ("3ds", &["n3ds"]),
];

/// The Caskade platform a RomM platform matches: by explicit RomM slug, IGDB id, then slug,
/// then name.
pub(crate) fn suggest(catalog: &Catalog, platform: &RemotePlatform) -> Option<String> {
    let slugs: Vec<&str> = [platform.slug.as_str(), platform.fs_slug.as_str()]
        .into_iter()
        .chain(
            SLUG_ALIASES
                .iter()
                .filter(|(slug, _)| *slug == platform.slug || *slug == platform.fs_slug)
                .flat_map(|(_, aliases)| aliases.iter().copied()),
        )
        .filter(|slug| !slug.is_empty())
        .collect();
    let by = |test: &dyn Fn(&crate::emulation::Platform) -> bool| {
        catalog
            .platforms()
            .find(|candidate| test(candidate))
            .map(|candidate| candidate.id.clone())
    };
    by(&|candidate| {
        candidate
            .ids
            .romm
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&platform.slug))
    })
    .or_else(|| {
        platform
            .igdb_id
            .and_then(|igdb| by(&|candidate| candidate.ids.igdb == Some(igdb)))
    })
    .or_else(|| {
        by(&|candidate| {
            candidate.id == platform.slug
                || [&candidate.ids.esde, &candidate.ids.playnite]
                    .into_iter()
                    .flatten()
                    .any(|id| slugs.iter().any(|slug| id.eq_ignore_ascii_case(slug)))
        })
    })
    .or_else(|| {
        by(&|candidate| {
            candidate.name.eq_ignore_ascii_case(&platform.name)
                || candidate.name.eq_ignore_ascii_case(platform.display_name())
        })
    })
}

/// The emulator a platform's ROMs launch with.
pub(crate) fn target_for(mapping: Option<&RommPlatformMapping>) -> EmulatorTarget {
    match mapping.and_then(|mapping| mapping.emulator_id.clone().zip(mapping.profile_id.clone())) {
        Some((emulator_id, profile_id)) => EmulatorTarget::Profile {
            emulator_id,
            profile_id,
        },
        None => EmulatorTarget::PlatformDefault,
    }
}

/// The ROMs of a launch spec as the emulator sees them.
pub(crate) fn game_roms(paths: &[std::path::PathBuf], sizes: Option<u64>) -> Vec<GameRom> {
    let several = paths.len() > 1;
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| GameRom {
            name: several.then(|| crate::tr!("Disc {number}", number = index + 1)),
            size: if several { None } else { sizes },
            ..GameRom::absolute(path)
        })
        .collect()
}

enum Change {
    Added,
    Updated,
    Unchanged,
    Skipped,
}

impl RommService {
    /// Syncs every server; failures are logged and reported per server.
    pub async fn sync_all(&self) -> Vec<Result<RommSyncReport>> {
        let servers = match self.settings().await {
            Ok(settings) => settings.servers,
            Err(error) => return vec![Err(error)],
        };
        let mut reports = Vec::new();
        for server in servers {
            let report = self.sync(&server.id).await;
            if let Err(error) = &report {
                tracing::warn!(server = %server.id, %error, "RomM sync failed");
            }
            reports.push(report);
        }
        reports
    }

    /// Imports one server's selected platforms and collections, for the
    /// user: it removes games an earlier sync kept because they looked gone
    /// by mistake, if they are still gone.
    pub async fn sync(&self, server_id: &str) -> Result<RommSyncReport> {
        self.sync_as(server_id, Trigger::User).await
    }

    pub(super) async fn sync_as(
        &self,
        server_id: &str,
        trigger: Trigger,
    ) -> Result<RommSyncReport> {
        let _sync = keyed(&self.inner.syncing, server_id).lock_owned().await;
        let config = self.server_config(server_id).await?;
        let name = if config.name.trim().is_empty() {
            config.url.clone()
        } else {
            config.name.clone()
        };
        let _activity =
            caskade_jobs::global().activity(crate::tr!("Syncing {server}", server = name));
        self.set_state(server_id, |state| state.syncing = true);
        let was_offline = self.offline(server_id);
        let outcome = self.sync_inner(&config, trigger).await;
        self.note(server_id, &outcome);
        self.set_state(server_id, |state| {
            state.syncing = false;
            if outcome.is_ok() {
                state.last_sync_ms = Some(now_ms());
                if !state.held.is_empty() {
                    state.error = Some(held_note(state.held.len()));
                }
            }
        });
        match &outcome {
            Ok(report) => {
                tracing::info!(
                    server = server_id,
                    added = report.added,
                    updated = report.updated,
                    removed = report.removed,
                    "RomM server synced"
                );
                // Evictions held back while it was unreachable.
                if let Err(error) = self.enforce_limit(None, 0).await {
                    tracing::warn!(server = server_id, %error, "RomM cache limit not enforced");
                }
            }
            // Its games were marked when it went offline.
            Err(RommError::Unreachable(_)) if !was_offline => {
                self.mark_unavailable(&config).await?
            }
            Err(_) => {}
        }
        outcome
    }

    #[cfg(not(feature = "net"))]
    async fn sync_inner(
        &self,
        _config: &RommServerConfig,
        _trigger: Trigger,
    ) -> Result<RommSyncReport> {
        Err(super::offline_build())
    }

    #[cfg(feature = "net")]
    async fn sync_inner(
        &self,
        config: &RommServerConfig,
        trigger: Trigger,
    ) -> Result<RommSyncReport> {
        use std::collections::{BTreeMap, BTreeSet};
        let client = self.client(config, None).await?;
        if let Ok(version) = client.heartbeat().await {
            self.set_state(&config.id, |state| state.version = version);
        }
        let platforms = client.platforms().await?;
        let catalog = self.inner.emulation.catalog();
        let wanted: BTreeSet<u32> = if config.import_all {
            BTreeSet::new()
        } else {
            config.collections.iter().copied().collect()
        };
        let collection_roms: BTreeSet<u32> = if wanted.is_empty() {
            BTreeSet::new()
        } else {
            client
                .collections()
                .await?
                .into_iter()
                .filter(|collection| wanted.contains(&collection.id))
                .flat_map(|collection| collection.rom_ids)
                .collect()
        };
        let mut existing: BTreeMap<u32, RommRom> = self
            .inner
            .store
            .romm_roms(&config.id)
            .await?
            .into_iter()
            .map(|rom| (rom.rom_id, rom))
            .collect();
        let known = existing.len();
        let cache_root = self.cache_root().await?;
        let mut report = RommSyncReport {
            server_id: config.id.clone(),
            ..Default::default()
        };
        // ROMs in the selection, every ROM the listings brought, the
        // platforms listed and those that came back empty.
        let mut seen = BTreeSet::new();
        let mut on_server = BTreeSet::new();
        let mut fetched = BTreeSet::new();
        let mut emptied = BTreeSet::new();
        for platform in &platforms {
            let mapping = config.mapping(&platform.slug);
            let enabled = config.import_all || mapping.is_some_and(|mapping| mapping.enabled);
            if !enabled && collection_roms.is_empty() {
                continue;
            }
            let Some(caskade) = mapping
                .and_then(|mapping| mapping.platform.clone())
                .or_else(|| suggest(&catalog, platform))
                .filter(|id| catalog.platform(id).is_some())
            else {
                if enabled {
                    report
                        .unmatched_platforms
                        .push(platform.display_name().to_owned());
                }
                continue;
            };
            let platform_name = catalog
                .platform(&caskade)
                .map_or_else(|| caskade.clone(), |platform| platform.name.clone());
            let target = target_for(mapping);
            let roms = client.roms(platform.id).await?;
            fetched.insert(platform.id);
            if roms.is_empty() {
                emptied.insert(platform.id);
            }
            for rom in roms {
                on_server.insert(rom.id);
                if !enabled && !collection_roms.contains(&rom.id) {
                    continue;
                }
                seen.insert(rom.id);
                // Lost on the server's disk: one in the library stays,
                // unavailable; a new one waits until the server finds it.
                if rom.missing_from_fs && !existing.contains_key(&rom.id) {
                    continue;
                }
                let title = rom.title();
                let previous = existing.remove(&rom.id);
                let change = self
                    .upsert(
                        config,
                        &client,
                        &cache_root,
                        (caskade.as_str(), platform_name.as_str()),
                        &target,
                        rom,
                        previous,
                    )
                    .await;
                match change {
                    Ok(Change::Added) => report.added += 1,
                    Ok(Change::Updated) => report.updated += 1,
                    Ok(Change::Unchanged) => report.unchanged += 1,
                    Ok(Change::Skipped) => {}
                    // Without the server, the rest of the list is unknown:
                    // stop before removing anything.
                    Err(error @ (RommError::Unreachable(_) | RommError::Auth(_))) => {
                        return Err(error);
                    }
                    Err(error) => report.warnings.push(crate::tr!(
                        "{game}: {error}",
                        game = title,
                        error = error
                    )),
                }
            }
        }
        // ROMs gone from the server (missing from their platform's listing,
        // or their platform is no longer listed), or only from the selection.
        let listed: BTreeSet<u32> = platforms.iter().map(|platform| platform.id).collect();
        let (gone, deselected): (Vec<RommRom>, Vec<RommRom>) = existing
            .into_values()
            .filter(|rom| !seen.contains(&rom.rom_id))
            .partition(|rom| {
                let platform = rom.rom.platform_id;
                !on_server.contains(&rom.rom_id)
                    && (fetched.contains(&platform) || !listed.contains(&platform))
            });
        for rom in deselected {
            self.drop_rom(rom, &mut report).await?;
        }
        // A listing that lost a platform's games, or most of the server's,
        // is more likely wrong than true.
        let suspicious = !gone.is_empty()
            && (gone.len() * 2 > known
                || gone
                    .iter()
                    .any(|rom| emptied.contains(&rom.rom.platform_id)));
        let held_before = lock(&self.inner.servers)
            .get(&config.id)
            .map(|state| state.held.clone())
            .unwrap_or_default();
        let confirmed = trigger == Trigger::User
            && !held_before.is_empty()
            && gone.iter().all(|rom| held_before.contains(&rom.rom_id));
        let mut held = BTreeSet::new();
        if suspicious && !confirmed {
            tracing::warn!(
                server = config.id,
                gone = gone.len(),
                known,
                "RomM listing lost many games at once; keeping them"
            );
            held = gone.iter().map(|rom| rom.rom_id).collect();
            report.warnings.push(held_note(held.len()));
        } else {
            for rom in gone {
                self.drop_rom(rom, &mut report).await?;
            }
        }
        self.set_state(&config.id, |state| state.held = held);
        Ok(report)
    }

    /// A ROM gone from the server or the selection: orphaned while it is
    /// cached, out of the library otherwise.
    #[cfg(feature = "net")]
    async fn drop_rom(&self, mut rom: RommRom, report: &mut RommSyncReport) -> Result<()> {
        if rom.cache.is_some() {
            if !rom.orphaned {
                rom.orphaned = true;
                self.inner.store.save_romm_rom(&rom).await?;
                report.orphaned += 1;
            }
        } else {
            self.remove_from_library(&rom).await?;
            report.removed += 1;
        }
        Ok(())
    }

    /// One ROM into the library.
    #[cfg(feature = "net")]
    #[allow(clippy::too_many_arguments)]
    async fn upsert(
        &self,
        config: &RommServerConfig,
        client: &super::client::Client,
        cache_root: &Path,
        (platform_id, platform_name): (&str, &str),
        target: &EmulatorTarget,
        rom: super::RemoteRom,
        previous: Option<RommRom>,
    ) -> Result<Change> {
        let store = &self.inner.store;
        let source = source_id(&config.id);
        let title = rom.title();
        let placeholder_dir = cache_root.join(&config.id).join(rom.id.to_string());
        let size = (rom.fs_size_bytes > 0).then_some(rom.fs_size_bytes);
        let previous_entry = match &previous {
            Some(previous) => store.get_entry(&previous.entry_id).await?,
            None => None,
        };
        if let (Some(mut record), Some(entry)) = (previous, previous_entry) {
            let remote_changed = record.rom != rom || record.platform_id != platform_id;
            if remote_changed {
                // What IGDB filled in where RomM had nothing gives way to
                // RomM's own value.
                let igdb = store
                    .igdb_match(&entry.game_id)
                    .await?
                    .map(|row| row.applied)
                    .unwrap_or_default();
                // RomM writes beneath the fields the user set (#171).
                store
                    .update_automatic_game(&entry.game_id, |game| {
                        let old = &record.rom;
                        // Only what hasn't changed since the last sync follows the
                        // server, edits from before manual fields (#171) included.
                        if game.title == old.title() {
                            if game.sort_title == game.title {
                                game.sort_title = title.clone();
                            }
                            game.title = title.clone();
                        }
                        let summary = rom.summary.clone().unwrap_or_default();
                        if game.description == old.summary.clone().unwrap_or_default()
                            || (!summary.is_empty()
                                && igdb.description.as_ref() == Some(&game.description))
                        {
                            game.description = summary;
                        }
                        let old_genres = old.metadatum.clone().unwrap_or_default().genres;
                        let genres = rom.metadatum.clone().unwrap_or_default().genres;
                        if game.genres == old_genres
                            || (!genres.is_empty() && igdb.genres.as_ref() == Some(&game.genres))
                        {
                            game.genres = genres;
                        }
                        let release_date = rom.release_date();
                        if game.release_date == old.release_date()
                            || (release_date.is_some() && igdb.release_date == game.release_date)
                        {
                            game.release_date = release_date;
                        }
                        // The server's old names go, its new ones join any others.
                        let old_aliases = old.aliases();
                        game.aliases.retain(|alias| !old_aliases.contains(alias));
                        for alias in rom.aliases() {
                            if alias != game.title && !game.aliases.contains(&alias) {
                                game.aliases.push(alias);
                            }
                        }
                        game.aliases.truncate(256);
                        if !game.platforms.iter().any(|name| name == platform_name) {
                            game.platforms.push(platform_name.to_owned());
                        }
                    })
                    .await?;
            }
            let mut next = entry.clone();
            let (current_target, args) = match &entry.launch {
                Some(LaunchSpec::Emulator { target, args, .. }) => (target.clone(), args.clone()),
                _ => (record.synced_target.clone(), ArgsOverride::Default),
            };
            // A target changed on the game page is the user's; keep it.
            let next_target = if current_target == record.synced_target {
                target.clone()
            } else {
                current_target
            };
            let cached = record
                .cache
                .as_ref()
                .filter(|cache| cache.roms.iter().all(|rom| rom.is_file()));
            let (dir, roms, entry_size) = match cached {
                Some(cache) => (
                    cache.dir.clone(),
                    game_roms(&cache.roms, Some(cache.size)),
                    Some(cache.size),
                ),
                None => (
                    placeholder_dir.clone(),
                    game_roms(&[placeholder_dir.join(&rom.fs_name)], size),
                    size,
                ),
            };
            next.launch = Some(LaunchSpec::Emulator {
                platform_id: platform_id.to_owned(),
                target: next_target,
                roms,
                args,
            });
            next.state = if rom.missing_from_fs && cached.is_none() {
                InstallState::Broken {
                    reason: super::missing_from_fs(),
                }
            } else {
                InstallState::Installed
            };
            next.location = Some(InstallLocation::External { path: dir });
            next.size = entry_size;
            let cover_changed = record.rom.cover() != rom.cover();
            let screenshots_changed = !record.rom.screenshots().eq(rom.screenshots());
            let entry_changed = next != entry;
            if entry_changed {
                store.upsert_entry(&next).await?;
            }
            let record_changed = remote_changed
                || record.orphaned
                || record.synced_target != *target
                || record.entry_id != next.id;
            record.rom = rom;
            record.platform_id = platform_id.to_owned();
            record.synced_target = target.clone();
            record.orphaned = false;
            record.entry_id = next.id.clone();
            record.game_id = next.game_id.clone();
            if record_changed {
                store.save_romm_rom(&record).await?;
            }
            if cover_changed || self.missing(&record, ArtworkKind::Cover).await? {
                self.import_cover(client, &record).await;
            }
            // The game's page downloads the new ones.
            if screenshots_changed
                && let Err(error) = self
                    .replace_artwork(&record, ArtworkKind::Screenshot, &[])
                    .await
            {
                tracing::warn!(rom = record.rom.id, %error, "RomM screenshots not dropped");
            }
            return Ok(if remote_changed || entry_changed {
                self.inner.bus.emit(Event::LibraryGameUpdated {
                    game_id: next.game_id,
                });
                Change::Updated
            } else {
                Change::Unchanged
            });
        }

        // New to the library (or removed from it by the user since).
        let mut detected =
            caskade_discovery::DetectedGame::new(source.clone(), rom.id.to_string(), &title);
        detected.install_dir = Some(placeholder_dir.clone());
        detected.state = InstallState::Installed;
        detected.size = size;
        let intercepted = self
            .inner
            .bus
            .run::<crate::bus::LibraryImport>(detected)
            .await;
        let detected = match intercepted.outcome {
            crate::bus::Outcome::Continue(detected) => detected,
            crate::bus::Outcome::Cancelled { by, reason } => {
                tracing::info!(rom = rom.id, %by, %reason, "RomM ROM skipped by library.import");
                return Ok(Change::Skipped);
            }
        };
        let mut game = Game::new(&detected.title);
        game.description = rom.summary.clone().unwrap_or_default();
        game.genres = rom.metadatum.clone().unwrap_or_default().genres;
        game.release_date = rom.release_date();
        game.aliases = rom.aliases();
        game.aliases.retain(|alias| *alias != game.title);
        game.platforms = vec![platform_name.to_owned()];
        let mut entry = GameEntry::new(&game.id, source, rom.id.to_string());
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: placeholder_dir.clone(),
        });
        entry.size = size;
        entry.launch = Some(LaunchSpec::Emulator {
            platform_id: platform_id.to_owned(),
            target: target.clone(),
            roms: game_roms(&[placeholder_dir.join(&rom.fs_name)], size),
            args: ArgsOverride::Default,
        });
        let saved = store.import_group(&game, &[entry], None).await?;
        let entry_id =
            saved.entry_ids.first().cloned().ok_or_else(|| {
                RommError::Server(crate::tr!("the library import saved no entry"))
            })?;
        let record = RommRom {
            server_id: config.id.clone(),
            rom_id: rom.id,
            entry_id,
            game_id: saved.game_id.clone(),
            platform_id: platform_id.to_owned(),
            rom,
            synced_target: target.clone(),
            pinned: false,
            orphaned: false,
            cache: None,
        };
        store.save_romm_rom(&record).await?;
        self.import_cover(client, &record).await;
        self.inner.bus.emit(if saved.created {
            Event::LibraryGameAdded {
                game_id: saved.game_id,
            }
        } else {
            Event::LibraryGameUpdated {
                game_id: saved.game_id,
            }
        });
        Ok(if saved.created {
            Change::Added
        } else {
            Change::Updated
        })
    }

    /// Removes a ROM's entry, and its game when nothing else is left of it.
    pub(super) async fn remove_from_library(&self, rom: &RommRom) -> Result<()> {
        let store = &self.inner.store;
        if let Some(entry) = store.get_entry(&rom.entry_id).await? {
            store.delete_entry(&entry.id).await?;
            if store.entries_for_game(&entry.game_id).await?.is_empty() {
                store.delete_game(&entry.game_id).await?;
                self.inner.bus.emit(Event::LibraryGameRemoved {
                    game_id: entry.game_id,
                });
            } else {
                self.inner.bus.emit(Event::LibraryGameUpdated {
                    game_id: entry.game_id,
                });
            }
        }
        store.delete_romm_rom(&rom.server_id, rom.rom_id).await?;
        Ok(())
    }

    /// The server can't be reached: games that aren't downloaded show as
    /// unavailable, with the reason.
    pub(super) async fn mark_unavailable(&self, config: &RommServerConfig) -> Result<()> {
        let reason = self.unavailable(&config.id).await;
        for rom in self.inner.store.romm_roms(&config.id).await? {
            // Lost on the server's disk keeps saying so.
            if rom.cache.is_some() || rom.rom.missing_from_fs {
                continue;
            }
            self.set_entry_state(
                &rom.entry_id,
                InstallState::Broken {
                    reason: reason.clone(),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// The server answers again: its games are playable again.
    pub(super) async fn mark_available(&self, server: &str) -> Result<()> {
        for rom in self.inner.store.romm_roms(server).await? {
            if rom.rom.missing_from_fs && rom.cache.is_none() {
                continue;
            }
            if let Some(entry) = self.inner.store.get_entry(&rom.entry_id).await?
                && matches!(entry.state, InstallState::Broken { .. })
            {
                self.set_entry_state(&rom.entry_id, InstallState::Installed)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn set_entry_state(&self, entry_id: &str, state: InstallState) -> Result<()> {
        let store = &self.inner.store;
        if let Some(mut entry) = store.get_entry(entry_id).await?
            && entry.state != state
        {
            entry.state = state;
            store.upsert_entry(&entry).await?;
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_romm_slugs_work_without_igdb_and_win_over_wrong_ids() {
        let catalog = Catalog::load([(
            "platforms.toml",
            r#"
[[platform]]
id = "vectrex"
name = "GCE Vectrex"
short = "Vectrex"
ids = { igdb = 70, romm = ["vectrex"] }
[[platform]]
id = "intellivision"
name = "Intellivision"
short = "Intellivision"
ids = { igdb = 67 }
"#,
        )])
        .unwrap();
        let mut remote = RemotePlatform {
            id: 3,
            slug: "vectrex".into(),
            fs_slug: "custom-folder".into(),
            name: "My console".into(),
            custom_name: None,
            rom_count: 1,
            igdb_id: None,
        };
        assert_eq!(suggest(&catalog, &remote).as_deref(), Some("vectrex"));
        remote.igdb_id = Some(67);
        assert_eq!(suggest(&catalog, &remote).as_deref(), Some("vectrex"));
    }
}
