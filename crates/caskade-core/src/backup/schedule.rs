//! Backups from Settings (#193): the `core.backups` section, the status
//! the app shows, Back up now, automatic backups and their retention.
//!
//! Automatic backups are off until the user picks daily or weekly. One is
//! due an interval after the last one that worked; a start (or the watch's
//! next look, every few minutes) after that makes it, so a missed one is
//! made once, never once per missed interval. A failed one is tried again
//! after 30 minutes, doubling, four times; then it waits for the next
//! interval. Nothing ever asks the user: a missing folder or a full disk is
//! shown as the last outcome. After an automatic backup verifies, the
//! oldest backups of the same set beyond the number to keep are deleted;
//! manual backups, other files and other sets' backups never are.

use super::{
    BackupError, Backups, CancellationToken, Component, ExportOptions, Exported, Inspection,
    Journal, Progress, ProgressFn, RestoreOptions, RestoreOutcome, restore,
};
use crate::{error::ErrorReport, settings::SettingsService};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const NAMESPACE: &str = "core.backups";
/// The schedule's state, in the settings table (not a section: the user
/// doesn't edit it).
const STATE: &str = "backup.state";
const HOUR_MS: i64 = 60 * 60 * 1000;
const RETRY_MS: i64 = HOUR_MS / 2;
const RETRIES: u32 = 4;
/// How long after start the watch first looks, so a start isn't slowed.
const SETTLE: Duration = Duration::from_secs(120);
/// How often the watch looks again.
const TICK: Duration = Duration::from_secs(5 * 60);

/// Settings → Backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSettings {
    #[schemars(title = "Backup folder")]
    /// Where backups go; empty for Caskade's own folder.
    pub destination: String,
    #[schemars(title = "Also back up")]
    /// What goes in a backup besides the library.
    pub components: BTreeSet<Component>,
    #[schemars(title = "Back up automatically")]
    pub schedule: Schedule,
    #[schemars(title = "Automatic backups to keep", range(min = 1, max = 100))]
    pub keep: u32,
    #[schemars(title = "Also keep backups on")]
    /// A paired device that keeps a copy of each backup (#243); empty keeps them here only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_on_device: Option<String>,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            destination: String::new(),
            components: BTreeSet::from([
                Component::Artwork,
                Component::Plugins,
                Component::PluginData,
                Component::Themes,
            ]),
            schedule: Schedule::Off,
            keep: 7,
            keep_on_device: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", rename = "BackupSchedule")]
pub enum Schedule {
    Off,
    Daily,
    Weekly,
}

impl Schedule {
    fn interval_ms(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(24 * HOUR_MS),
            Self::Weekly => Some(7 * 24 * HOUR_MS),
        }
    }
}

/// The schedule's memory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    /// The automatic backups' set: retention only ever deletes its own.
    set_id: String,
    last: Option<LastBackup>,
    automatic_ok_ms: Option<i64>,
    automatic_tried_ms: Option<i64>,
    /// Automatic backups that failed in a row.
    failures: u32,
}

/// How the last backup went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LastBackup {
    pub at_ms: i64,
    pub automatic: bool,
    /// The archive, when it worked.
    pub path: Option<PathBuf>,
    pub bytes: u64,
    pub error: Option<ErrorReport>,
}

/// What Settings → Backup shows, kept current by [`Backups::subscribe`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub running: Option<RunningJob>,
    pub last: Option<LastBackup>,
    /// When the next automatic backup is due (Unix ms); `None` when they're
    /// off.
    pub next_automatic_ms: Option<i64>,
    /// Where backups go when no folder is chosen.
    pub default_destination: PathBuf,
    /// A restore waiting for the restart.
    pub staged_restore: Option<Journal>,
    /// How the restore before this start ended.
    pub last_restore: Option<RestoreOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupRunningJob")]
pub struct RunningJob {
    pub kind: BackupJobKind,
    pub progress: Option<Progress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BackupJobKind {
    Backup,
    AutomaticBackup,
    Inspect,
    Restore,
}

/// Registers `core.backups`.
pub fn register_settings(settings: &SettingsService) -> crate::settings::Result<()> {
    settings.register(crate::settings::SectionDefinition::typed::<BackupSettings>(
        NAMESPACE, 1,
    )?)
}

/// When the next automatic backup is due under `schedule`, given `state`,
/// at `now`.
fn due(schedule: Schedule, state: &State, now: i64) -> Option<i64> {
    let interval = schedule.interval_ms()?;
    // A clock set back: the last one seems to be from the future.
    let ok = state.automatic_ok_ms.filter(|ok| *ok <= now + HOUR_MS);
    let tried = state
        .automatic_tried_ms
        .filter(|tried| *tried <= now + HOUR_MS);
    let after_ok = ok.map_or(now, |ok| ok + interval);
    match tried {
        Some(tried) if state.failures > 0 && ok.is_none_or(|ok| tried >= ok) => {
            let wait = if state.failures > RETRIES {
                interval
            } else {
                (RETRY_MS << (state.failures - 1)).min(interval)
            };
            Some(tried + wait)
        }
        _ => Some(after_ok),
    }
}

impl Backups {
    /// Reads `core.backups` from `settings`; registers the section first.
    pub fn set_settings(&self, settings: SettingsService) {
        if let Err(error) = register_settings(&settings) {
            tracing::error!(%error, "backup settings unavailable");
        }
        let _ = self.settings.set(settings);
    }

    async fn settings(&self) -> BackupSettings {
        let Some(settings) = self.settings.get() else {
            return BackupSettings::default();
        };
        settings.read_or_default(NAMESPACE).await
    }

    async fn state(&self) -> State {
        match self.store.get_setting(STATE).await {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_default(),
            _ => State::default(),
        }
    }

    async fn save_state(&self, state: &State) {
        let result = match serde_json::to_value(state) {
            Ok(value) => self.store.set_setting(STATE, &value).await,
            Err(error) => Err(error.into()),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "the backup schedule's state wasn't saved");
        }
    }

    /// The backup folder the settings choose.
    pub async fn destination(&self) -> PathBuf {
        let settings = self.settings().await;
        if settings.destination.trim().is_empty() {
            self.default_destination()
        } else {
            PathBuf::from(settings.destination)
        }
    }

    /// What Settings → Backup shows.
    pub async fn status(&self) -> BackupStatus {
        let (settings, state) = (self.settings().await, self.state().await);
        BackupStatus {
            running: crate::lock(&self.job).as_ref().map(|job| job.1.clone()),
            last: state.last.clone(),
            next_automatic_ms: due(settings.schedule, &state, crate::now_ms()),
            default_destination: self.default_destination(),
            staged_restore: restore::pending(&self.paths).ok().flatten(),
            last_restore: crate::lock(&self.last_restore).clone(),
        }
    }

    /// Every status change, coalesced: progress at most ten times a second.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<Option<BackupStatus>> {
        self.changes.subscribe()
    }

    async fn publish(&self) {
        let status = self.status().await;
        self.changes.send_replace(Some(status));
    }

    /// Keeps how the restore before this start ended, for the status.
    pub fn set_last_restore(&self, outcome: Option<RestoreOutcome>) {
        *crate::lock(&self.last_restore) = outcome;
    }

    /// Cancels the backup, check or restore under way.
    pub fn cancel(&self) {
        if let Some(job) = crate::lock(&self.job).as_ref() {
            job.0.cancel();
        }
    }

    /// Runs `work` as the job shown in the status, with its progress and
    /// cancellation.
    async fn job<T, F, Fut>(&self, kind: BackupJobKind, work: F) -> Result<T, BackupError>
    where
        F: FnOnce(ProgressFn, CancellationToken) -> Fut,
        Fut: std::future::Future<Output = Result<T, BackupError>>,
    {
        let cancel = CancellationToken::new();
        {
            let mut job = crate::lock(&self.job);
            if job.is_some() || self.running.try_lock().is_err() {
                return Err(BackupError::Busy);
            }
            *job = Some((
                cancel.clone(),
                RunningJob {
                    kind,
                    progress: None,
                },
            ));
        }
        self.publish().await;
        let progress: ProgressFn = Arc::new({
            let this = self.clone();
            let last = Arc::new(std::sync::Mutex::new((std::time::Instant::now(), None)));
            move |progress: Progress| {
                let mut last = crate::lock(&last);
                let now = std::time::Instant::now();
                if last.1 == Some(progress.phase)
                    && now.duration_since(last.0) < Duration::from_millis(100)
                {
                    return;
                }
                *last = (now, Some(progress.phase));
                if let Some(job) = crate::lock(&this.job).as_mut() {
                    job.1.progress = Some(progress);
                }
                let this = this.clone();
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move { this.publish().await });
                }
            }
        });
        let result = work(progress, cancel).await;
        *crate::lock(&self.job) = None;
        self.publish().await;
        result
    }

    /// Back up now, with the settings' folder and components. Never deleted
    /// by retention.
    pub async fn back_up_now(&self) -> Result<Exported, BackupError> {
        let settings = self.settings().await;
        let options = ExportOptions {
            destination: self.destination().await,
            components: settings.components,
            set_id: None,
        };
        let result = self
            .job(BackupJobKind::Backup, |progress, cancel| {
                self.export(options, progress, cancel)
            })
            .await;
        if !matches!(result, Err(BackupError::Busy)) {
            let mut state = self.state().await;
            state.last = Some(last(&result, false));
            self.save_state(&state).await;
            self.publish().await;
        }
        result
    }

    /// Makes the automatic backup if it is due at `now`; `None` when it
    /// isn't (or another backup is running: it's looked at again later).
    pub async fn run_due(&self, now: i64) -> Option<Result<Exported, BackupError>> {
        let settings = self.settings().await;
        let mut state = self.state().await;
        if due(settings.schedule, &state, now)? > now {
            return None;
        }
        if state.set_id.is_empty() {
            state.set_id = crate::model::new_id();
            self.save_state(&state).await;
        }
        let destination = self.destination().await;
        let options = ExportOptions {
            destination: destination.clone(),
            components: settings.components.clone(),
            set_id: Some(state.set_id.clone()),
        };
        let result = self
            .job(BackupJobKind::AutomaticBackup, |progress, cancel| {
                self.export(options, progress, cancel)
            })
            .await;
        if matches!(result, Err(BackupError::Busy)) {
            return None;
        }
        state.automatic_tried_ms = Some(now);
        match &result {
            Ok(exported) => {
                state.automatic_ok_ms = Some(now);
                state.failures = 0;
                prune(&destination, &state.set_id, settings.keep, &exported.path).await;
            }
            Err(error) => {
                state.failures = state.failures.saturating_add(1);
                tracing::warn!(%error, failures = state.failures, "the automatic backup failed");
            }
        }
        state.last = Some(last(&result, true));
        self.save_state(&state).await;
        self.publish().await;
        Some(result)
    }

    /// Makes automatic backups when they're due, until the process ends.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.clone().watch_loop())
    }

    async fn watch_loop(self) {
        tokio::time::sleep(SETTLE).await;
        let mut changes = self.settings.get().map(|settings| settings.subscribe());
        loop {
            let _ = self.run_due(crate::now_ms()).await;
            let changed = async {
                match changes.as_mut() {
                    Some(changes) => loop {
                        match changes.recv().await {
                            Ok(changed) if changed.namespace == NAMESPACE => break,
                            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                std::future::pending::<()>().await
                            }
                        }
                    },
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = tokio::time::sleep(TICK) => {}
                () = changed => self.publish().await,
            }
        }
    }

    /// [`Backups::inspect`], shown as a job.
    pub async fn inspect_job(&self, archive: &Path) -> Result<Inspection, BackupError> {
        self.job(BackupJobKind::Inspect, |progress, cancel| async move {
            restore::inspect(&self.paths, archive, &cancel, &progress).await
        })
        .await
    }

    /// [`Backups::stage_restore`], shown as a job.
    pub async fn restore_job(&self, options: RestoreOptions) -> Result<Journal, BackupError> {
        self.job(BackupJobKind::Restore, |progress, cancel| async move {
            restore::stage(&self.paths, &options, &cancel, &progress).await
        })
        .await
    }

    /// Forgets the restore waiting for the restart.
    pub async fn discard_restore_job(&self) -> Result<(), BackupError> {
        let result = self.discard_restore().await;
        self.publish().await;
        result
    }
}

fn last(result: &Result<Exported, BackupError>, automatic: bool) -> LastBackup {
    match result {
        Ok(exported) => LastBackup {
            at_ms: exported.manifest.created_at_ms,
            automatic,
            path: Some(exported.path.clone()),
            bytes: exported.bytes,
            error: None,
        },
        Err(error) => LastBackup {
            at_ms: crate::now_ms(),
            automatic,
            path: None,
            bytes: 0,
            error: Some(ErrorReport::from_error(error)),
        },
    }
}

/// Deletes the oldest backups of `set_id` in `destination` beyond `keep`.
/// Only files named like backups whose manifests say they belong to the
/// set count; `newest`, just verified, always stays.
async fn prune(destination: &Path, set_id: &str, keep: u32, newest: &Path) {
    let (destination, set_id, newest) =
        (destination.to_owned(), set_id.to_owned(), newest.to_owned());
    let result = caskade_jobs::blocking("Removing old backups", move || {
        let mut owned: Vec<(i64, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(&destination)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(super::PREFIX)
                || !name.ends_with(&format!(".{}", super::EXTENSION))
                || !entry.file_type().is_ok_and(|t| t.is_file())
            {
                continue;
            }
            let Ok(file) = std::fs::File::open(entry.path()) else {
                continue;
            };
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                continue;
            };
            if let Ok(manifest) = super::archive::read_manifest(&mut zip)
                && manifest.set_id.as_deref() == Some(set_id.as_str())
            {
                owned.push((manifest.created_at_ms, entry.path()));
            }
        }
        owned.sort_by(|a, b| b.cmp(a));
        let keep = usize::try_from(keep.max(1)).unwrap_or(usize::MAX);
        for (_, path) in owned.into_iter().skip(keep) {
            if path == newest {
                continue;
            }
            tracing::info!(path = %path.display(), "removing an old automatic backup");
            std::fs::remove_file(&path)?;
        }
        std::io::Result::Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "old backups weren't all removed"),
        Err(error) => tracing::warn!(%error, "old backups weren't removed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * HOUR_MS;

    fn state(ok: Option<i64>, tried: Option<i64>, failures: u32) -> State {
        State {
            automatic_ok_ms: ok,
            automatic_tried_ms: tried,
            failures,
            ..State::default()
        }
    }

    #[test]
    fn off_is_never_due_and_a_first_one_is_due_now() {
        assert_eq!(due(Schedule::Off, &State::default(), 1000), None);
        assert_eq!(due(Schedule::Daily, &State::default(), 1000), Some(1000));
    }

    #[test]
    fn one_is_due_an_interval_after_the_last_that_worked() {
        let worked = state(Some(10 * DAY), Some(10 * DAY), 0);
        assert_eq!(due(Schedule::Daily, &worked, 10 * DAY + 5), Some(11 * DAY));
        assert_eq!(due(Schedule::Weekly, &worked, 10 * DAY + 5), Some(17 * DAY));
        // A month away: due once, now, not once per missed day.
        assert_eq!(due(Schedule::Daily, &worked, 40 * DAY), Some(11 * DAY));
    }

    #[test]
    fn failures_retry_with_growing_waits_then_wait_for_the_interval() {
        let at = 10 * DAY;
        let waits: Vec<i64> = (1..=6)
            .map(|failures| {
                due(
                    Schedule::Daily,
                    &state(Some(at - DAY), Some(at), failures),
                    at,
                )
                .unwrap()
                    - at
            })
            .collect();
        assert_eq!(
            waits,
            [RETRY_MS, 2 * RETRY_MS, 4 * RETRY_MS, 8 * RETRY_MS, DAY, DAY]
        );
        // Never sooner than the interval allows after the last success,
        // never later than an interval after the last try.
        let weekly = due(Schedule::Weekly, &state(Some(at), Some(at), 6), at).unwrap();
        assert_eq!(weekly, at + 7 * DAY);
    }

    #[test]
    fn a_clock_set_back_makes_one_due_now() {
        let future = state(Some(100 * DAY), Some(100 * DAY), 0);
        assert_eq!(due(Schedule::Daily, &future, 50 * DAY), Some(50 * DAY));
    }
}
