//! Installing games into storage roots (#51, INSTALLER-PLAN §3, §10): job
//! queues over the installer pipeline, and install outcomes as library
//! games.
//!
//! Downloads run up to `downloads` at a time; installs run one at a time per
//! storage root. Every job is a [`JobSnapshot`] published on the bus
//! (`install.job.changed`) on each change. A finished install becomes a
//! `Game` and a `caskade-storage` entry keyed on the install's stable id,
//! launched through its alias.

mod carry;
mod downloads;
mod failure_text;
pub use failure_text::failure_message;
pub mod dropped;
pub mod hooks;
mod jobs;
mod mapping;
mod operations;
mod peer;
mod recovery;
mod remove;
mod rescan;
mod roms;
mod settings;
mod source;
mod update;
pub use operations::{JobAnswer, Verification};
pub use recovery::Recovery;
pub use settings::{InstallerRunnerResolver, runner_paths};

pub use jobs::{JobKind, JobPhase, JobProgress, JobSnapshot, JobStatus};
pub use mapping::{
    HANDOFF_SOURCE, MANAGED_PREFIX, NEEDS_EMULATOR, STORAGE_SOURCE, from_handoff, from_manifest,
};

use crate::{
    Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    lock,
    model::StorageRoot,
    now_ms,
};
use caskade_installer::{
    InstallOutcome,
    discover::DiscoveryHints,
    events::{EventSink, JobEvent},
    exec::CancellationToken,
    manifest::InstallManifest,
    pipeline::{self, Context},
    runtime::alias::AliasStore,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Queue sizes and the alias store.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Downloads that may run at once.
    pub downloads: usize,
    /// Where game aliases live; `None` installs without one.
    pub aliases: Option<AliasStore>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            downloads: 2,
            aliases: AliasStore::for_host(),
        }
    }
}

pub use peer::{FetchFailure, Fetched, PeerCopyTarget, PeerFetcher, PeerProgress};

struct Job {
    snapshot: JobSnapshot,
    cancel: CancellationToken,
    control: pipeline::RunControl,
    active: bool,
    /// What an update job updates.
    update: Option<update::UpdateTarget>,
    /// What the caller knew about the game; a retry keeps it.
    hints: DiscoveryHints,
    /// What a copy from another device brings (#233).
    peer: Option<peer::PeerCopyTarget>,
}

/// A run's input, downloaded or read, and what installing it needs.
struct Ready {
    prepared: pipeline::input::Prepared,
    host: caskade_installer::host::HostCaps,
    job_id: String,
    preferences: crate::settings::InstallerSettings,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    options: InstallOptions,
    downloads: Arc<downloads::DownloadQueue>,
    settings: Mutex<Option<crate::settings::SettingsService>>,
    host: Mutex<Option<caskade_installer::host::HostCaps>>,
    tool: Mutex<Option<caskade_installer::formats::archive::sevenzip::SevenZip>>,
    storage: Mutex<Option<crate::storage::StorageService>>,
    emulation: Mutex<Option<crate::emulation::EmulationService>>,
    /// ROM installs waiting for the user to pick a folder (#120).
    pending_roms: Mutex<HashMap<String, roms::PendingRoms>>,
    /// Follows finished installs for updates.
    tracker: Mutex<Option<crate::updates::Tracker>>,
    roots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    jobs: Mutex<BTreeMap<String, Job>>,
    /// Failure repeats and automatic retries (#158).
    recovery: recovery::State,
    /// Where copies from other devices come from (#233).
    peer_fetcher: Mutex<Option<Arc<dyn peer::PeerFetcher>>>,
}

/// The install queues; cheap to clone.
#[derive(Clone)]
pub struct InstallService {
    inner: Arc<Inner>,
}

impl InstallService {
    pub fn new(store: LibraryStore, bus: Bus, options: InstallOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                downloads: downloads::DownloadQueue::new(options.downloads),
                settings: Mutex::default(),
                host: Mutex::default(),
                store,
                bus,
                options,
                roots: Mutex::default(),
                jobs: Mutex::default(),
                tool: Mutex::default(),
                storage: Mutex::default(),
                emulation: Mutex::default(),
                pending_roms: Mutex::default(),
                tracker: Mutex::default(),
                recovery: recovery::State::default(),
                peer_fetcher: Mutex::new(None),
            }),
        }
    }

    /// Picks roots through the storage service, so its root choices (the
    /// default root, "always use default") apply to installs.
    pub fn with_storage(self, storage: crate::storage::StorageService) -> Self {
        *lock(&self.inner.storage) = Some(storage);
        self
    }

    /// ROM hand-offs map to the emulation catalog's platforms and report
    /// missing scanners and emulators (#120). Settable after construction
    /// because the app builds the emulation service later.
    pub fn set_emulation(&self, emulation: crate::emulation::EmulationService) {
        *lock(&self.inner.emulation) = Some(emulation);
    }

    /// Finished installs are followed for updates through `tracker`.
    /// Settable after construction because the app builds the tracker
    /// later.
    pub fn set_tracker(&self, tracker: crate::updates::Tracker) {
        *lock(&self.inner.tracker) = Some(tracker);
    }

    fn storage(&self) -> Option<crate::storage::StorageService> {
        lock(&self.inner.storage).clone()
    }

    /// The root to install into: `root_id`, or the default root.
    pub async fn root(&self, root_id: Option<&str>) -> Result<StorageRoot> {
        if let Some(storage) = self.storage() {
            return storage
                .resolve(root_id)
                .await
                .map_err(|error| Error::Invalid(error.to_string()));
        }
        let store = &self.inner.store;
        let root = match root_id {
            Some(id) => store.get_root(id).await?,
            None => {
                let roots = store.roots().await?;
                roots
                    .iter()
                    .find(|root| root.is_default)
                    .or(roots.first())
                    .cloned()
            }
        };
        let root = root.ok_or_else(|| {
            Error::Invalid(match root_id {
                Some(id) => crate::tr!("unknown storage root {root}", root = id),
                None => crate::tr!("no storage root is configured"),
            })
        })?;
        let checked = root.clone();
        tokio::task::spawn_blocking(move || {
            let identity = caskade_installer::manifest::RootManifest::read(&checked.path)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if identity.root_id() != checked.id {
                return Err(Error::Invalid(crate::tr!("storage root identity changed")));
            }
            Ok::<_, Error>(())
        })
        .await
        .map_err(|e| Error::Invalid(e.to_string()))??;
        Ok(root)
    }

    /// Queues an install of a path or URL and returns its first snapshot.
    pub async fn install(&self, input: String, root_id: Option<String>) -> Result<JobSnapshot> {
        self.install_with(input, root_id, DiscoveryHints::default())
            .await
    }

    /// [`install`](Self::install) with what the caller already knows about
    /// the game, e.g. a dropped shortcut's title and target
    /// ([`dropped::DropPlan::hints`]).
    pub async fn install_with(
        &self,
        input: String,
        root_id: Option<String>,
        hints: DiscoveryHints,
    ) -> Result<JobSnapshot> {
        let root = self.root(root_id.as_deref()).await?;
        let title = hints
            .installer_title
            .clone()
            .unwrap_or_else(|| title_of(&input));
        let snapshot = JobSnapshot::queued(
            JobKind::Install,
            title,
            input.clone(),
            Some(root.id.clone()),
        );
        let cancel = self.add_job(snapshot.clone(), None, hints);
        self.lifecycle(&snapshot);
        self.spawn_install(snapshot.id.clone(), input, root, cancel);
        Ok(snapshot)
    }

    /// Installs, updates, uninstalls and rescans take a root's lock, one at
    /// a time.
    fn root_lock(&self, root_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.inner.roots)
            .entry(root_id.to_owned())
            .or_default()
            .clone()
    }

    /// Lists `snapshot`, a new job, and publishes it. Returns its cancel
    /// token.
    fn add_job(
        &self,
        snapshot: JobSnapshot,
        update: Option<update::UpdateTarget>,
        hints: DiscoveryHints,
    ) -> CancellationToken {
        let cancel = CancellationToken::new();
        lock(&self.inner.jobs).insert(
            snapshot.id.clone(),
            Job {
                snapshot: snapshot.clone(),
                cancel: cancel.clone(),
                control: pipeline::RunControl::default(),
                active: true,
                update,
                hints,
                peer: None,
            },
        );
        self.publish(&snapshot);
        cancel
    }

    async fn run(
        &self,
        id: &str,
        input: String,
        root: StorageRoot,
        cancel: CancellationToken,
    ) -> Result<()> {
        let mut ctx = self.context(id, cancel.clone());
        let Some(ready) = self
            .prepare_run(id, &input, &root, &mut ctx, &cancel)
            .await?
        else {
            return Ok(());
        };
        let source = ready.prepared.path.clone();
        let download = ready.prepared.download_url.clone();
        let remote = pipeline::input::downloads(&input);
        let cleanup = source::SourceCleanup::capture(
            self,
            &ready.prepared,
            &input,
            // A copy's received folder is Caskade's own, never the user's to
            // keep or bin (#233).
            if lock(&self.inner.jobs)
                .get(id)
                .is_some_and(|job| job.peer.is_some())
            {
                crate::settings::SourceHandling::Delete
            } else {
                ready.preferences.source_handling
            },
            cancel.clone(),
        )
        .await?;

        // Install, one job per root at a time.
        let root_lock = self.root_lock(&root.id);
        let _guard = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
            guard = root_lock.lock() => guard,
        };
        self.update(id, |job| {
            job.status = JobStatus::Installing;
            job.phase = Some(JobPhase::Install);
            // The download's last report isn't the install's.
            job.progress = None;
        });
        let outcome = pipeline::install_prepared(
            ready.prepared,
            root.path.clone(),
            ready.job_id,
            ready.host,
            ready.preferences.keep_downloads,
            false,
            &ctx,
        )
        .await
        .map_err(|error| {
            self.failed(
                id,
                &*error,
                caskade_installer::failure::Probe {
                    stage: self.running_stage(id),
                    source: Some(&source),
                    remote,
                    root: Some(&root.path),
                },
            )
        })?;
        self.sync_plans(id);
        self.finish(
            id,
            &root,
            &input,
            (&source, download.as_ref()),
            outcome,
            (cleanup, cancel),
        )
        .await
    }

    /// Sets `ctx` up for a run into `root`, then downloads `input` (under
    /// the download limit) or reads it. `None` when the job now waits on a
    /// question.
    async fn prepare_run(
        &self,
        id: &str,
        input: &str,
        root: &StorageRoot,
        ctx: &mut Context,
        cancel: &CancellationToken,
    ) -> Result<Option<Ready>> {
        let preferences = self.preferences().await?;
        ctx.config = preferences.config();
        ctx.host = lock(&self.inner.host).clone();
        self.inner
            .downloads
            .set_limit(preferences.concurrent_downloads as usize);
        if let Some(storage) = self.storage()
            && ctx.folder_template.is_none()
        {
            ctx.folder_template = Some(
                storage
                    .folder_template(&root.id)
                    .await
                    .map_err(|error| Error::Invalid(error.to_string()))?,
            );
        }
        let failed =
            |error: Box<dyn std::error::Error + Send + Sync>| Error::Invalid(error.to_string());
        let mut host = pipeline::actual_host(ctx).map_err(failed)?;
        if let Some(storage) = self.storage() {
            let caps = storage
                .capabilities(&root.id)
                .await
                .map_err(|e| Error::Invalid(e.to_string()))?;
            host.filesystem = caskade_installer::host::FileSystemCaps {
                writable: caps.writable,
                exec_bits: caps.executable,
                symlinks: caps.symlinks,
                case_sensitive: caps.case_sensitive,
            };
        }
        let job_id = pipeline::input::job_id(input, Some(id.to_owned())).map_err(failed)?;
        let remote = pipeline::input::downloads(input);
        ctx.config.policy.execution = preferences.execution_for(remote);

        // Download (or resolve) under the download limit.
        let prepared = {
            let _permit = if remote {
                self.update(id, |job| {
                    job.status = JobStatus::Downloading;
                    job.phase = Some(JobPhase::Download);
                });
                Some(self.inner.downloads.acquire(cancel).await?)
            } else {
                None
            };
            match pipeline::input::prepare(input.to_owned(), &root.path, &job_id, &host, ctx).await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    if let Some(pipeline::NeedsDecision(question)) =
                        error.downcast_ref::<pipeline::NeedsDecision>()
                    {
                        self.update(id, |job| {
                            job.status = JobStatus::NeedsUser;
                            job.question = Some(question.clone());
                        });
                        return Ok(None);
                    }
                    return Err(self.failed(
                        id,
                        &*error,
                        caskade_installer::failure::Probe {
                            stage: caskade_installer::failure::FailureStage::Inspect,
                            source: (!remote).then(|| std::path::Path::new(input)),
                            remote,
                            root: Some(&root.path),
                        },
                    ));
                }
            }
        };
        if remote {
            self.inner.bus.emit(Event::InstallDownloaded {
                job_id: id.to_owned(),
            });
        }
        let handlers = self.inner.bus.install_handlers();
        if !handlers.is_empty() {
            let input = crate::bus::InstallInput {
                path: prepared.path.clone(),
                artifact: prepared.artifact.clone(),
            };
            ctx.handlers = hooks::propose(handlers, &input).await;
        }
        Ok(Some(Ready {
            prepared,
            host,
            job_id,
            preferences,
        }))
    }

    /// Stores what the install produced and finishes the job.
    async fn finish(
        &self,
        id: &str,
        root: &StorageRoot,
        input: &str,
        (source, download): (&std::path::Path, Option<&url::Url>),
        outcome: InstallOutcome,
        retirement: (Option<source::SourceCleanup>, CancellationToken),
    ) -> Result<()> {
        let store = &self.inner.store;
        match outcome {
            InstallOutcome::Installed(installed) => {
                let (cleanup, cancel) = retirement;
                let manifest = InstallManifest::read(&root.path.join(&installed.dir))
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                let (game, mut entry) = from_manifest(&root.id, &installed.dir, &manifest)?;
                // A copy from another device is a release of the game the
                // user copied, while it's still here (#233).
                let copied = lock(&self.inner.jobs)
                    .get(id)
                    .and_then(|job| job.peer.as_ref().map(|peer| peer.game_id.clone()));
                let existing = match copied {
                    Some(game_id) => store.game(&game_id).await?,
                    None => None,
                };
                let game = match existing {
                    Some(existing) => existing,
                    None => store.upsert_game(&game).await?,
                };
                entry.game_id = game.id.clone();
                let entry = store.upsert_entry(&entry).await?;
                self.track(&entry.id, &manifest, download).await;
                let cleanup_warning = match cleanup {
                    Some(cleanup) => cleanup.finish(cancel).await,
                    None => None,
                };
                self.inner.bus.emit(Event::LibraryGameAdded {
                    game_id: game.id.clone(),
                });
                let after_warning = hooks::notify::<hooks::InstallAfter>(
                    &self.inner.bus,
                    hooks::InstalledGame {
                        job_id: id.to_owned(),
                        game_id: game.id.clone(),
                        title: game.title.clone(),
                        dir: root.path.join(&installed.dir),
                    },
                )
                .await;
                self.update(id, |job| {
                    job.status = JobStatus::Completed;
                    job.title = game.title.clone();
                    job.warnings = installed
                        .warnings
                        .iter()
                        .cloned()
                        .chain(cleanup_warning)
                        .chain(after_warning)
                        .collect();
                    job.game_id = Some(game.id.clone());
                    job.target_dir = Some(root.path.join(&installed.dir));
                    job.dependencies = installed.dependencies.clone();
                });
            }
            InstallOutcome::HandOff(caskade_installer::HandOff::Rom { platform, files }) => {
                self.finish_roms(id, &platform, &files, source).await?;
            }
            InstallOutcome::HandOff(handoff) => {
                let (game, entry) = from_handoff(&handoff, &title_of(input), source);
                let game = store.upsert_game(&game).await?;
                store.upsert_entry(&entry).await?;
                self.inner.bus.emit(Event::LibraryGameAdded {
                    game_id: game.id.clone(),
                });
                self.update(id, |job| {
                    job.status = JobStatus::Completed;
                    job.title = game.title.clone();
                    job.game_id = Some(game.id.clone());
                });
            }
            InstallOutcome::NeedsUser(question) => {
                self.update(id, |job| {
                    job.status = JobStatus::NeedsUser;
                    job.question = Some(question.clone());
                });
            }
            InstallOutcome::Unsupported(reason) => {
                self.update(id, |job| {
                    job.status = JobStatus::Failed;
                    job.error = Some(format!(
                        "unsupported: {}",
                        serde_json::to_string(&reason).unwrap_or_default()
                    ));
                });
            }
        }
        Ok(())
    }

    /// Finishes handed-off ROMs (EMULATION-PLAN §9): into a ROM scanner's
    /// folder if the user picks one, otherwise where they are.
    async fn finish_roms(
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
                format!("Add a ROM scanner for {name} in Settings → Emulators to keep its ROMs together."),
            ));
        }
        let has_default = store
            .get_platform_settings(&platform)
            .await?
            .is_some_and(|settings| settings.default.is_some());
        if !has_default {
            warnings.push(warning(
                "rom_no_emulator",
                format!("No emulator for {name} yet — configure one in Settings → Emulators."),
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

    /// Cancels a queued or running job.
    pub fn cancel(&self, id: &str) -> Result<JobSnapshot> {
        let (cancel, active) = lock(&self.inner.jobs)
            .get(id)
            .map(|job| (job.cancel.clone(), job.active))
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown job {id}", id = id)))?;
        cancel.cancel();
        self.update(id, |job| {
            if !active && !job.status.is_finished() {
                job.status = JobStatus::Cancelled;
                job.question = None;
            }
            // A failed job's scheduled automatic retry is off.
            if let Some(failure) = &mut job.failure {
                failure.retry_at_ms = None;
            }
        })
        .ok_or_else(|| Error::Invalid(crate::tr!("unknown job {id}", id = id)))
    }

    pub fn jobs(&self) -> Vec<JobSnapshot> {
        lock(&self.inner.jobs)
            .values()
            .map(|job| shown(job.snapshot.clone()))
            .collect()
    }

    pub fn job(&self, id: &str) -> Option<JobSnapshot> {
        lock(&self.inner.jobs)
            .get(id)
            .map(|job| shown(job.snapshot.clone()))
    }

    /// Applies `change`, bumps the revision and publishes the snapshot.
    fn update(&self, id: &str, change: impl FnOnce(&mut JobSnapshot)) -> Option<JobSnapshot> {
        let (snapshot, previous) = {
            let mut jobs = lock(&self.inner.jobs);
            let job = jobs.get_mut(id)?;
            // A finished job only changes through an explicit new run.
            let finished = job.snapshot.status.is_finished();
            let before = job.snapshot.clone();
            change(&mut job.snapshot);
            if finished && job.snapshot.status != before.status {
                job.snapshot = before;
                return Some(job.snapshot.clone());
            }
            job.snapshot.revision += 1;
            job.snapshot.updated_at_ms = now_ms();
            (job.snapshot.clone(), before.status)
        };
        self.publish(&snapshot);
        if previous != snapshot.status {
            self.lifecycle(&snapshot);
        }
        Some(snapshot)
    }

    /// The hook events of a job's status change (#55).
    fn lifecycle(&self, job: &JobSnapshot) {
        let job_id = job.id.clone();
        let event = match (job.kind, job.status) {
            (JobKind::Install, JobStatus::Queued) => Event::InstallQueued {
                job_id,
                title: job.title.clone(),
            },
            (JobKind::Install, JobStatus::Completed) => Event::InstallCompleted {
                job_id,
                game_id: job.game_id.clone(),
            },
            (JobKind::Install, JobStatus::Failed) => Event::InstallFailed {
                job_id,
                error: job.error.clone().unwrap_or_default(),
            },
            (JobKind::Uninstall, JobStatus::Completed) => Event::UninstallCompleted {
                job_id,
                game_id: job.game_id.clone(),
            },
            (JobKind::Move, JobStatus::Completed) => Event::GameMoved {
                job_id,
                game_id: job.game_id.clone(),
                dir: job.target_dir.clone(),
            },
            _ => return,
        };
        self.inner.bus.emit(event);
    }

    fn publish(&self, snapshot: &JobSnapshot) {
        self.inner
            .bus
            .emit(Event::InstallJobChanged(Box::new(shown(snapshot.clone()))));
    }
}

/// A job as people see it: its failure's summary in the language in use,
/// so a language switch changes jobs already listed (#214).
fn shown(mut snapshot: JobSnapshot) -> JobSnapshot {
    if let Some(failure) = &mut snapshot.failure {
        failure_text::translate(failure);
    }
    snapshot
}

/// A readable title from a path or URL until discovery names the game.
fn title_of(input: &str) -> String {
    // A magnet link's display name, until the install names the game.
    if let Ok(url) = url::Url::parse(input)
        && url.scheme() == "magnet"
    {
        return url
            .query_pairs()
            .find(|(key, value)| key == "dn" && !value.trim().is_empty())
            .map_or_else(|| "Torrent".to_owned(), |(_, name)| name.into_owned());
    }
    let trimmed = input.trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let name = name.split('?').next().unwrap_or(name);
    if name.is_empty() {
        input.to_owned()
    } else {
        name.to_owned()
    }
}

/// How often a job's progress reaches the bus at most. The fetch engine
/// reports every network chunk (64 KiB) and extraction every file, so an
/// unthrottled sink would publish thousands of job snapshots a second.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Installer progress as job progress, throttled to [`PROGRESS_INTERVAL`].
///
/// The first report, a phase or message change (`started`, `verifying`,
/// `completed`, …) and a report that reaches its total are published straight
/// away. Other reports inside the interval only replace the pending report,
/// which the next publish supersedes and dropping the sink flushes while the
/// job still runs, so the newest numbers are never lost.
struct Progress {
    service: InstallService,
    job_id: String,
    throttle: Mutex<Throttle>,
}

#[derive(Default)]
struct Throttle {
    published_at: Option<Instant>,
    /// The phase and message of the last report, published or not.
    last: Option<(JobPhase, String)>,
    /// The phase whose completed report was published, so a stream of
    /// reports at their total (or a zero total) is still throttled.
    completed: Option<JobPhase>,
    pending: Option<(JobPhase, JobProgress)>,
}

impl Progress {
    fn new(service: InstallService, job_id: String) -> Self {
        Self {
            service,
            job_id,
            throttle: Mutex::default(),
        }
    }

    fn publish(&self, phase: JobPhase, progress: JobProgress) {
        // Cheap when the plan set is unchanged: no snapshot is published.
        self.service.sync_plans(&self.job_id);
        self.service.update(&self.job_id, |job| {
            job.phase = Some(phase);
            job.progress = Some(progress.clone());
        });
        self.service.inner.bus.emit(Event::InstallProgress {
            job_id: self.job_id.clone(),
            phase,
            progress,
        });
    }
}

impl EventSink for Progress {
    fn emit(&self, event: JobEvent) {
        let phase = JobPhase::from(event.phase);
        let progress = JobProgress {
            bytes: event.bytes,
            total_bytes: event.total_bytes,
            files: event.files,
            eta_secs: event.eta_secs,
            message: event.message,
        };
        let now = Instant::now();
        {
            let mut throttle = lock(&self.throttle);
            let changed = throttle
                .last
                .as_ref()
                .is_none_or(|(last, message)| *last != phase || *message != progress.message);
            let complete = progress
                .total_bytes
                .is_some_and(|total| progress.bytes >= total);
            let completes = complete && throttle.completed != Some(phase);
            let due = throttle
                .published_at
                .is_none_or(|at| now.duration_since(at) >= PROGRESS_INTERVAL);
            if changed {
                throttle.last = Some((phase, progress.message.clone()));
            }
            if !(changed || completes || due) {
                throttle.pending = Some((phase, progress));
                return;
            }
            if complete {
                throttle.completed = Some(phase);
            }
            throttle.published_at = Some(now);
            throttle.pending = None;
        }
        self.publish(phase, progress);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let pending = self
            .throttle
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .take();
        // A finished job keeps its last snapshot: its outcome is the news.
        let running = self
            .service
            .job(&self.job_id)
            .is_some_and(|job| !job.status.is_finished());
        if let Some((phase, progress)) = pending.filter(|_| running) {
            self.publish(phase, progress);
        }
    }
}

#[cfg(test)]
mod tests;
