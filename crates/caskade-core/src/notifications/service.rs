//! The notifications service: catches up on game additions, follows the
//! bus for failures and updates, and serves the feed.

use super::{
    ENTRY_GRACE_MS, KEEP_READ_MS, MemberKind, MemberPage, MemberRow, Notice, NotificationCategory,
    NotificationPage, NotificationTarget, Severity,
};
use crate::{
    Result,
    bus::{Bus, Event},
    db::LibraryStore,
    emulation::setup::{update_available, version_label},
    i18n::Text,
    install::{JobKind, JobSnapshot, JobStatus},
    msg, msgn,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, watch};

/// Unix milliseconds now; tests turn it.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// How long after a library event the journal is read: one pass covers a
/// burst of additions.
const SETTLE: Duration = Duration::from_secs(2);
/// How often the journal and the updates are read without an event, for
/// additions that came without one and games that waited for entries.
const TICK: Duration = Duration::from_secs(60);
/// Additions read per transaction.
const CHUNK: u32 = 500;
const UPDATES_KEY: &str = "updates.available";
const APP_UPDATE_KEY: &str = "app.update";

/// The notification feed. Cheap to clone.
#[derive(Clone)]
pub struct Notifications {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    clock: Clock,
    /// Counts changes, for the UI's badge.
    changed: watch::Sender<u64>,
    /// One writer at a time: passes over the journal and group syncs.
    writing: Mutex<()>,
}

impl Notifications {
    pub fn new(store: LibraryStore) -> Self {
        Self::with_clock(store, Arc::new(crate::now_ms))
    }

    pub fn with_clock(store: LibraryStore, clock: Clock) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                clock,
                changed: watch::channel(0).0,
                writing: Mutex::new(()),
            }),
        }
    }

    fn now(&self) -> i64 {
        (self.inner.clock)()
    }

    /// Ticks after every change of the feed or its read state.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.changed.subscribe()
    }

    fn changed(&self, changed: bool) -> bool {
        if changed {
            self.inner.changed.send_modify(|count| *count += 1);
        }
        changed
    }

    /// Folds every unseen game addition into its batch. Whether the feed
    /// changed.
    pub async fn ingest_additions(&self) -> Result<bool> {
        let _writing = self.inner.writing.lock().await;
        let mut changed = false;
        loop {
            let done = self
                .inner
                .store
                .ingest_game_additions(self.now(), ENTRY_GRACE_MS, CHUNK)
                .await?;
            changed |= done.added > 0;
            // A full chunk of games still waiting for entries ends the pass:
            // the next one picks them up.
            if done.added + done.skipped == 0
                || done.added + done.skipped + done.waiting < CHUNK as usize
            {
                break;
            }
        }
        Ok(self.changed(changed))
    }

    /// Makes the "Updates available" item list exactly the installed games
    /// and managed emulators with a newer release on offer. Repeated checks
    /// that find the same release change nothing.
    pub async fn sync_updates(&self) -> Result<bool> {
        let store = &self.inner.store;
        let mut members = Vec::new();
        for row in store.followed_updates().await? {
            if !row.available() {
                continue;
            }
            let latest = row.latest.clone().unwrap_or_default();
            let title = store
                .game(&row.game_id)
                .await?
                .map_or_else(|| row.project.clone(), |game| game.title);
            members.push(MemberRow {
                key: format!("game:{}@{latest}", row.entry_id),
                kind: MemberKind::GameUpdate,
                game_id: Some(row.game_id.clone()),
                entry_id: Some(row.entry_id.clone()),
                label: format!("{title} {latest}"),
            });
        }
        for managed in store.managed_emulators().await? {
            if !update_available(&managed) {
                continue;
            }
            let latest = managed.latest.clone().unwrap_or_default();
            members.push(MemberRow {
                key: format!("emulator:{}@{latest}", managed.emulator_id),
                kind: MemberKind::EmulatorUpdate,
                game_id: None,
                entry_id: None,
                label: format!("{} {}", managed.definition_id, version_label(&latest)),
            });
        }
        for managed in store.managed_tools().await? {
            if !managed.update_available() {
                continue;
            }
            let latest = managed.latest.clone().unwrap_or_default();
            members.push(MemberRow {
                key: format!("tool:{}@{latest}", managed.tool_id),
                kind: MemberKind::ToolUpdate,
                game_id: None,
                entry_id: None,
                label: format!(
                    "{} {}",
                    crate::managed::tool_label(&managed.tool_id),
                    version_label(&latest)
                ),
            });
        }
        let count = members.len();
        let notice = Notice {
            category: NotificationCategory::UpdatesAvailable,
            severity: Severity::Info,
            key: UPDATES_KEY.into(),
            title: msgn!("{n} update available", "{n} updates available", count),
            // Games' titles and versions: shown as they are.
            body: Text::Plain(
                members
                    .iter()
                    .take(3)
                    .map(|m| m.label.as_str())
                    .chain((count > 3).then_some("…"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            target: NotificationTarget::Members,
        };
        let _writing = self.inner.writing.lock().await;
        let changed = store
            .sync_notice_members(&notice, &members, self.now())
            .await?;
        Ok(self.changed(changed))
    }

    /// Caskade's own update: `Some(version)` while one is on offer, `None`
    /// once there is none (up to date, or installed).
    pub async fn set_app_update(&self, version: Option<&str>) -> Result<bool> {
        let now = self.now();
        let changed = match version {
            Some(version) => {
                self.inner
                    .store
                    .upsert_notice(
                        &Notice {
                            category: NotificationCategory::UpdatesAvailable,
                            severity: Severity::Info,
                            key: APP_UPDATE_KEY.into(),
                            title: msg!("Caskade {version} is available", version = version),
                            body: msg!("Download and install it from Settings → General.").into(),
                            target: NotificationTarget::Settings {
                                tab: "general".into(),
                                section: None,
                            },
                        },
                        now,
                    )
                    .await?
            }
            None => self.inner.store.resolve_notice(APP_UPDATE_KEY, now).await?,
        };
        Ok(self.changed(changed))
    }

    /// Records what a bus event means for the feed. Whether the journal or
    /// the updates should be read again soon.
    pub async fn on_event(&self, event: &Event) -> bool {
        let result = match event {
            Event::InstallJobChanged(job) => self.on_job(job).await,
            Event::PluginDisabled { plugin, reason } => {
                self.notice(Notice {
                    category: NotificationCategory::PluginDisabled,
                    severity: Severity::Warning,
                    key: format!("plugin.disabled:{plugin}"),
                    title: msg!("Plugin {plugin} was turned off", plugin = plugin),
                    body: Text::Plain(reason.clone()),
                    target: NotificationTarget::Settings {
                        tab: "plugins".into(),
                        section: None,
                    },
                })
                .await
            }
            Event::LifecycleScriptsRan(report) => self.on_scripts(report).await,
            Event::AchievementsUnlocked(unlocks) => self.on_unlocks(unlocks).await,
            Event::LibraryGameAdded { .. }
            | Event::LibraryGameUpdated { .. }
            | Event::LibraryScanCompleted { .. }
            | Event::EmulationScanCompleted { .. }
            | Event::InstallCompleted { .. } => return true,
            Event::EmulationChanged { scope } => return scope == "emulators",
            Event::ToolsChanged => return true,
            _ => return false,
        };
        if let Err(error) = result {
            tracing::warn!(event = event.name(), %error, "notification not recorded");
        }
        false
    }

    /// Failed after-start and after-exit scripts: one item per session and
    /// phase, naming each failure. A failed before-launch script stops the
    /// launch, which says why itself.
    async fn on_scripts(&self, report: &crate::scripts::ScriptReport) -> Result<bool> {
        use crate::scripts::ScriptPhase;
        let phase = match report.phase {
            Some(phase @ (ScriptPhase::AfterStart | ScriptPhase::AfterExit)) => phase,
            _ => return Ok(false),
        };
        let failed: Vec<String> = report
            .runs
            .iter()
            .filter(|run| !run.succeeded())
            .map(|run| run.describe())
            .collect();
        if failed.is_empty() {
            return Ok(false);
        }
        let game = &report.game_title;
        self.notice(Notice {
            category: NotificationCategory::ScriptFailed,
            severity: Severity::Warning,
            key: format!(
                "script.failed:{}:{}",
                report.session_id.as_deref().unwrap_or(&report.game_id),
                phase.as_str()
            ),
            title: match phase {
                ScriptPhase::AfterStart => {
                    msg!("A script after {game} started failed", game = game)
                }
                _ => msg!("A cleanup script after {game} failed", game = game),
            },
            body: Text::Plain(failed.join("\n")),
            target: NotificationTarget::Game {
                game_id: report.game_id.clone(),
            },
        })
        .await
    }

    /// Newly seen unlocks: one item per set and batch of unlocks. The core
    /// sends them only after a set's baseline, and only once each, so
    /// neither a first import nor a repeated refresh floods the feed.
    async fn on_unlocks(&self, unlocks: &crate::achievements::NewUnlocks) -> Result<bool> {
        if unlocks.names.is_empty() {
            return Ok(false);
        }
        let game = &unlocks.game_title;
        let count = unlocks.names.len();
        self.notice(Notice {
            category: NotificationCategory::AchievementsUnlocked,
            severity: Severity::Info,
            key: format!(
                "achievements.unlocked:{}:{}",
                unlocks.set_id,
                unlocks.names.join("\u{1f}")
            ),
            title: msgn!(
                "Achievement unlocked in {game}",
                "{n} achievements unlocked in {game}",
                count,
                game = game
            ),
            body: msg!(
                "{names} ({provider}, {account})",
                names = unlocks.names.join(", "),
                provider = unlocks.provider_name,
                account = unlocks.account_name
            )
            .into(),
            target: NotificationTarget::Game {
                game_id: unlocks.game_id.clone(),
            },
        })
        .await
    }

    async fn notice(&self, notice: Notice) -> Result<bool> {
        let changed = self.inner.store.upsert_notice(&notice, self.now()).await?;
        Ok(self.changed(changed))
    }

    /// A failed job is one item per job, updated when a retry fails another
    /// way and resolved when the job finishes or is cancelled. Progress and
    /// waiting for an answer stay in the job list.
    async fn on_job(&self, job: &JobSnapshot) -> Result<bool> {
        let key = format!("job.failed:{}", job.id);
        match job.status {
            JobStatus::Failed => {
                let game = &job.title;
                let title = match job.kind {
                    JobKind::Install | JobKind::Fetch => {
                        msg!("{game} couldn't install", game = game)
                    }
                    JobKind::Update => msg!("{game} couldn't update", game = game),
                    JobKind::Uninstall => msg!("{game} couldn't uninstall", game = game),
                    JobKind::Move => msg!("{game} couldn't move", game = game),
                    JobKind::Verify => msg!("{game} couldn't check", game = game),
                    JobKind::PeerCopy => msg!("{game} couldn't be copied", game = game),
                };
                // The classified failure says why in the language shown;
                // other jobs' errors stay as they came.
                let body = match &job.failure {
                    Some(failure) => crate::install::failure_message(failure).into(),
                    None => Text::Plain(job.error.clone().unwrap_or_default()),
                };
                let target = match (&job.game_id, job.kind) {
                    (Some(game_id), JobKind::Update | JobKind::Verify | JobKind::Move) => {
                        NotificationTarget::Game {
                            game_id: game_id.clone(),
                        }
                    }
                    _ => NotificationTarget::Jobs,
                };
                self.notice(Notice {
                    category: NotificationCategory::JobFailed,
                    severity: Severity::Error,
                    key,
                    title,
                    body,
                    target,
                })
                .await
            }
            JobStatus::Completed | JobStatus::Cancelled => {
                let changed = self.inner.store.resolve_notice(&key, self.now()).await?;
                Ok(self.changed(changed))
            }
            _ => Ok(false),
        }
    }

    /// Catches up: the journal, the updates on offer, and old read items.
    pub async fn refresh(&self) {
        if let Err(error) = self.ingest_additions().await {
            tracing::warn!(%error, "game additions not read; the next pass retries");
        }
        if let Err(error) = self.sync_updates().await {
            tracing::warn!(%error, "updates not read");
        }
        if let Err(error) = self.prune().await {
            tracing::warn!(%error, "old notifications not cleaned up");
        }
    }

    /// Deletes read items untouched for [`KEEP_READ_MS`].
    pub async fn prune(&self) -> Result<u64> {
        let pruned = self
            .inner
            .store
            .prune_notifications(self.now().saturating_sub(KEEP_READ_MS))
            .await?;
        self.changed(pruned > 0);
        Ok(pruned)
    }

    /// Follows the bus until it closes: a refresh at once, after library
    /// events (once a burst settles) and every minute; failures and plugin
    /// notices as they come.
    pub fn watch(&self, bus: &Bus) -> tokio::task::JoinHandle<()> {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let this = self.clone();
        let mut events = bus.stream();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                let due = tokio::select! {
                    _ = tick.tick() => true,
                    received = events.recv() => match received {
                        Ok(event) => this.on_event(&event).await,
                        Err(RecvError::Lagged(_)) => true,
                        Err(RecvError::Closed) => return,
                    },
                };
                if !due {
                    continue;
                }
                tokio::time::sleep(SETTLE).await;
                loop {
                    match events.try_recv() {
                        Ok(event) => {
                            this.on_event(&event).await;
                        }
                        Err(TryRecvError::Lagged(_)) => {}
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => return,
                    }
                }
                this.refresh().await;
            }
        })
    }

    pub async fn list(
        &self,
        unread_only: bool,
        offset: u32,
        limit: u32,
    ) -> Result<NotificationPage> {
        self.inner
            .store
            .notifications(unread_only, offset, limit)
            .await
    }

    pub async fn unread(&self) -> Result<u32> {
        self.inner.store.unread_notifications().await
    }

    pub async fn members(&self, id: &str, offset: u32, limit: u32) -> Result<MemberPage> {
        self.inner
            .store
            .notification_members(id, offset, limit)
            .await
    }

    /// Marks an item read up to the `version` that was shown.
    pub async fn mark_read(&self, id: &str, version: i64) -> Result<bool> {
        let changed = self.inner.store.mark_notification_read(id, version).await?;
        Ok(self.changed(changed))
    }

    pub async fn mark_unread(&self, id: &str) -> Result<bool> {
        let changed = self.inner.store.mark_notification_unread(id).await?;
        Ok(self.changed(changed))
    }

    /// Marks read what the feed held at `revision`. How many changed.
    pub async fn mark_all_read(&self, revision: i64) -> Result<u64> {
        let changed = self
            .inner
            .store
            .mark_all_notifications_read(revision)
            .await?;
        self.changed(changed > 0);
        Ok(changed)
    }
}
