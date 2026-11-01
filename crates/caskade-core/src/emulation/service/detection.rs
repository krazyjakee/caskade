//! Emulator detection jobs (§5): detect in the background, review, import.

use super::{Availability, EmulationService};
use crate::{
    Error, Result,
    bus::Event,
    emulation::{DetectedInstall, EmulatorInstall, PlatformSettings, ProfileRef, detect},
    lock,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, path::PathBuf};

/// How a detection ended, or that it is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum DetectStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// A detection and what it found (§5 steps 1–4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectJob {
    pub id: String,
    /// Bumped on every change; imports name the revision they reviewed.
    pub revision: u32,
    pub status: DetectStatus,
    pub folder: Option<String>,
    pub results: Vec<DetectedInstall>,
    pub error: Option<String>,
}

/// What an import created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectImport {
    /// Ids of the new emulators.
    pub emulators: Vec<String>,
    /// Platforms that got a default (§5 step 5).
    pub defaults_set: Vec<String>,
}

pub(super) struct DetectState {
    job: DetectJob,
    cancel: caskade_discovery::Cancel,
}

impl EmulationService {
    /// Detects against another machine (tests, fixtures).
    pub fn with_host_env(self, env: caskade_discovery::HostEnv) -> Self {
        self.configure(|inner| inner.env = env)
    }

    fn update_job(&self, id: &str, change: impl FnOnce(&mut DetectJob)) -> Option<DetectJob> {
        let job = {
            let mut jobs = lock(&self.inner.jobs);
            let state = jobs.get_mut(id)?;
            change(&mut state.job);
            state.job.revision += 1;
            state.job.clone()
        };
        self.inner
            .bus
            .emit(Event::EmulationDetectChanged(Box::new(job.clone())));
        Some(job)
    }

    /// Starts detecting emulators in standard locations and, if given, a
    /// user folder. Progress and results arrive as `emulation.detect.changed`.
    pub async fn detect_start(&self, folder: Option<PathBuf>) -> Result<DetectJob> {
        let configured = self.inner.store.emulators().await?;
        let cancel = caskade_discovery::Cancel::new();
        let job = DetectJob {
            id: crate::model::new_id(),
            revision: 0,
            status: DetectStatus::Running,
            folder: folder
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned()),
            results: Vec::new(),
            error: None,
        };
        {
            let mut jobs = lock(&self.inner.jobs);
            // Finished detections beyond the last few are dropped.
            if jobs.len() >= 8 {
                let finished: Vec<String> = jobs
                    .iter()
                    .filter(|(_, state)| state.job.status != DetectStatus::Running)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in finished.iter().take(jobs.len() - 7) {
                    jobs.remove(id);
                }
            }
            jobs.insert(
                job.id.clone(),
                DetectState {
                    job: job.clone(),
                    cancel: cancel.clone(),
                },
            );
        }
        self.inner
            .bus
            .emit(Event::EmulationDetectChanged(Box::new(job.clone())));
        let service = self.clone();
        let id = job.id.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Detecting emulators"));
            let catalog = service.catalog();
            let env = service.inner.env.clone();
            let ctx_cancel = cancel.clone();
            let outcome = caskade_jobs::blocking(crate::tr!("Detecting emulators"), move || {
                let ctx = caskade_discovery::ScanCtx::new(ctx_cancel);
                detect(&catalog, &configured, &env, &ctx, folder.as_deref())
            })
            .await;
            service.update_job(&id, |job| {
                if job.status != DetectStatus::Running {
                    return;
                }
                match outcome {
                    Ok(Ok(results)) => {
                        job.status = DetectStatus::Done;
                        job.results = results;
                    }
                    Ok(Err(caskade_discovery::SourceError::Cancelled)) => {
                        job.status = DetectStatus::Cancelled;
                    }
                    Ok(Err(error)) => {
                        job.status = DetectStatus::Failed;
                        job.error = Some(error.to_string());
                    }
                    Err(error) => {
                        job.status = DetectStatus::Failed;
                        job.error = Some(error.to_string());
                    }
                }
            });
        });
        Ok(job)
    }

    pub fn detect_get(&self, id: &str) -> Option<DetectJob> {
        lock(&self.inner.jobs)
            .get(id)
            .map(|state| state.job.clone())
    }

    /// The most recent detection, e.g. the one a library scan started.
    pub fn detect_latest(&self) -> Option<DetectJob> {
        lock(&self.inner.jobs)
            .values()
            .map(|state| state.job.clone())
            .max_by(|a, b| a.id.cmp(&b.id))
    }

    pub fn detect_cancel(&self, id: &str) -> Result<DetectJob> {
        let cancel = lock(&self.inner.jobs)
            .get(id)
            .map(|state| state.cancel.clone())
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown detection {id}", id = id)))?;
        cancel.cancel();
        self.update_job(id, |job| {
            if job.status == DetectStatus::Running {
                job.status = DetectStatus::Cancelled;
            }
        })
        .ok_or_else(|| Error::Invalid(crate::tr!("unknown detection {id}", id = id)))
    }

    /// Imports the ticked installs of a finished detection (§5 step 4),
    /// then gives every platform without a default the first available
    /// profile for it, in definition order (step 5).
    pub async fn detect_import(
        &self,
        id: &str,
        revision: u32,
        keys: &[String],
    ) -> Result<DetectImport> {
        let job = self
            .detect_get(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown detection {id}", id = id)))?;
        if job.status != DetectStatus::Done {
            return Err(Error::Invalid(crate::tr!("the detection has not finished")));
        }
        if job.revision != revision {
            return Err(Error::Invalid(crate::tr!(
                "the detection changed since it was reviewed; review it again"
            )));
        }
        let selected: Vec<&DetectedInstall> = job
            .results
            .iter()
            .filter(|found| keys.contains(&found.key) && !found.configured)
            .collect();
        let existing = self.inner.store.emulators().await?;
        let mut names: Vec<String> = existing
            .iter()
            .map(|emulator| emulator.name.clone())
            .collect();
        let mut imported = Vec::new();
        for found in &selected {
            let shared = selected
                .iter()
                .filter(|other| other.definition_id == found.definition_id)
                .count()
                > 1
                || names.contains(&found.name);
            let name = if shared {
                format!("{} ({})", found.name, install_label(&found.install))
            } else {
                found.name.clone()
            };
            names.push(name.clone());
            let view = self
                .save_emulator(detect::emulator_for(found, name))
                .await?;
            imported.push(view);
        }

        let mut settings: BTreeMap<String, PlatformSettings> = self
            .inner
            .store
            .platform_settings()
            .await?
            .into_iter()
            .map(|settings| (settings.platform.clone(), settings))
            .collect();
        let mut defaults_set = Vec::new();
        for view in &imported {
            for profile in &view.profiles {
                if profile.availability != Availability::Available {
                    continue;
                }
                for platform in &profile.platforms {
                    let current = settings
                        .entry(platform.clone())
                        .or_insert(PlatformSettings {
                            platform: platform.clone(),
                            default: None,
                            bios_dir: None,
                        });
                    if current.default.is_some() {
                        continue;
                    }
                    current.default = Some(ProfileRef {
                        emulator_id: view.emulator.id.clone(),
                        profile_id: profile.id.clone(),
                    });
                    self.inner.store.set_platform_settings(current).await?;
                    defaults_set.push(platform.clone());
                }
            }
        }
        if !defaults_set.is_empty() {
            self.changed("platforms");
        }
        let imported_keys: Vec<&str> = selected.iter().map(|found| found.key.as_str()).collect();
        self.update_job(id, |job| {
            for found in &mut job.results {
                if imported_keys.contains(&found.key.as_str()) {
                    found.configured = true;
                }
            }
        });
        Ok(DetectImport {
            emulators: imported.into_iter().map(|view| view.emulator.id).collect(),
            defaults_set,
        })
    }

    /// Detects after every library scan, so a newly installed emulator is
    /// offered without opening Settings. Nothing is imported automatically.
    pub fn watch_library_scans(&self) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(Event::LibraryScanCompleted { .. }) => {
                        if let Err(error) = service.detect_start(None).await {
                            tracing::warn!(%error, "emulator detection after a library scan failed");
                        }
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        })
    }
}

fn install_label(install: &EmulatorInstall) -> &'static str {
    match install {
        EmulatorInstall::Folder { .. } => "Folder",
        EmulatorInstall::Path { .. } => "PATH",
        EmulatorInstall::Flatpak { .. } => "Flatpak",
        EmulatorInstall::AppImage { .. } => "AppImage",
        EmulatorInstall::AppBundle { .. } => "App",
    }
}
