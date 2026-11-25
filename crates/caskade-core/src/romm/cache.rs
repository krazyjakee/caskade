//! The download cache: on-demand downloads at launch, verification,
//! unpacking, the atomic commit and LRU eviction.
//!
//! Layout under the cache folder:
//!
//! - `<server>/<rom id>/…` – a committed ROM, exactly as the emulator gets it.
//! - `.staging/<server>-<rom id>/download.part` – a download in progress;
//!   kept when interrupted so the next launch resumes it.
//! - `.staging/<server>-<rom id>/files/` – the ROM being assembled, renamed
//!   to its final folder only once it is complete and verified.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_imports))]

use super::{
    CacheEntry, CacheStatus, CachedRom, DownloadPhase, DownloadProgress, Result, RommError,
    RommGameStatus, RommRom, RommServerConfig, RommService, RommSettings, keyed, lock, now_ms,
    sync::{game_roms, target_for},
};
use crate::{
    bus::Event,
    db::LibraryStore,
    model::{InstallLocation, InstallState, LaunchSpec},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub(super) async fn remove_dir(dir: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(dir).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(dir = %dir.display(), %error, "RomM cache folder not removed");
    }
}

/// Batches title and favorite flags for cached RomM games in chunked queries.
async fn fetch_game_info(
    store: &LibraryStore,
    game_ids: &[&str],
) -> Result<HashMap<String, (String, bool)>> {
    let mut out = HashMap::new();
    for chunk in game_ids.chunks(500) {
        if chunk.is_empty() {
            continue;
        }
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id, title, favorite FROM games WHERE id IN (",
        );
        let mut separated = qb.separated(", ");
        for id in chunk {
            separated.push_bind(*id);
        }
        qb.push(")");
        let rows: Vec<(String, String, bool)> = qb
            .build_query_as()
            .fetch_all(&store.pool)
            .await
            .map_err(crate::Error::from)?;
        for (id, title, favorite) in rows {
            out.insert(id, (title, favorite));
        }
    }
    Ok(out)
}

impl RommService {
    pub(super) async fn cache_root(&self) -> Result<PathBuf> {
        let settings = self.settings().await?;
        self.cache_root_with(&settings).await
    }

    /// The configured folder, or `romm-cache/` on the default storage
    /// location.
    pub(super) async fn cache_root_with(&self, settings: &RommSettings) -> Result<PathBuf> {
        let configured = settings.cache.dir.trim();
        if !configured.is_empty() {
            return Ok(PathBuf::from(configured));
        }
        Ok(match self.inner.store.roots().await?.first() {
            Some(root) => root.path.join("romm-cache"),
            None => self.inner.fallback_cache.clone(),
        })
    }

    pub(super) async fn cache_status_with(&self, settings: &RommSettings) -> Result<CacheStatus> {
        let store = &self.inner.store;
        let cached: Vec<RommRom> = store.romm_cached().await?.into_iter().rev().collect();
        let game_ids: Vec<&str> = cached
            .iter()
            .filter(|rom| rom.cache.is_some())
            .map(|rom| rom.game_id.as_str())
            .collect();
        let game_info = fetch_game_info(store, &game_ids).await?;
        let mut games = Vec::new();
        for rom in cached {
            let Some(cache) = &rom.cache else { continue };
            let info = game_info.get(rom.game_id.as_str());
            games.push(CacheEntry {
                server_id: rom.server_id.clone(),
                rom_id: rom.rom_id,
                entry_id: rom.entry_id.clone(),
                game_id: rom.game_id.clone(),
                title: info.map_or_else(|| rom.rom.title(), |(title, _)| title.clone()),
                size: cache.size,
                last_used_ms: cache.last_used_ms,
                pinned: rom.pinned,
                favorite: info.is_some_and(|(_, favorite)| *favorite),
                orphaned: rom.orphaned,
            });
        }
        Ok(CacheStatus {
            dir: self.cache_root_with(settings).await?,
            used: games.iter().map(|game| game.size).sum(),
            limit: settings.cache.limit_bytes(),
            games,
        })
    }

    /// "Keep downloaded": a pinned game is never evicted.
    pub async fn set_pinned(&self, entry_id: &str, pinned: bool) -> Result<RommGameStatus> {
        let mut rom = self
            .inner
            .store
            .romm_rom_by_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("not a RomM game")))?;
        rom.pinned = pinned;
        self.inner.store.save_romm_rom(&rom).await?;
        if !pinned {
            self.enforce_limit(None, 0).await?;
        }
        self.game_status(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("not a RomM game")))
    }

    /// Removes a game's download; it downloads again at the next launch.
    pub async fn evict(&self, entry_id: &str) -> Result<bool> {
        let Some(rom) = self.inner.store.romm_rom_by_entry(entry_id).await? else {
            return Ok(false);
        };
        if rom.cache.is_none() {
            return Ok(false);
        }
        let _guard = keyed(&self.inner.preparing, entry_id).lock_owned().await;
        self.evict_rom(rom).await?;
        Ok(true)
    }

    /// Removes every download except pinned games.
    pub async fn clear_cache(&self) -> Result<CacheStatus> {
        for rom in self.inner.store.romm_cached().await? {
            if !rom.pinned && !self.busy(&rom.entry_id).await? {
                let _guard = keyed(&self.inner.preparing, &rom.entry_id)
                    .lock_owned()
                    .await;
                self.evict_rom(rom).await?;
            }
        }
        let settings = self.settings().await?;
        self.cache_status_with(&settings).await
    }

    /// Downloading or running now.
    async fn busy(&self, entry_id: &str) -> Result<bool> {
        if lock(&self.inner.downloads).contains_key(entry_id) {
            return Ok(true);
        }
        Ok(self
            .inner
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|session| session.entry_id == entry_id))
    }

    async fn evict_rom(&self, mut rom: RommRom) -> Result<()> {
        let Some(cache) = rom.cache.take() else {
            return Ok(());
        };
        remove_dir(&cache.dir).await;
        tracing::info!(
            server = %rom.server_id,
            rom = rom.rom_id,
            bytes = cache.size,
            "RomM download evicted"
        );
        if rom.orphaned {
            return self.remove_from_library(&rom).await;
        }
        self.inner.store.save_romm_rom(&rom).await?;
        let Some(entry) = self.inner.store.get_entry(&rom.entry_id).await? else {
            return Ok(());
        };
        if self.offline(&rom.server_id) {
            let reason = self.unavailable(&rom.server_id).await;
            self.set_entry_state(entry, InstallState::Broken { reason })
                .await?;
        } else {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id,
            });
        }
        Ok(())
    }

    /// Evicts least recently played games until `incoming` more bytes fit
    /// under the limit. Pinned, running and (by default) favourite games
    /// stay, as does `keep`; the limit can be exceeded when only those are
    /// left. So do games whose server isn't known to answer now (offline,
    /// not reached yet this session, or removed): they couldn't download
    /// again. A sync that reaches the server enforces the limit again.
    pub(super) async fn enforce_limit(&self, keep: Option<&str>, incoming: u64) -> Result<()> {
        let _evicting = self.inner.evicting.lock().await;
        let settings = self.settings().await?;
        let Some(limit) = settings.cache.limit_bytes() else {
            return Ok(());
        };
        let cached = self.inner.store.romm_cached().await?;
        let mut used: u64 = cached
            .iter()
            .filter_map(|rom| rom.cache.as_ref().map(|cache| cache.size))
            .sum();
        for rom in cached {
            if used.saturating_add(incoming) <= limit {
                return Ok(());
            }
            if rom.pinned
                || keep == Some(rom.entry_id.as_str())
                || !self.connected(&rom.server_id)
                || settings.server(&rom.server_id).is_none()
                || self.busy(&rom.entry_id).await?
            {
                continue;
            }
            if settings.cache.keep_favorites
                && self
                    .inner
                    .store
                    .game(&rom.game_id)
                    .await?
                    .is_some_and(|game| game.favorite)
            {
                continue;
            }
            let size = rom.cache.as_ref().map_or(0, |cache| cache.size);
            self.evict_rom(rom).await?;
            used = used.saturating_sub(size);
        }
        if used.saturating_add(incoming) > limit {
            tracing::warn!(
                used,
                incoming,
                limit,
                "RomM cache is over its limit: only kept games are left"
            );
        }
        Ok(())
    }

    /// Gets a RomM entry ready to launch: downloads it unless it is cached
    /// and points the entry at the files. `false` for other entries.
    pub async fn prepare_entry(&self, entry_id: &str) -> Result<bool> {
        let store = &self.inner.store;
        if store.romm_rom_by_entry(entry_id).await?.is_none() {
            return Ok(false);
        }
        let _guard = keyed(&self.inner.preparing, entry_id).lock_owned().await;
        let mut rom = store
            .romm_rom_by_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("the game left the library")))?;
        let entry = store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| RommError::Config(crate::tr!("the game left the library")))?;
        let settings = self.settings().await?;
        let config = settings.server(&rom.server_id).cloned();
        let ready = rom.cache.as_ref().is_some_and(|cache| {
            cache.dir.is_dir() && !cache.roms.is_empty() && cache.roms.iter().all(|p| p.is_file())
        });
        let downloaded = !ready;
        if !ready {
            // Files removed behind our back: forget the stale copy.
            if rom.cache.take().is_some() {
                store.save_romm_rom(&rom).await?;
            }
            let config = config.clone().ok_or_else(|| {
                RommError::Config(crate::tr!("this game's RomM server was removed"))
            })?;
            if rom.orphaned {
                return Err(RommError::Server(crate::tr!(
                    "this game was deleted on the RomM server"
                )));
            }
            let was_offline = self.offline(&rom.server_id);
            match self.download(&config, &settings, &rom, &entry).await {
                Ok(cache) => {
                    self.note(&rom.server_id, None);
                    if was_offline {
                        self.mark_available(&rom.server_id).await?;
                    }
                    rom.cache = Some(cache);
                }
                Err(error) => {
                    self.note_error(&rom.server_id, &error);
                    return Err(match error {
                        RommError::Unreachable(_) => {
                            let message = self.unavailable(&rom.server_id).await;
                            self.mark_unavailable(&config).await?;
                            RommError::Config(message)
                        }
                        other => other,
                    });
                }
            }
        }
        let Some(cache) = rom.cache.as_mut() else {
            return Err(RommError::Config(crate::tr!("the download is missing")));
        };
        cache.last_used_ms = now_ms();
        let cache = cache.clone();

        // Point the entry at the files and the current platform mapping,
        // unless the game page chose another emulator.
        let mapped = config
            .as_ref()
            .map(|config| target_for(config.mapping(&rom.rom.platform_slug)))
            .unwrap_or_else(|| rom.synced_target.clone());
        let mut next = entry.clone();
        let (target, args) = match &entry.launch {
            Some(LaunchSpec::Emulator { target, args, .. }) => (target.clone(), args.clone()),
            _ => (rom.synced_target.clone(), Default::default()),
        };
        let target = if target == rom.synced_target {
            rom.synced_target = mapped.clone();
            mapped
        } else {
            target
        };
        next.launch = Some(LaunchSpec::Emulator {
            platform_id: rom.platform_id.clone(),
            target,
            roms: game_roms(&cache.roms, Some(cache.size)),
            args,
        });
        next.state = InstallState::Installed;
        next.location = Some(InstallLocation::External {
            path: cache.dir.clone(),
        });
        next.size = Some(cache.size);
        if next != entry {
            store.upsert_entry(&next).await?;
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: next.game_id.clone(),
            });
        }
        store.save_romm_rom(&rom).await?;
        if downloaded {
            self.enforce_limit(Some(entry_id), 0).await?;
        }
        Ok(true)
    }

    /// Downloads, verifies, unpacks and commits one ROM, publishing progress.
    async fn download(
        &self,
        config: &RommServerConfig,
        settings: &RommSettings,
        rom: &RommRom,
        entry: &crate::model::GameEntry,
    ) -> Result<CachedRom> {
        let title = self
            .inner
            .store
            .game(&entry.game_id)
            .await?
            .map_or_else(|| rom.rom.title(), |game| game.title);
        let cancel = CancellationToken::new();
        let mut progress = DownloadProgress {
            entry_id: entry.id.clone(),
            game_id: entry.game_id.clone(),
            title,
            server_id: rom.server_id.clone(),
            phase: DownloadPhase::Downloading,
            received: 0,
            total: (rom.rom.fs_size_bytes > 0).then_some(rom.rom.fs_size_bytes),
            error: None,
        };
        lock(&self.inner.downloads).insert(
            entry.id.clone(),
            super::Download {
                progress: progress.clone(),
                cancel: cancel.clone(),
            },
        );
        self.publish(progress.clone());
        let outcome = self
            .fetch(config, settings, rom, &cancel, &mut progress)
            .await;
        match &outcome {
            Ok(_) => progress.phase = DownloadPhase::Done,
            Err(RommError::Cancelled) => progress.phase = DownloadPhase::Cancelled,
            Err(error) => {
                progress.phase = DownloadPhase::Failed;
                progress.error = Some(error.to_string());
            }
        }
        self.publish(progress);
        lock(&self.inner.downloads).remove(&entry.id);
        outcome
    }

    #[cfg(not(feature = "net"))]
    async fn fetch(
        &self,
        _config: &RommServerConfig,
        _settings: &RommSettings,
        _rom: &RommRom,
        _cancel: &CancellationToken,
        _progress: &mut DownloadProgress,
    ) -> Result<CachedRom> {
        Err(super::offline_build())
    }

    #[cfg(feature = "net")]
    async fn fetch(
        &self,
        config: &RommServerConfig,
        settings: &RommSettings,
        rom: &RommRom,
        cancel: &CancellationToken,
        progress: &mut DownloadProgress,
    ) -> Result<CachedRom> {
        let root = self.cache_root_with(settings).await?;
        let staging = root
            .join(".staging")
            .join(format!("{}-{}", rom.server_id, rom.rom_id));
        let part = staging.join("download.part");
        let files = staging.join("files");
        let target = root.join(&rom.server_id).join(rom.rom_id.to_string());
        tokio::fs::create_dir_all(&staging).await?;
        let client = self.client(config, None).await?;
        {
            let title = progress.title.clone();
            let activity =
                caskade_jobs::global().activity(crate::tr!("Downloading {game}", game = title));
            // The status bar label moves in 5% steps, events in 1% steps
            // (or every 8 MiB without a known size).
            let mut labelled: Option<u64> = None;
            let mut published: Option<u64> = None;
            let mut reported = 0u64;
            let mut update = |received: u64, total: Option<u64>| {
                progress.received = received;
                if total.is_some() {
                    progress.total = total;
                }
                let percent = progress
                    .total
                    .filter(|total| *total > 0)
                    .map(|total| received.saturating_mul(100) / total);
                if let Some(now) = percent {
                    let step = |before: u64| now >= before + 5 || (now == 100 && before < 100);
                    if labelled.is_none_or(step) {
                        activity.set_label(crate::tr!(
                            "Downloading {game} · {percent}%",
                            game = title,
                            percent = now
                        ));
                        labelled = Some(now);
                    }
                    if published != Some(now) {
                        published = Some(now);
                        self.publish(progress.clone());
                    }
                } else if received.saturating_sub(reported) >= 8 << 20 {
                    reported = received;
                    self.publish(progress.clone());
                }
            };
            // Room for the download, once the server is sending it.
            let room = async || {
                // It answered, so its own games may make room too.
                self.note(&rom.server_id, None);
                self.enforce_limit(Some(&rom.entry_id), rom.rom.fs_size_bytes)
                    .await
            };
            client
                .download(&rom.rom, &part, cancel, room, &mut update)
                .await?;
            drop(activity);
        }
        progress.phase = if rom.rom.is_multi() {
            DownloadPhase::Unpacking
        } else {
            DownloadPhase::Verifying
        };
        self.publish(progress.clone());
        let extensions = self
            .inner
            .emulation
            .catalog()
            .platform(&rom.platform_id)
            .map(|platform| platform.extensions.clone())
            .unwrap_or_default();
        let remote = rom.rom.clone();
        let check = settings.cache.verify;
        let (part_path, files_path) = (part.clone(), files.clone());
        let (roms, size) = caskade_jobs::blocking(crate::tr!("Unpacking ROM"), move || {
            super::assemble::assemble(&part_path, &files_path, &remote, &extensions, check)
        })
        .await
        .map_err(|error| RommError::Core(crate::Error::Io(std::io::Error::other(error))))??;
        // Commit: the finished folder replaces any stale copy in one rename.
        if tokio::fs::try_exists(&target).await? {
            tokio::fs::remove_dir_all(&target).await?;
        }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&files, &target).await?;
        remove_dir(&staging).await;
        let now = now_ms();
        tracing::info!(
            server = %rom.server_id,
            rom = rom.rom_id,
            bytes = size,
            "RomM ROM downloaded"
        );
        Ok(CachedRom {
            roms: roms.iter().map(|path| target.join(path)).collect(),
            dir: target,
            size,
            cached_at_ms: now,
            last_used_ms: now,
        })
    }
}
