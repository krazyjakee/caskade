//! Scanning a scanner's folder and reconciling the library with it.

use super::{
    ROMS_SOURCE, ScanSummary, ScannerCount, ScannerService, entry_paths,
    overrides::{Found, apply_overrides},
    rom_path, scan_error,
};
use crate::{
    Error, Result,
    bus::{Event, LibraryImport, Outcome},
    db::ScanState,
    emulation::{
        ArgsOverride, RomScanner,
        scan::{self, CancellationToken, ScanError, ScanLimits, ScanProgress, ScanRequest},
    },
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec},
    now_ms,
};
use caskade_discovery::{Confidence, DetectedGame};
use std::collections::{BTreeSet, HashMap};

impl ScannerService {
    /// Enumerates and groups a scanner's ROMs with its stored choices
    /// applied. Blocking work runs off the async runtime.
    pub(super) async fn find(
        &self,
        scanner: &RomScanner,
        cancel: &CancellationToken,
        progress: impl FnMut(ScanProgress) + Send + 'static,
    ) -> Result<(Vec<Found>, Vec<String>)> {
        let resolved = self.resolve(scanner).await?;
        let overrides = self.store.rom_overrides(&scanner.id).await?;
        let global = self.global_exclusions().await?;
        let regions: Vec<_> = self.emulation.catalog().regions().cloned().collect();
        let (scanner, cancel) = (scanner.clone(), cancel.clone());
        let report = caskade_jobs::blocking(crate::tr!("Scanning ROMs"), move || {
            let mut progress = progress;
            let request = ScanRequest {
                scanner: &scanner,
                extensions: &resolved.extensions,
                platforms: &resolved.platforms,
                regions: &regions,
                global_exclusions: &global,
                limits: ScanLimits::default(),
                identifier: None,
            };
            scan::scan_folder_with_progress(&request, &cancel, &mut progress)
        })
        .await
        .map_err(|error| Error::Invalid(crate::tr!("ROM scan task: {error}", error = error)))?
        .map_err(scan_error)?;
        let warnings = report
            .warnings
            .iter()
            .map(|warning| match &warning.path {
                Some(path) => format!("{}: {}", path.display(), warning.message),
                None => warning.message.clone(),
            })
            .collect();
        Ok((apply_overrides(report.games, &overrides), warnings))
    }

    /// Scans and imports one scanner now (§6.1 automatic). This is what the
    /// installer's ROM hand-off runs after placing files in a scanner folder.
    pub async fn scan_import(&self, scanner_id: &str) -> Result<ScanSummary> {
        let summary = self
            .import_now(scanner_id, &CancellationToken::new(), |_| {})
            .await?;
        self.completed(std::slice::from_ref(&summary));
        Ok(summary)
    }

    /// Scans and imports every scanner that takes part in library updates.
    /// A scanner that fails reports its error and the others still run.
    pub async fn scan_all(&self) -> Result<Vec<ScanSummary>> {
        let mut summaries = Vec::new();
        for scanner in self.store.scanners().await? {
            if !scanner.in_library_update {
                continue;
            }
            let summary = match self
                .import_now(&scanner.id, &CancellationToken::new(), |_| {})
                .await
            {
                Ok(summary) => summary,
                Err(error) => ScanSummary {
                    scanner_id: scanner.id.clone(),
                    error: Some(error.to_string()),
                    ..Default::default()
                },
            };
            summaries.push(summary);
        }
        self.completed(&summaries);
        Ok(summaries)
    }

    pub(super) async fn import_now(
        &self,
        scanner_id: &str,
        cancel: &CancellationToken,
        progress: impl FnMut(ScanProgress) + Send + 'static,
    ) -> Result<ScanSummary> {
        let _activity = caskade_jobs::global().activity(crate::tr!("Importing ROMs"));
        let busy = self.busy(scanner_id);
        let _guard = busy.lock().await;
        let scanner = self.store.get_scanner(scanner_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!("no ROM scanner {scanner}", scanner = scanner_id))
        })?;
        let (games, warnings) = self.find(&scanner, cancel, progress).await?;
        if cancel.is_cancelled() {
            return Err(scan_error(ScanError::Cancelled));
        }
        let mut summary = self.reconcile(&scanner, &games).await?;
        summary.warnings.splice(0..0, warnings);
        self.store
            .set_scan_state(
                &scanner.id,
                ScanState {
                    last_scan_ms: now_ms(),
                    games: games.len() as u32,
                },
            )
            .await?;
        self.changed();
        Ok(summary)
    }

    pub(super) fn completed(&self, summaries: &[ScanSummary]) {
        self.bus.emit(Event::EmulationScanCompleted {
            scanners: summaries
                .iter()
                .filter(|summary| summary.error.is_none())
                .map(ScannerCount::from)
                .collect(),
        });
    }

    /// Updates the library from a scan (§6.2 step 7): known ROMs update their
    /// entry in place, entries whose ROMs are gone are marked not installed
    /// (never deleted), and new games go through `library.import`.
    async fn reconcile(&self, scanner: &RomScanner, games: &[Found]) -> Result<ScanSummary> {
        let mut existing = self.store.entries_for_scanner(&scanner.id).await?;
        let mut keys: BTreeSet<String> = existing
            .iter()
            .map(|entry| entry.external_id.clone())
            .collect();
        let mut by_path: HashMap<String, usize> = HashMap::new();
        for (index, entry) in existing.iter().enumerate() {
            for path in entry_paths(entry) {
                by_path.entry(path).or_insert(index);
            }
        }
        let mut used = vec![false; existing.len()];
        let mut summary = ScanSummary {
            scanner_id: scanner.id.clone(),
            ..Default::default()
        };
        let location = Some(InstallLocation::External {
            path: scanner.folder.clone(),
        });
        for Found { game, titled } in games {
            let size = game
                .roms
                .iter()
                .try_fold(0u64, |total, rom| total.checked_add(rom.size?));
            let matched = game
                .roms
                .iter()
                .filter_map(|rom| by_path.get(&rom_path(rom)?).copied())
                .find(|index| !used[*index]);
            if let Some(index) = matched {
                used[index] = true;
                let before = existing[index].clone();
                let entry = &mut existing[index];
                let (target, args) = match &entry.launch {
                    Some(LaunchSpec::Emulator { target, args, .. }) => {
                        (target.clone(), args.clone())
                    }
                    _ => (scanner.launch_with.clone(), ArgsOverride::Default),
                };
                entry.launch = Some(LaunchSpec::Emulator {
                    platform_id: game.platform.clone(),
                    target,
                    roms: game.roms.clone(),
                    args,
                });
                entry.state = InstallState::Installed;
                entry.size = size;
                entry.location = location.clone();
                summary.games.push(entry.game_id.clone());
                let mut changed = *entry != before;
                if changed {
                    self.store.upsert_entry(entry).await?;
                }
                // Beneath a title the user set (#171).
                if *titled
                    && let Some(((), renamed)) = self
                        .store
                        .update_automatic_game(&entry.game_id, |stored| {
                            game.title.clone_into(&mut stored.title);
                        })
                        .await?
                {
                    changed |= renamed;
                }
                if changed {
                    summary.updated += 1;
                    self.bus.emit(Event::LibraryGameUpdated {
                        game_id: entry.game_id.clone(),
                    });
                }
                continue;
            }
            let first = game.roms.first().and_then(rom_path).unwrap_or_default();
            let mut key = format!("{}/{first}", scanner.id);
            let mut suffix = 1;
            while keys.contains(&key) {
                suffix += 1;
                key = format!("{}/{first}#{suffix}", scanner.id);
            }
            keys.insert(key.clone());
            let mut detected = DetectedGame::new(ROMS_SOURCE, key, game.title.clone());
            detected.install_dir = Some(scanner.folder.clone());
            detected.state = InstallState::Installed;
            detected.size = size;
            detected.confidence = Confidence::Certain;
            let intercepted = self.bus.run::<LibraryImport>(detected).await;
            for (id, why) in intercepted.skipped {
                summary.warnings.push(format!("library.import {id}: {why}"));
            }
            let detected = match intercepted.outcome {
                Outcome::Continue(detected) => detected,
                Outcome::Cancelled { by, reason } => {
                    summary.warnings.push(crate::tr!(
                        "{game}: skipped by {by}: {reason}",
                        game = game.title,
                        by = by,
                        reason = reason
                    ));
                    continue;
                }
            };
            let created = Game::new(&detected.title);
            let mut entry = GameEntry::from_detected(&created.id, &detected);
            entry.launch = Some(LaunchSpec::Emulator {
                platform_id: game.platform.clone(),
                target: scanner.launch_with.clone(),
                roms: game.roms.clone(),
                args: ArgsOverride::Default,
            });
            entry.location = location.clone();
            match self.store.import_group(&created, &[entry], None).await {
                Ok(saved) => {
                    summary.added += u32::from(saved.created);
                    summary.updated += u32::from(!saved.created);
                    self.bus.emit(if saved.created {
                        Event::LibraryGameAdded {
                            game_id: saved.game_id.clone(),
                        }
                    } else {
                        Event::LibraryGameUpdated {
                            game_id: saved.game_id.clone(),
                        }
                    });
                    summary.games.push(saved.game_id);
                }
                Err(error) => summary.warnings.push(format!("{}: {error}", created.title)),
            }
        }
        for (index, mut entry) in existing.into_iter().enumerate() {
            if used[index] || entry.state == InstallState::Partial {
                continue;
            }
            entry.state = InstallState::Partial;
            entry.size = None;
            self.store.upsert_entry(&entry).await?;
            summary.missing += 1;
            self.bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id,
            });
        }
        Ok(summary)
    }
}
