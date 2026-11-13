//! ROMs the installer hands off (#120). With a ROM
//! scanner for the platform, the job asks whether to move or copy the files
//! into its folder (move by default on the same filesystem), then runs that
//! scanner. Otherwise the ROMs are imported where they are.

use super::{InstallService, JobStatus, lock};
use crate::{
    Error, Result,
    bus::Event,
    emulation::{EmulationService, RomLocation, RomScanner},
    model::LaunchSpec,
};
use caskade_installer::{Destination, Question};
use std::path::{Path, PathBuf};

/// A ROM install waiting for the user to choose where the files go.
pub(super) struct PendingRoms {
    platform: String,
    base: PathBuf,
    files: Vec<PathBuf>,
    /// Parallel to the question's options: the scanner to use, if any.
    scanners: Vec<Option<RomScanner>>,
    copies: Vec<bool>,
}

#[cfg(unix)]
fn same_filesystem(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_filesystem(a: &Path, b: &Path) -> bool {
    a.components().next() == b.components().next()
}

/// Moves or copies `files` (relative to `base`) into `folder`, keeping
/// their relative paths. Nothing is overwritten.
fn transfer(base: &Path, files: &[PathBuf], folder: &Path, copy: bool) -> Result<()> {
    for file in files {
        let target = folder.join(file);
        if target.exists() {
            return Err(Error::Invalid(crate::tr!(
                "{file} is already in the scanner folder",
                file = file.display()
            )));
        }
    }
    for file in files {
        let (from, to) = (base.join(file), folder.join(file));
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if copy {
            std::fs::copy(&from, &to)?;
        } else if std::fs::rename(&from, &to).is_err() {
            // Another filesystem: copy, then remove the original.
            std::fs::copy(&from, &to)?;
            std::fs::remove_file(&from)?;
        }
    }
    Ok(())
}

impl InstallService {
    /// Finishes a ROM hand-off: asks where the files go when a scanner
    /// covers the platform, otherwise imports them in place.
    pub(super) async fn route_roms(
        &self,
        id: &str,
        emulation: Option<&EmulationService>,
        platform: &str,
        base: &Path,
        files: &[PathBuf],
    ) -> Result<bool> {
        let Some(emulation) = emulation else {
            return Ok(false);
        };
        let scanners = emulation.scanners().for_platform(platform).await?;
        if scanners.is_empty() {
            return Ok(false);
        }
        let mut options = Vec::new();
        let mut targets = Vec::new();
        let mut copies = Vec::new();
        for scanner in &scanners {
            let folder = scanner.folder.display();
            let mut pair = [
                (
                    false,
                    crate::tr!(
                        "Move into {scanner} ({folder})",
                        scanner = scanner.name,
                        folder = folder
                    ),
                ),
                (
                    true,
                    crate::tr!(
                        "Copy into {scanner} ({folder})",
                        scanner = scanner.name,
                        folder = folder
                    ),
                ),
            ];
            if !same_filesystem(base, &scanner.folder) {
                pair.reverse();
            }
            for (copy, label) in pair {
                options.push(Destination {
                    label,
                    folder: Some(scanner.folder.clone()),
                    copy,
                });
                targets.push(Some(scanner.clone()));
                copies.push(copy);
            }
        }
        options.push(Destination {
            label: crate::tr!("Keep them where they are"),
            folder: None,
            copy: false,
        });
        targets.push(None);
        copies.push(false);
        lock(&self.inner.pending_roms).insert(
            id.to_owned(),
            PendingRoms {
                platform: platform.to_owned(),
                base: base.to_path_buf(),
                files: files.to_vec(),
                scanners: targets,
                copies,
            },
        );
        self.update(id, |job| {
            job.status = JobStatus::NeedsUser;
            job.question = Some(Question::ChooseDestination {
                reason: "A ROM scanner keeps these ROMs with the rest of the platform's games."
                    .into(),
                options,
            });
        });
        Ok(true)
    }

    /// The answer to a ROM destination question.
    pub(super) async fn place_roms(&self, id: &str, index: u32) -> Result<super::JobSnapshot> {
        let pending = lock(&self.inner.pending_roms)
            .remove(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("this job isn't waiting for a ROM folder")))?;
        let index = index as usize;
        let Some(scanner) = pending.scanners.get(index).cloned() else {
            lock(&self.inner.pending_roms).insert(id.to_owned(), pending);
            return Err(Error::Invalid(crate::tr!("choice is out of range")));
        };
        self.update(id, |job| {
            job.status = JobStatus::Installing;
            job.question = None;
        });
        let emulation = lock(&self.inner.emulation).clone();
        let outcome = match scanner {
            None => {
                self.import_roms(
                    id,
                    emulation.as_ref(),
                    &pending.platform,
                    &pending.base,
                    &pending.files,
                )
                .await
            }
            Some(scanner) => {
                self.scan_roms(id, emulation, &scanner, &pending, pending.copies[index])
                    .await
            }
        };
        if let Err(error) = outcome {
            self.update(id, |job| {
                job.status = JobStatus::Failed;
                job.error = Some(error.to_string());
            });
        }
        self.job(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("This job is no longer in Activity.")))
    }

    async fn scan_roms(
        &self,
        id: &str,
        emulation: Option<EmulationService>,
        scanner: &RomScanner,
        pending: &PendingRoms,
        copy: bool,
    ) -> Result<()> {
        let emulation =
            emulation.ok_or_else(|| Error::Invalid(crate::tr!("emulation isn't available")))?;
        let (base, files, folder) = (
            pending.base.clone(),
            pending.files.clone(),
            scanner.folder.clone(),
        );
        caskade_jobs::blocking(crate::tr!("Placing ROMs"), move || {
            transfer(&base, &files, &folder, copy)
        })
        .await??;
        let summary = emulation.scanners().scan_import(&scanner.id).await?;
        // The game whose ROMs are the files just placed.
        let store = &self.inner.store;
        let wanted: Vec<String> = pending
            .files
            .iter()
            .map(|file| file.to_string_lossy().replace('\\', "/"))
            .collect();
        let mut found = None;
        'games: for game_id in &summary.games {
            for entry in store.entries_for_game(game_id).await? {
                let Some(LaunchSpec::Emulator { roms, .. }) = &entry.launch else {
                    continue;
                };
                let placed = roms.iter().any(|rom| {
                    matches!(&rom.location, RomLocation::Scanner { scanner_id, path }
                        if *scanner_id == scanner.id
                            && wanted.contains(&path.to_string_lossy().replace('\\', "/")))
                });
                if placed {
                    found = Some(game_id.clone());
                    break 'games;
                }
            }
        }
        let game_id = found.ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "the {scanner} scanner didn't import the ROM; check its extensions and exclusions",
                scanner = scanner.name
            ))
        })?;
        let title = store
            .game(&game_id)
            .await?
            .map(|game| game.title)
            .unwrap_or_default();
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: game_id.clone(),
        });
        self.update(id, |job| {
            job.status = JobStatus::Completed;
            job.title = title;
            job.game_id = Some(game_id);
            job.warnings = Vec::new();
        });
        Ok(())
    }
}

impl InstallService {
    /// Finishes handed-off ROMs: into a ROM scanner's
    /// folder if the user picks one, otherwise where they are.
    pub(super) async fn finish_roms(
        &self,
        id: &str,
        installer_platform: &str,
        files: &[std::path::PathBuf],
        source: &std::path::Path,
    ) -> Result<()> {
        let emulation = lock(&self.inner.emulation).clone();
        let platform = emulation
            .as_ref()
            .and_then(|emulation| {
                crate::emulation::platform_for(&emulation.catalog(), installer_platform, files)
            })
            .unwrap_or_else(|| installer_platform.to_owned());
        let base = if source.is_dir() {
            source
        } else {
            source.parent().unwrap_or(source)
        };
        if self
            .route_roms(id, emulation.as_ref(), &platform, base, files)
            .await?
        {
            return Ok(());
        }
        self.import_roms(id, emulation.as_ref(), &platform, base, files)
            .await
    }

    /// Imports ROMs where they are (§9 step 2), updating the game when the
    /// same ROM was installed before. The job says when no scanner or
    /// emulator handles the platform yet.
    async fn import_roms(
        &self,
        id: &str,
        emulation: Option<&crate::emulation::EmulationService>,
        platform: &str,
        base: &std::path::Path,
        files: &[std::path::PathBuf],
    ) -> Result<()> {
        let store = &self.inner.store;
        let platform = platform.to_owned();
        let catalog = emulation.map(|emulation| emulation.catalog());
        let (game, mut entry) =
            crate::emulation::rom_entry(&platform, base, files, catalog.as_deref());
        let game = match store
            .entry_by_source(entry.source_id.as_str(), &entry.external_id)
            .await?
        {
            Some(existing) => {
                entry.id = existing.id;
                entry.game_id = existing.game_id.clone();
                store
                    .game(&existing.game_id)
                    .await?
                    .ok_or_else(|| Error::Invalid(crate::tr!("the ROM's game is missing")))?
            }
            None => store.upsert_game(&game).await?,
        };
        store.upsert_entry(&entry).await?;

        let name = catalog
            .as_deref()
            .and_then(|catalog| catalog.platform(&platform))
            .map_or(platform.clone(), |platform| platform.short.clone());
        let warning = |code: &str, message: String| caskade_installer::Warning {
            code: code.into(),
            message,
            path: None,
        };
        let mut warnings = Vec::new();
        let scanned = store
            .scanners()
            .await?
            .iter()
            .any(|scanner| scanner.platform.as_deref() == Some(platform.as_str()));
        if !scanned {
            warnings.push(warning(
                "rom_scanner_suggested",
                crate::tr!(
                    "Add a ROM scanner for {name} in Settings → Emulators to keep its ROMs together.",
                    name = name
                ),
            ));
        }
        let has_default = store
            .get_platform_settings(&platform)
            .await?
            .is_some_and(|settings| settings.default.is_some());
        if !has_default {
            warnings.push(warning(
                "rom_no_emulator",
                crate::tr!(
                    "No emulator for {name} yet — configure one in Settings → Emulators.",
                    name = name
                ),
            ));
        }
        self.inner.bus.emit(Event::LibraryGameAdded {
            game_id: game.id.clone(),
        });
        self.inner.bus.emit(Event::EmulationChanged {
            scope: "platforms".into(),
        });
        self.update(id, |job| {
            job.status = JobStatus::Completed;
            job.title = game.title.clone();
            job.game_id = Some(game.id.clone());
            job.warnings = warnings;
        });
        Ok(())
    }
}
