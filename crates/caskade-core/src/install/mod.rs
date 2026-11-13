//! Installing games into storage roots (#51): job
//! queues over the installer pipeline, and install outcomes as library
//! games.
//!
//! Downloads run up to `downloads` at a time; installs run one at a time per
//! storage root. Every job is a [`JobSnapshot`] published on the bus
//! (`install.job.changed`) on each change. A finished install becomes a
//! `Game` and a `caskade-storage` entry keyed on the install's stable id,
//! launched through its alias.

mod attach;
mod carry;
mod downloads;
mod failure_text;
pub use attach::{AttachTo, PlacedFile};
pub use failure_text::failure_message;
pub mod dropped;
pub mod hooks;
mod job_text;
mod jobs;
mod mapping;
mod move_location;
#[cfg(feature = "net")]
pub mod nearby;
mod operations;
mod pause;
mod peer;
mod progress;
mod recovery;
mod remove;
mod rescan;
mod roms;
mod run;
mod settings;
mod source;
mod update;
pub use operations::{JobAnswer, Verification};
use progress::Progress;
pub use recovery::Recovery;
pub use settings::{InstallerRunnerResolver, ProtonBuilds, proton_builds, runner_paths};

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

pub use peer::{FetchFailure, Fetched, PeerCopyTarget, PeerFetcher, PeerProgress, Placed};

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
    /// Where that copy comes from, when not the app's own copies (a
    /// share, #246).
    peer_fetcher: Option<Arc<dyn peer::PeerFetcher>>,
    /// The library game the install becomes a release of; a retry keeps it.
    attach: Option<AttachTo>,
    /// Stopping to pause, not to cancel (#131).
    pausing: bool,
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
    /// Asked for a download before its source (#236).
    #[cfg(feature = "net")]
    local_source: Mutex<Option<Arc<dyn caskade_installer::fetch::LocalSource>>>,
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
                #[cfg(feature = "net")]
                local_source: Mutex::new(None),
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

    /// Downloads are asked for here before their source: another device
    /// on the network that has them (#236).
    #[cfg(feature = "net")]
    pub fn set_local_source(&self, source: Arc<dyn caskade_installer::fetch::LocalSource>) {
        *lock(&self.inner.local_source) = Some(source);
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
        .await??;
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
        self.queue_install(input, root_id, hints, None).await
    }

    /// [`install_with`](Self::install_with), finishing as another release
    /// of the library game `attach` names instead of a game of its own.
    pub async fn install_attached(
        &self,
        input: String,
        root_id: Option<String>,
        mut hints: DiscoveryHints,
        attach: AttachTo,
    ) -> Result<JobSnapshot> {
        // The program to start, found by name inside the release's folder,
        // so discovery doesn't ask between it and the release's tools.
        if hints.launch_targets.is_empty()
            && let Some(executable) = &attach.executable
        {
            hints
                .launch_targets
                .push(caskade_installer::discover::LaunchHint {
                    path: executable.into(),
                    args: Vec::new(),
                    working_dir: None,
                    reason: crate::tr!("the program the release names"),
                });
        }
        self.queue_install(input, root_id, hints, Some(attach))
            .await
    }

    /// Lists a new install job for `input` into `root_id` (or the default
    /// root) and starts it.
    async fn queue_install(
        &self,
        input: String,
        root_id: Option<String>,
        hints: DiscoveryHints,
        attach: Option<AttachTo>,
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
        let cancel = self.add_job(snapshot.clone(), None, hints, attach);
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
        attach: Option<AttachTo>,
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
                peer_fetcher: None,
                attach,
                pausing: false,
            },
        );
        self.publish(&snapshot);
        cancel
    }

    /// Cancels a queued, running or paused job.
    pub fn cancel(&self, id: &str) -> Result<JobSnapshot> {
        let (cancel, active, paused) = lock(&self.inner.jobs)
            .get_mut(id)
            .map(|job| {
                // A cancel while pausing wins.
                job.pausing = false;
                let paused = job.snapshot.status == JobStatus::Paused;
                (job.cancel.clone(), job.active, paused)
            })
            .ok_or_else(|| Error::Invalid(crate::tr!("This job is no longer in Activity.")))?;
        cancel.cancel();
        if paused {
            self.forget_paused(id);
        }
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
        .ok_or_else(|| Error::Invalid(crate::tr!("This job is no longer in Activity.")))
    }

    /// Takes a finished job out of Activity, with its scheduled automatic
    /// retry and its failure count. What it downloaded or installed stays.
    pub fn remove(&self, id: &str) -> Result<()> {
        let mut jobs = lock(&self.inner.jobs);
        let job = jobs
            .get(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("This job is no longer in Activity.")))?;
        if !job.snapshot.status.is_finished() {
            return Err(Error::Invalid(crate::tr!(
                "Cancel this job before removing it from Activity."
            )));
        }
        job.cancel.cancel();
        jobs.remove(id);
        drop(jobs);
        self.forget_attempts(id);
        lock(&self.inner.pending_roms).remove(id);
        Ok(())
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

    /// Applies `change`, bumps the revision and publishes the snapshot, when
    /// it changed anything.
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
            // Nothing changed (a task ending on its question): no new
            // revision, so an answer to the snapshot shown still holds (#117).
            if job.snapshot == before {
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

/// A job as people see it: its failure's summary and the installer's text
/// in the language in use, so a language switch changes jobs already listed
/// (#214).
fn shown(mut snapshot: JobSnapshot) -> JobSnapshot {
    if let Some(failure) = &mut snapshot.failure {
        failure_text::translate(failure);
    }
    job_text::translate(&mut snapshot);
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

#[cfg(test)]
mod tests;
