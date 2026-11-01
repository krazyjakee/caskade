//! Background scan jobs, the review list and its submission.

use super::{
    JobState, ReviewChoice, ReviewGame, ReviewRom, ScanJob, ScanMode, ScanStatus, ScannerService,
    entry_paths,
    overrides::{Choices, Found, review_choices},
    rom_path,
};
use crate::{
    Error, Result,
    bus::Event,
    emulation::{
        RomScanner,
        scan::{CancellationToken, ScanPhase, ScanProgress},
    },
    lock,
    model::new_id,
};
use std::collections::HashMap;

impl ScannerService {
    fn update_job(&self, id: &str, change: impl FnOnce(&mut ScanJob)) -> Option<ScanJob> {
        let job = {
            let mut jobs = lock(&self.shared.jobs);
            let state = jobs.get_mut(id)?;
            change(&mut state.job);
            state.job.revision += 1;
            state.job.clone()
        };
        self.bus
            .emit(Event::EmulationScanChanged(Box::new(job.clone())));
        Some(job)
    }

    /// Starts a scan in the background. `Review` stops at a review list for
    /// [`Self::submit`]; `Import` imports when it is done. Changes arrive as
    /// `emulation.scan.changed`.
    pub async fn start(&self, scanner_id: &str, mode: ScanMode) -> Result<ScanJob> {
        let scanner = self.store.get_scanner(scanner_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!("no ROM scanner {scanner}", scanner = scanner_id))
        })?;
        let cancel = CancellationToken::new();
        let job = ScanJob {
            id: new_id(),
            revision: 0,
            scanner_id: scanner.id.clone(),
            mode,
            status: ScanStatus::Running,
            progress: None,
            review: Vec::new(),
            summary: None,
            warnings: Vec::new(),
            error: None,
        };
        {
            let mut jobs = lock(&self.shared.jobs);
            // Finished scans beyond the last few are dropped.
            let finished: Vec<String> = jobs
                .iter()
                .filter(|(_, state)| {
                    !matches!(state.job.status, ScanStatus::Running | ScanStatus::Review)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in finished.iter().take(finished.len().saturating_sub(7)) {
                jobs.remove(id);
            }
            jobs.insert(
                job.id.clone(),
                JobState {
                    job: job.clone(),
                    cancel: cancel.clone(),
                },
            );
        }
        self.bus
            .emit(Event::EmulationScanChanged(Box::new(job.clone())));
        let service = self.clone();
        let id = job.id.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Scanning ROMs"));
            let reporter = service.clone();
            let progress_id = id.clone();
            let mut last: Option<ScanProgress> = None;
            let progress = move |next: ScanProgress| {
                // Phase changes and every 250 files, not every file.
                if last.is_some_and(|last| {
                    last.phase == next.phase && next.files < last.files.saturating_add(250)
                }) && next.phase != ScanPhase::Group
                {
                    return;
                }
                last = Some(next);
                reporter.update_job(&progress_id, |job| job.progress = Some(next));
            };
            let outcome = match mode {
                ScanMode::Import => service
                    .import_now(&scanner.id, &cancel, progress)
                    .await
                    .map(|summary| {
                        service.completed(std::slice::from_ref(&summary));
                        (Vec::new(), Some(summary), Vec::new())
                    }),
                ScanMode::Review => service
                    .review(&scanner, &cancel, progress)
                    .await
                    .map(|(review, warnings)| (review, None, warnings)),
            };
            service.update_job(&id, |job| {
                if job.status != ScanStatus::Running {
                    return;
                }
                match outcome {
                    Ok((review, summary, warnings)) => {
                        job.status = if summary.is_some() {
                            ScanStatus::Done
                        } else {
                            ScanStatus::Review
                        };
                        job.review = review;
                        job.summary = summary;
                        job.warnings = warnings;
                    }
                    Err(_) if cancel.is_cancelled() => job.status = ScanStatus::Cancelled,
                    Err(error) => {
                        job.status = ScanStatus::Failed;
                        job.error = Some(error.to_string());
                    }
                }
            });
        });
        Ok(job)
    }

    async fn review(
        &self,
        scanner: &RomScanner,
        cancel: &CancellationToken,
        progress: impl FnMut(ScanProgress) + Send + 'static,
    ) -> Result<(Vec<ReviewGame>, Vec<String>)> {
        let (games, warnings) = self.find(scanner, cancel, progress).await?;
        let mut owners: HashMap<String, String> = HashMap::new();
        for entry in self.store.entries_for_scanner(&scanner.id).await? {
            for path in entry_paths(&entry) {
                owners.entry(path).or_insert_with(|| entry.game_id.clone());
            }
        }
        let review = games
            .into_iter()
            .map(|Found { game, .. }| {
                let roms: Vec<ReviewRom> = game
                    .roms
                    .iter()
                    .filter_map(|rom| {
                        Some(ReviewRom {
                            path: rom_path(rom)?,
                            name: rom.name.clone(),
                            size: rom.size,
                        })
                    })
                    .collect();
                ReviewGame {
                    game_id: roms.iter().find_map(|rom| owners.get(&rom.path).cloned()),
                    title: game.title,
                    platform: game.platform,
                    regions: game.regions,
                    roms,
                    files: game
                        .files
                        .iter()
                        .map(|file| file.to_string_lossy().replace('\\', "/"))
                        .collect(),
                }
            })
            .collect();
        Ok((review, warnings))
    }

    pub fn get(&self, id: &str) -> Option<ScanJob> {
        lock(&self.shared.jobs)
            .get(id)
            .map(|state| state.job.clone())
    }

    /// Retained scans, newest first; the UI resumes a review from here.
    pub fn jobs(&self) -> Vec<ScanJob> {
        let mut jobs: Vec<ScanJob> = lock(&self.shared.jobs)
            .values()
            .map(|state| state.job.clone())
            .collect();
        jobs.sort_by(|a, b| b.id.cmp(&a.id));
        jobs
    }

    pub fn cancel(&self, id: &str) -> Option<ScanJob> {
        let running = {
            let jobs = lock(&self.shared.jobs);
            let state = jobs.get(id)?;
            state.cancel.cancel();
            matches!(state.job.status, ScanStatus::Running | ScanStatus::Review)
        };
        if running {
            self.update_job(id, |job| job.status = ScanStatus::Cancelled)
        } else {
            self.get(id)
        }
    }

    /// Stores the choices of a reviewed scan as exclusions and overrides,
    /// then imports with them, exactly as the next automatic scan will.
    pub async fn submit(
        &self,
        job_id: &str,
        revision: u32,
        choices: Vec<ReviewChoice>,
    ) -> Result<ScanJob> {
        let job = self
            .get(job_id)
            .ok_or_else(|| Error::Invalid(crate::tr!("no ROM scan {job}", job = job_id)))?;
        if job.status != ScanStatus::Review {
            return Err(Error::Invalid(crate::tr!(
                "this scan is not waiting for a review"
            )));
        }
        if job.revision != revision {
            return Err(Error::Invalid(crate::tr!(
                "the scan changed; review the latest results"
            )));
        }
        let busy = self.busy(&job.scanner_id);
        {
            let _guard = busy.lock().await;
            let mut scanner = self
                .store
                .get_scanner(&job.scanner_id)
                .await?
                .ok_or_else(|| {
                    Error::Invalid(crate::tr!(
                        "no ROM scanner {scanner}",
                        scanner = job.scanner_id
                    ))
                })?;
            let stored = self.store.rom_overrides(&scanner.id).await?;
            let Choices {
                exclusions,
                overrides,
            } = review_choices(&job.review, &choices, &stored)?;
            for pattern in exclusions {
                if !scanner.exclusions.contains(&pattern) {
                    scanner.exclusions.push(pattern);
                }
            }
            self.store.upsert_scanner(&scanner).await?;
            self.store
                .set_rom_overrides(&scanner.id, &overrides)
                .await?;
        }
        self.update_job(job_id, |job| {
            job.status = ScanStatus::Running;
            job.mode = ScanMode::Import;
        });
        let cancel = lock(&self.shared.jobs)
            .get(job_id)
            .map(|state| state.cancel.clone())
            .unwrap_or_default();
        let outcome = self.import_now(&job.scanner_id, &cancel, |_| {}).await;
        if let Ok(summary) = &outcome {
            self.completed(std::slice::from_ref(summary));
        }
        self.update_job(job_id, |job| match outcome {
            Ok(summary) => {
                job.status = ScanStatus::Done;
                job.summary = Some(summary);
            }
            Err(error) => {
                job.status = ScanStatus::Failed;
                job.error = Some(error.to_string());
            }
        })
        .ok_or_else(|| Error::Invalid(crate::tr!("no ROM scan {job}", job = job_id)))
    }
}
