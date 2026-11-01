use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use cap_std::fs::Dir;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

use crate::{
    Candidate, Program, RuntimeSpec, Step, Strategy, disk::DiskSpace, events::EventSink,
    runtime::Runtime, safety,
};

use super::{
    AttemptFailure, ExecConfig, ExecError, ExecReport, ExecRequest, Extractor, FinalizationContext,
    Finalizer, JournalHandle, ProcessObserver, Result,
    api::{NoExtractor, NoObserver, Runtimes, one_component},
    context, journal,
    steps::Services,
};
use crate::runtime::alias::{AliasStore, supports_alias};

#[derive(Clone)]
pub struct Executor {
    config: ExecConfig,
    runtimes: Runtimes,
    disk: Arc<dyn DiskSpace>,
    sink: Arc<dyn EventSink>,
    extractor: Arc<dyn Extractor>,
    observer: Arc<dyn ProcessObserver>,
    finalizer: Option<Arc<dyn Finalizer>>,
    aliases: Option<Aliases>,
}

/// Per-job alias configuration (#31). Callers omit it for games whose recipe
/// disables the alias.
#[derive(Clone)]
struct Aliases {
    store: AliasStore,
    root_id: String,
}

impl Executor {
    pub fn new(
        runtime: Arc<dyn Runtime>,
        disk: Arc<dyn DiskSpace>,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            config: ExecConfig::default(),
            runtimes: vec![(RuntimeSpec::Native, runtime)],
            disk,
            sink,
            extractor: Arc::new(NoExtractor),
            observer: Arc::new(NoObserver),
            finalizer: None,
            aliases: None,
        }
    }
    /// Give each committed game a stable alias `<store>/<id>`, where the ID
    /// derives from the storage root ID and game directory (not the absolute
    /// path, so it survives relocation). Native installers receive the alias
    /// as `INSTALL_DIR`; other installs are linked after commit.
    pub fn with_aliases(mut self, store: AliasStore, root_id: impl Into<String>) -> Self {
        self.aliases = Some(Aliases {
            store,
            root_id: root_id.into(),
        });
        self
    }
    pub fn with_config(mut self, config: ExecConfig) -> Self {
        self.config = config;
        self
    }
    pub fn with_extractor(mut self, extractor: Arc<dyn Extractor>) -> Self {
        self.extractor = extractor;
        self
    }
    pub fn with_observer(mut self, observer: Arc<dyn ProcessObserver>) -> Self {
        self.observer = observer;
        self
    }
    pub fn with_finalizer(mut self, finalizer: Arc<dyn Finalizer>) -> Self {
        self.finalizer = Some(finalizer);
        self
    }
    pub fn register_runtime(&mut self, spec: RuntimeSpec, runtime: Arc<dyn Runtime>) {
        self.runtimes.retain(|(existing, _)| *existing != spec);
        self.runtimes.push((spec, runtime));
    }

    /// Dropping this future cancels the blocking worker. Its root lock remains
    /// held until the worker has stopped and rolled back; cleanup can safely wait
    /// on that same lock without racing a detached writer.
    pub async fn execute(
        &self,
        mut request: ExecRequest,
        cancellation: CancellationToken,
    ) -> Result<ExecReport> {
        validate(&request)?;
        request.root = std::fs::canonicalize(&request.root)?;
        let token = cancellation.child_token();
        let _cancel_on_drop = token.clone().drop_guard();
        let lock = root_lock(&request.root);
        let guard = tokio::select! {
            guard = lock.lock_owned() => guard,
            _ = token.cancelled() => return Err(ExecError::Cancelled),
        };
        if token.is_cancelled() {
            return Err(ExecError::Cancelled);
        }
        let executor = self.clone();
        caskade_jobs::blocking("Installing game", move || {
            executor.run(request, token, guard)
        })
        .await
        .map_err(|error| ExecError::Worker {
            message: error.to_string(),
        })?
    }

    /// Startup recovery is serialized against all jobs for the same canonical
    /// root, including workers whose async caller has already been dropped.
    pub async fn cleanup(&self, root: &Path) -> Result<usize> {
        let root = std::fs::canonicalize(root)?;
        let guard = root_lock(&root).lock_owned().await;
        let runtimes = self.runtimes.clone();
        caskade_jobs::blocking("Recovering installs", move || {
            let _guard = guard;
            journal::recover(&safety::open_staging(&root)?, &runtimes)
        })
        .await
        .map_err(|error| ExecError::Worker {
            message: error.to_string(),
        })?
    }

    /// Run recorded uninstall/maintenance steps against an existing game folder.
    /// This never owns, stages, renames or deletes that folder. The caller owns
    /// final deletion and reporting. Delegate uninstall steps need no inverse.
    pub async fn execute_steps_in_place(
        &self,
        root: &Path,
        relative_dir: &Path,
        job_id: &str,
        steps: Vec<Step>,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.execute_steps_in_place_then(
            root,
            relative_dir,
            job_id,
            steps,
            cancellation,
            |_, result| result,
        )
        .await
    }

    /// Keep root exclusion through the uninstall owner's deletion and reporting
    /// callback. The callback receives the step result, including handled failure,
    /// and a capability for this root; it never runs after a detached writer.
    pub async fn execute_steps_in_place_then<T, F>(
        &self,
        root: &Path,
        relative_dir: &Path,
        job_id: &str,
        steps: Vec<Step>,
        cancellation: CancellationToken,
        finalize: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Dir, Result<()>) -> Result<T> + Send + 'static,
    {
        one_component(relative_dir)?;
        one_component(Path::new(job_id))?;
        let root_path = std::fs::canonicalize(root)?;
        let token = cancellation.child_token();
        let _cancel_on_drop = token.clone().drop_guard();
        let guard = tokio::select! {
            _ = token.cancelled() => return Err(ExecError::Cancelled),
            guard = root_lock(&root_path).lock_owned() => guard,
        };
        let relative_dir = relative_dir.to_owned();
        let job_id = job_id.to_owned();
        let executor = self.clone();
        caskade_jobs::blocking("Updating game files", move || {
            let _guard = guard;
            let root = Arc::new(safety::open_staging(&root_path)?);
            let dir = match context::subdir(&root, &relative_dir, false) {
                Ok(dir) => dir,
                Err(ExecError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    return finalize(&root, Err(ExecError::Io(error)));
                }
                Err(error) => return Err(error),
            };
            let request = ExecRequest {
                root: root_path,
                job_id,
                relative_dir,
                plans: crate::PlanSet {
                    candidates: vec![],
                    selected: None,
                    question: None,
                },
                sources: Default::default(),
            };
            let mut config = executor.config.clone();
            config.disk_margin_bytes = 0;
            let mut context = context::new_context(
                dir,
                &request,
                &config,
                token,
                executor.sink.clone(),
                executor.disk.clone(),
                None,
            );
            context.working_dir = request.root.join(&request.relative_dir);
            context.emit("started")?;
            let journal = journal::prepare(&root, &request.job_id, request.relative_dir.clone())?;
            let mut attempt = Attempt {
                root: root.clone(),
                journal,
                runtimes: executor.runtimes.clone(),
                armed: true,
            };
            let game_path = request.root.join(&request.relative_dir);
            let services = Services {
                request: &request,
                game_path: &game_path,
                install_dir: &game_path,
                runtimes: &executor.runtimes,
                config: &config,
                extractor: &*executor.extractor,
                observer: &*executor.observer,
                journal: &attempt.journal,
                compensate_delegates: false,
            };
            let result = (|| {
                for (index, step) in steps.iter().enumerate() {
                    context.step =
                        Some(u32::try_from(index).map_err(|_| ExecError::InvalidRequest {
                            message: "too many in-place steps".into(),
                        })?);
                    context.emit("step_started")?;
                    services.execute(step, &context)?;
                    context.emit("step_completed")?;
                }
                context.step = None;
                context.emit("completed")
            })();
            let execution = match result {
                Ok(()) => attempt.finish(),
                Err(error) => match attempt.rollback() {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(rollback.after(&error)),
                },
            };
            finalize(&root, execution)
        })
        .await
        .map_err(|error| ExecError::Worker {
            message: error.to_string(),
        })?
    }

    fn run(
        &self,
        request: ExecRequest,
        token: CancellationToken,
        _guard: OwnedMutexGuard<()>,
    ) -> Result<ExecReport> {
        let span = tracing::info_span!("installer.execute", job_id = %request.job_id);
        let _entered = span.enter();
        let root = Arc::new(safety::open_staging(&request.root)?);
        let result = self.run_inner(&request, &token, &root);
        journal::prune(&root);
        result
    }

    fn run_inner(
        &self,
        request: &ExecRequest,
        token: &CancellationToken,
        root: &Arc<Dir>,
    ) -> Result<ExecReport> {
        let selected = request.plans.selected.ok_or(ExecError::DecisionRequired)? as usize;
        let mut initial = context::new_context(
            root.try_clone()?,
            request,
            &self.config,
            token.clone(),
            self.sink.clone(),
            self.disk.clone(),
            request.plans.candidates[selected].requires.est_disk_bytes,
        );
        initial.emit("started")?;
        initial.checkpoint(
            request.plans.candidates[selected]
                .requires
                .est_disk_bytes
                .unwrap_or(0),
        )?;
        let relative_dir = collision_name(root, &request.relative_dir, self.config.case_sensitive)?;
        let final_path = request.root.join(&relative_dir);
        let order: Vec<usize> = std::iter::once(selected)
            .chain((0..request.plans.candidates.len()).filter(|index| *index != selected))
            .collect();
        // Running installer code needs the Allow policy or a chosen strategy
        // that runs it; a failed static strategy never falls back to one
        // that does without asking (CSK-01).
        let consented = self.config.execution == crate::plan::ExecutionPolicy::Allow
            || crate::plan::executes(&request.plans.candidates[selected]);
        let mut failures = vec![];
        // The inputs there now; one that vanishes stops the fallback.
        let present: Vec<&Path> = request
            .sources
            .values()
            .map(PathBuf::as_path)
            .filter(|source| std::fs::symlink_metadata(source).is_ok())
            .collect();
        for (position, &index) in order.iter().enumerate() {
            initial.check_cancelled()?;
            let candidate = &request.plans.candidates[index];
            if position > 0 && !consented && crate::plan::executes(candidate) {
                return Err(ExecError::NeedsUser {
                    question: crate::Question::ConfirmExecution {
                        candidates: order[position..]
                            .iter()
                            .map(|index| request.plans.candidates[*index].clone())
                            .collect(),
                    },
                });
            }
            let span = tracing::info_span!("installer.candidate", handler = %candidate.handler.0, candidate = index);
            let _entered = span.enter();
            let journal = journal::prepare(root, &request.job_id, relative_dir.clone())?;
            let mut attempt = Attempt {
                root: root.clone(),
                journal,
                runtimes: self.runtimes.clone(),
                armed: true,
            };
            let alias_id = self
                .aliases
                .as_ref()
                .filter(|_| supports_alias(&final_path))
                .map(|aliases| {
                    AliasStore::short_id(&format!(
                        "{}/{}",
                        aliases.root_id,
                        relative_dir.to_string_lossy()
                    ))
                });
            let mut alias_path = None;
            let result = (|| {
                let staging = context::subdir(root, Path::new(".caskade/staging"), true)?;
                staging.create_dir(&request.job_id)?;
                let native = native_installer(candidate);
                let game_path = if native {
                    final_path.clone()
                } else {
                    request.root.join(".caskade/staging").join(&request.job_id)
                };
                if native {
                    attempt.journal.set_final_owned()?;
                    if let Err(error) = root.create_dir(&relative_dir) {
                        attempt.journal.release_final()?;
                        return Err(error.into());
                    }
                }
                let dir = if native {
                    root.open_dir(&relative_dir)?
                } else {
                    staging.open_dir(&request.job_id)?
                };
                let mut context = context::new_context(
                    dir,
                    request,
                    &self.config,
                    token.clone(),
                    self.sink.clone(),
                    self.disk.clone(),
                    candidate.requires.est_disk_bytes,
                );
                context.working_dir = game_path.clone();
                context.final_dir = final_path.clone();
                context.handler = Some(candidate.handler.clone());
                context.emit("candidate_started")?;
                context.checkpoint(candidate.requires.est_disk_bytes.unwrap_or(0))?;
                if native && let (Some(aliases), Some(id)) = (&self.aliases, &alias_id) {
                    alias_path = link_alias(&aliases.store, id, &final_path);
                }
                let install_dir = alias_path.clone().unwrap_or_else(|| game_path.clone());
                let services = Services {
                    request,
                    game_path: &game_path,
                    install_dir: &install_dir,
                    runtimes: &self.runtimes,
                    config: &self.config,
                    extractor: &*self.extractor,
                    observer: &*self.observer,
                    journal: &attempt.journal,
                    compensate_delegates: true,
                };
                for (step_index, step) in candidate.steps.iter().enumerate() {
                    context.step =
                        Some(
                            u32::try_from(step_index).map_err(|_| ExecError::InvalidRequest {
                                message: "too many steps".into(),
                            })?,
                        );
                    context.emit("step_started")?;
                    services.execute(step, &context)?;
                    context.emit("step_completed")?;
                }
                context.step = None;
                if let Some(finalizer) = &self.finalizer {
                    context.emit("finalize_started")?;
                    let journal = attempt.journal.entries();
                    finalizer.finalize(&FinalizationContext {
                        game_dir: &game_path,
                        final_dir: &final_path,
                        relative_dir: &relative_dir,
                        candidate,
                        steps: &context,
                        alias: alias_id.as_deref(),
                        journal: &journal,
                    })?;
                    context.emit("finalize_completed")?;
                }
                context.emit("commit_started")?;
                context.checkpoint(0)?;
                if !native {
                    if root.try_exists(&relative_dir)? {
                        return Err(safety::SafetyError::Collision(relative_dir.clone()).into());
                    }
                    attempt.journal.set_final_owned()?;
                    if let Err(error) = staging.rename(&request.job_id, root, &relative_dir) {
                        attempt.journal.release_final()?;
                        return Err(error.into());
                    }
                }
                if !native && let (Some(aliases), Some(id)) = (&self.aliases, &alias_id) {
                    alias_path = link_alias(&aliases.store, id, &final_path);
                }
                context.emit("committed")?;
                // This is the last cancellable event. The completed journal
                // marker is the success boundary and is written afterwards.
                context.emit("completed")?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    let entries = attempt.journal.entries();
                    attempt.finish()?;
                    return Ok(ExecReport {
                        dir: final_path,
                        relative_dir,
                        candidate_index: index as u32,
                        candidate: candidate.clone(),
                        failures,
                        journal: entries,
                        alias: alias_path.and(alias_id),
                    });
                }
                Err(error) => {
                    if let (Some(aliases), Some(id), Some(_)) =
                        (&self.aliases, &alias_id, &alias_path)
                    {
                        let _ = aliases.store.remove(id);
                    }
                    attempt
                        .rollback()
                        .map_err(|rollback| rollback.after(&error))?;
                    if token.is_cancelled() || matches!(error, ExecError::Cancelled) {
                        return Err(ExecError::Cancelled);
                    }
                    if matches!(
                        error,
                        ExecError::DecisionRequired | ExecError::NeedsUser { .. }
                    ) {
                        return Err(error);
                    }
                    // A vanished source, refused access or a full disk would
                    // fail the next strategy too, or run installer code the
                    // user didn't choose: stop here (#158).
                    if crate::failure::stops_fallback(&error, &present) {
                        return Err(error);
                    }
                    failures.push(AttemptFailure {
                        candidate: index as u32,
                        message: error.to_string(),
                    });
                    initial.total = candidate.requires.est_disk_bytes;
                    initial.emit("candidate_failed")?;
                }
            }
        }
        Err(ExecError::AllCandidatesFailed { failures })
    }
}

/// An alias is an optimisation for later moves; failing to create one never
/// fails the install, which then simply records no alias.
fn link_alias(store: &AliasStore, id: &str, target: &Path) -> Option<PathBuf> {
    store
        .link(id, target)
        .inspect_err(|error| tracing::warn!(%error, "game alias not created"))
        .ok()
}

fn validate(request: &ExecRequest) -> Result<()> {
    one_component(Path::new(&request.job_id))?;
    one_component(&request.relative_dir)?;
    if request.plans.question.is_some() {
        return Err(ExecError::DecisionRequired);
    }
    let selected = request.plans.selected.ok_or(ExecError::DecisionRequired)? as usize;
    if selected >= request.plans.candidates.len() {
        return Err(ExecError::InvalidRequest {
            message: "selected candidate index is out of range".into(),
        });
    }
    Ok(())
}

fn native_installer(candidate: &Candidate) -> bool {
    candidate.steps.iter().any(|step| match step {
        Step::Run {
            program,
            runtime: RuntimeSpec::Native,
            ..
        } => {
            matches!(program, Program::Source(_))
                || matches!(
                    candidate.strategy,
                    Strategy::RunSilent | Strategy::RunInteractive
                )
        }
        Step::RunInteractive {
            runtime: RuntimeSpec::Native,
            ..
        } => true,
        _ => false,
    })
}

fn collision_name(root: &Dir, base: &Path, case_sensitive: bool) -> Result<PathBuf> {
    let fold = |name: &str| {
        if case_sensitive {
            name.to_owned()
        } else {
            name.to_lowercase()
        }
    };
    let names = root
        .entries()?
        .map(|entry| entry.map(|entry| fold(&entry.file_name().to_string_lossy())))
        .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
    let exists = |path: &Path| names.contains(&fold(&path.to_string_lossy()));
    if !exists(base) {
        return Ok(base.into());
    }
    for suffix in 2..=100_000 {
        let name = PathBuf::from(format!("{} ({suffix})", base.to_string_lossy()));
        if !exists(&name) {
            return Ok(name);
        }
    }
    Err(ExecError::InvalidRequest {
        message: "folder collision suffix limit reached".into(),
    })
}

struct Attempt {
    root: Arc<Dir>,
    journal: JournalHandle,
    runtimes: Runtimes,
    armed: bool,
}
impl Attempt {
    fn rollback(&mut self) -> Result<()> {
        self.armed = false;
        journal::rollback(&self.root, &self.journal.snapshot(), &self.runtimes)?;
        self.journal.remove()?;
        journal::prune(&self.root);
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        self.journal.complete()?;
        self.armed = false;
        let stage = Path::new(".caskade/staging").join(&self.journal.snapshot().job_id);
        journal::remove(&self.root, &stage)?;
        self.journal.remove()?;
        journal::prune(&self.root);
        Ok(())
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.rollback();
        }
    }
}

type RootLocks = Mutex<BTreeMap<PathBuf, Weak<AsyncMutex<()>>>>;
pub(super) fn root_lock(root: &Path) -> Arc<AsyncMutex<()>> {
    static LOCKS: OnceLock<RootLocks> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .expect("root lock registry poisoned");
    locks.retain(|_, lock| lock.strong_count() != 0);
    if let Some(lock) = locks.get(root).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(root.into(), Arc::downgrade(&lock));
    lock
}
