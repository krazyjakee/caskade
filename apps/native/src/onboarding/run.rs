//! First-run setup's work after the questions (#97), as the webview's
//! `onboarding/run.ts`: saves the choices, then finds games, emulators,
//! ROMs and RomM's library, reporting each task as it goes. Setup counts as
//! done once the choices are saved and the first scan has started, so the
//! core's own first scan doesn't run a second one; the rest may finish
//! after the user opens the library.

use crate::data::sleep;
use std::collections::BTreeSet;
use std::time::Duration;

use caskade_core::error::ErrorReport;
use caskade_ui_data::emulation::{DetectJob, Emulation};
use caskade_ui_data::romm::{RommAuth, RommServerConfig};
use caskade_ui_data::{Discovery, Settings};
use caskade_ui_i18n::{t, tn};
use serde_json::{Value, json};

use super::sound::Cue;
use crate::data::change;

/// A RomM server to add: its address and sign-in.
#[derive(Clone, Debug, PartialEq)]
pub struct RommChoice {
    pub url: String,
    pub auth: RommAuth,
    /// With a password only.
    pub username: String,
    /// The password or API token; it goes to the keychain.
    pub secret: String,
}

impl Default for RommChoice {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth: RommAuth::Password,
            username: String::new(),
            secret: String::new(),
        }
    }
}

/// What the questions chose.
#[derive(Clone, Debug, PartialEq)]
pub struct Choices {
    /// Find games installed by Steam, Heroic and the rest.
    pub pc_games: bool,
    /// Keep the play time those launchers know.
    pub play_time: bool,
    /// A ROM collection to set up, or none.
    pub rom_folder: Option<String>,
    pub romm: Option<RommChoice>,
    /// Fetch artwork and game info in the background.
    pub online: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            pc_games: true,
            play_time: true,
            rom_folder: None,
            romm: None,
            online: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskId {
    Games,
    Emulators,
    Roms,
    Romm,
    Artwork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Waiting,
    Running,
    Done,
    Warning,
    Failed,
    /// It goes on after setup.
    Background,
    Skipped,
}

impl TaskState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Done => "done",
            Self::Warning => "warning",
            Self::Failed => "failed",
            Self::Background => "background",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub label: String,
    pub state: TaskState,
    pub detail: String,
}

/// The tasks `choices` run, in order, labelled in the language chosen at
/// setup's first question.
pub fn planned_tasks(choices: &Choices) -> Vec<Task> {
    let task = |id, label: String| Task {
        id,
        label,
        state: TaskState::Waiting,
        detail: String::new(),
    };
    let mut tasks = Vec::new();
    if choices.pc_games {
        tasks.push(task(TaskId::Games, t!("Games on this PC")));
    }
    if choices.rom_folder.is_some() {
        tasks.push(task(TaskId::Emulators, t!("Emulators")));
        tasks.push(task(TaskId::Roms, t!("Retro games")));
    }
    if choices.romm.is_some() {
        // A product name.
        tasks.push(task(TaskId::Romm, "RomM".to_owned()));
    }
    if choices.online {
        tasks.push(task(TaskId::Artwork, t!("Artwork and game info")));
    }
    tasks
}

fn game_count(total: u32) -> String {
    tn!("# game", "# games", total)
}

fn no_emulator(platforms: &[String]) -> String {
    t!(
        "No emulator for {platforms}",
        platforms = platforms.join(", ")
    )
}

/// The app's parts setup works with.
#[derive(Clone)]
pub struct SetupParts {
    pub settings: Settings,
    pub discovery: Option<Discovery>,
    pub emulation: Option<Emulation>,
    pub romm: Option<caskade_ui_data::romm::Romm>,
}

/// What a run reports to.
pub trait Report {
    fn update(&self, tasks: Vec<Task>);
    fn cue(&self, cue: Cue);
    /// Settings are saved: the library may open while the rest goes on.
    fn ready(&self);
}

/// Between looks at work that's running.
const POLL: Duration = Duration::from_millis(400);

/// A section's saved value with `patch`'s fields over it.
async fn update_section(
    settings: &Settings,
    namespace: &str,
    patch: Value,
) -> Result<(), ErrorReport> {
    let settings = settings.clone();
    let namespace = namespace.to_owned();
    change(async move {
        settings
            .update(&namespace, move |mut value| {
                if let (Some(value), Some(patch)) = (value.as_object_mut(), patch.as_object()) {
                    for (key, field) in patch {
                        value.insert(key.clone(), field.clone());
                    }
                }
                value
            })
            .await
            .map(|_| ())
    })
    .await
}

/// Saves the choices that are settings. Setup counts as done after this:
/// the first scan and the online lookups may start.
pub async fn save_choices(settings: &Settings, choices: &Choices) -> Result<(), ErrorReport> {
    update_section(
        settings,
        "core.discovery",
        json!({ "enabled": choices.pc_games }),
    )
    .await?;
    // The play history section belongs to discovery's sources; without it
    // there's nothing to keep.
    let _ = update_section(
        settings,
        "discovery.import",
        json!({ "seedPlaytime": choices.play_time }),
    )
    .await;
    update_section(
        settings,
        "core.metadata",
        json!({ "online": choices.online }),
    )
    .await
}

pub async fn mark_completed(settings: &Settings) -> Result<(), ErrorReport> {
    update_section(settings, "core.onboarding", json!({ "completed": true })).await
}

/// Each task as it goes.
struct Tasks<'a> {
    list: std::cell::RefCell<Vec<Task>>,
    report: &'a dyn Report,
}

impl Tasks<'_> {
    fn set(&self, id: TaskId, state: TaskState, detail: impl Into<String>) {
        let detail = detail.into();
        let mut list = self.list.borrow_mut();
        let Some(task) = list.iter_mut().find(|task| task.id == id) else {
            return;
        };
        if task.state == state && task.detail == detail {
            return;
        }
        task.state = state;
        task.detail = detail;
        let shown = list.clone();
        drop(list);
        self.report.update(shown);
    }
}

/// Runs setup's work for `choices`; the tasks as they ended, or why the
/// choices couldn't be saved (nothing else ran then).
pub async fn run_setup(
    parts: &SetupParts,
    choices: &Choices,
    report: &dyn Report,
) -> Result<Vec<Task>, ErrorReport> {
    let tasks = Tasks {
        list: std::cell::RefCell::new(planned_tasks(choices)),
        report,
    };
    report.update(tasks.list.borrow().clone());
    save_choices(&parts.settings, choices).await?;
    // The scan starts before setup counts as done, so the core's own first
    // scan doesn't run a second one.
    let scan_started = match (&parts.discovery, choices.pc_games) {
        (Some(discovery), true) => {
            tasks.set(
                TaskId::Games,
                TaskState::Running,
                t!("Scanning installed launchers and game folders…"),
            );
            let discovery = discovery.clone();
            Some(change(async move { discovery.scan().await }).await)
        }
        (None, true) => {
            tasks.set(
                TaskId::Games,
                TaskState::Failed,
                t!("The scan didn't finish."),
            );
            None
        }
        _ => None,
    };
    mark_completed(&parts.settings).await?;
    report.ready();
    if choices.online {
        tasks.set(
            TaskId::Artwork,
            TaskState::Background,
            t!("Matches and downloads run in the background as games arrive. You can open your library now."),
        );
    }
    let games = async {
        if let (Some(discovery), Some(_)) = (&parts.discovery, scan_started) {
            games(discovery, &tasks, report).await;
        }
    };
    let retro = async {
        if let Some(folder) = &choices.rom_folder {
            match &parts.emulation {
                Some(emulation) => retro_games(emulation, folder, &tasks, report).await,
                None => tasks.set(
                    TaskId::Emulators,
                    TaskState::Failed,
                    t!("Emulator detection failed. Retry in Settings → Emulation."),
                ),
            }
        }
    };
    let romm = async {
        if let Some(choice) = &choices.romm {
            romm(parts, choice, &tasks, report).await;
        }
    };
    tokio::join!(games, retro, romm);
    Ok(tasks.list.into_inner())
}

fn finished(phase: &str) -> bool {
    matches!(phase, "complete" | "failed" | "cancelled")
}

async fn games(discovery: &Discovery, tasks: &Tasks<'_>, report: &dyn Report) {
    let mut announced = BTreeSet::new();
    loop {
        let read = discovery.clone();
        let snapshot = match change(async move { read.status().await }).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tasks.set(TaskId::Games, TaskState::Failed, error.summary);
                return;
            }
        };
        let found: Vec<_> = snapshot
            .sources
            .iter()
            .filter(|source| source.count > 0)
            .collect();
        for source in &found {
            if announced.insert(source.source.as_str().to_owned()) {
                report.cue(Cue::Found);
            }
        }
        let listed = found
            .iter()
            .map(|source| {
                format!(
                    "{} {}",
                    crate::settings::source_name(source.source.as_str()),
                    source.count
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if !finished(&snapshot.phase) {
            let active = snapshot
                .active_sources
                .iter()
                .map(|source| crate::settings::source_name(source))
                .collect::<Vec<_>>()
                .join(" · ");
            let detail = if snapshot.phase == "importing" {
                t!(
                    "Adding discovered games — {count} found",
                    count = game_count(snapshot.found)
                )
            } else if !active.is_empty() {
                t!(
                    "Scanning {sources} — {found} found so far",
                    sources = active,
                    found = if listed.is_empty() {
                        game_count(0)
                    } else {
                        listed.clone()
                    }
                )
            } else if !listed.is_empty() {
                listed
            } else {
                t!("Scanning installed launchers and game folders…")
            };
            tasks.set(TaskId::Games, TaskState::Running, detail);
            sleep(POLL).await;
            continue;
        }
        let problems: Vec<String> = snapshot
            .sources
            .iter()
            .flat_map(|source| {
                let name = crate::settings::source_name(source.source.as_str());
                source
                    .error
                    .iter()
                    .map(ToString::to_string)
                    .chain(source.warnings.iter().cloned())
                    .map(move |warning| format!("{name}: {warning}"))
                    .collect::<Vec<_>>()
            })
            .chain(snapshot.warnings.iter().cloned())
            .collect();
        if snapshot.phase == "failed" {
            tasks.set(
                TaskId::Games,
                TaskState::Failed,
                snapshot
                    .error
                    .clone()
                    .unwrap_or_else(|| t!("The scan didn't finish.")),
            );
        } else if snapshot.phase == "cancelled" {
            tasks.set(
                TaskId::Games,
                TaskState::Skipped,
                t!("Scan cancelled. Retry in Settings → Library."),
            );
        } else if !problems.is_empty() {
            let mut parts = vec![game_count(snapshot.imported)];
            parts.extend(problems);
            tasks.set(TaskId::Games, TaskState::Warning, parts.join(" · "));
        } else if snapshot.found == 0 {
            tasks.set(TaskId::Games, TaskState::Warning, t!("None found"));
        } else if snapshot.imported == 0 {
            // Found but none added: they wait for review in Settings.
            tasks.set(
                TaskId::Games,
                TaskState::Warning,
                t!(
                    "{count} awaiting review in Settings → Library.",
                    count = game_count(snapshot.found)
                ),
            );
        } else {
            // What was added, as the library counts it.
            tasks.set(
                TaskId::Games,
                TaskState::Done,
                game_count(snapshot.imported),
            );
        }
        return;
    }
}

fn detected(job: &DetectJob) -> bool {
    job.status != caskade_core::emulation::DetectStatus::Running
}

fn names(job: &DetectJob) -> String {
    let mut seen = BTreeSet::new();
    job.results
        .iter()
        .filter(|row| seen.insert(row.name.clone()))
        .map(|row| row.name.clone())
        .collect::<Vec<_>>()
        .join(" · ")
}

async fn retro_games(emulation: &Emulation, folder: &str, tasks: &Tasks<'_>, report: &dyn Report) {
    tasks.set(TaskId::Emulators, TaskState::Running, t!("Looking…"));
    let detection = async {
        let start = emulation.clone();
        let mut job = change(async move { start.detect(None).await }).await?;
        let mut announced = BTreeSet::new();
        loop {
            for row in &job.results {
                if announced.insert(row.key.clone()) {
                    report.cue(Cue::Found);
                }
            }
            if detected(&job) {
                break;
            }
            let shown = names(&job);
            tasks.set(
                TaskId::Emulators,
                TaskState::Running,
                if shown.is_empty() {
                    t!("Looking for installed emulators…")
                } else {
                    shown
                },
            );
            sleep(POLL).await;
            if let Some(next) = emulation.detection()
                && next.id == job.id
            {
                job = next;
            }
        }
        Ok::<DetectJob, ErrorReport>(job)
    }
    .await;
    match detection {
        Err(error) => tasks.set(TaskId::Emulators, TaskState::Failed, error.summary),
        Ok(job) if job.status == caskade_core::emulation::DetectStatus::Failed => tasks.set(
            TaskId::Emulators,
            TaskState::Failed,
            job.error
                .clone()
                .unwrap_or_else(|| t!("Emulator detection failed. Retry in Settings → Emulation.")),
        ),
        Ok(job) if job.status == caskade_core::emulation::DetectStatus::Cancelled => {
            tasks.set(
                TaskId::Emulators,
                TaskState::Skipped,
                t!("Emulator detection was cancelled."),
            );
            tasks.set(
                TaskId::Roms,
                TaskState::Skipped,
                t!("Set up ROM folders in Settings → Emulation."),
            );
            return;
        }
        Ok(job) => {
            let fresh: Vec<String> = job
                .results
                .iter()
                .filter(|row| !row.configured)
                .map(|row| row.key.clone())
                .collect();
            let imported = if fresh.is_empty() {
                Ok(())
            } else {
                let import = emulation.clone();
                let (id, revision) = (job.id.clone(), job.revision);
                change(async move { import.import_detected(&id, revision, &fresh).await })
                    .await
                    .map(|_| ())
            };
            match imported {
                Err(error) => tasks.set(TaskId::Emulators, TaskState::Failed, error.summary),
                Ok(()) if job.results.is_empty() => {
                    tasks.set(TaskId::Emulators, TaskState::Warning, t!("None found"))
                }
                Ok(()) => tasks.set(TaskId::Emulators, TaskState::Done, names(&job)),
            }
        }
    }

    tasks.set(
        TaskId::Roms,
        TaskState::Running,
        t!("Sorting your folders…"),
    );
    let setup_with = emulation.clone();
    let folder = folder.to_owned();
    let setup = match change(async move { setup_with.setup_rom_folder(&folder).await }).await {
        Ok(setup) => setup,
        Err(error) => {
            tasks.set(TaskId::Roms, TaskState::Failed, error.summary);
            return;
        }
    };
    if setup.added.is_empty() && setup.existing == 0 {
        tasks.set(
            TaskId::Roms,
            TaskState::Warning,
            if setup.no_emulator.is_empty() {
                t!("No console folders found (like snes or psx)")
            } else {
                no_emulator(&setup.no_emulator)
            },
        );
        return;
    }
    let added = setup
        .added
        .iter()
        .map(|scanner| scanner.name.clone())
        .collect::<Vec<_>>()
        .join(" · ");
    tasks.set(
        TaskId::Roms,
        TaskState::Running,
        if added.is_empty() {
            t!("Scanning…")
        } else {
            added
        },
    );
    let scan = emulation.clone();
    match change(async move { scan.scan_all().await }).await {
        Err(error) => tasks.set(TaskId::Roms, TaskState::Failed, error.summary),
        Ok(summaries) => {
            let games: usize = summaries.iter().map(|summary| summary.games.len()).sum();
            if games > 0 {
                report.cue(Cue::Found);
            }
            let missing = if setup.no_emulator.is_empty() {
                String::new()
            } else {
                format!(" · {}", no_emulator(&setup.no_emulator))
            };
            tasks.set(
                TaskId::Roms,
                if setup.no_emulator.is_empty() {
                    TaskState::Done
                } else {
                    TaskState::Warning
                },
                format!("{}{missing}", game_count(games as u32)),
            );
        }
    }
}

/// The server a RomM choice saves: named after its host, with RomM's
/// defaults (every platform, synced at startup and every 6 hours).
pub fn romm_server(choice: &RommChoice, taken: &[String]) -> RommServerConfig {
    let trimmed = choice.url.trim();
    let url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    // The core refuses a bad address with a clearer message.
    let name = url::Url::parse(&url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "RomM".to_owned());
    let id = crate::settings::romm_server_id(&name, taken);
    RommServerConfig {
        auth: choice.auth,
        username: if choice.auth == RommAuth::Password {
            choice.username.trim().to_owned()
        } else {
            String::new()
        },
        ..RommServerConfig::new(id, name, url)
    }
}

async fn romm(parts: &SetupParts, choice: &RommChoice, tasks: &Tasks<'_>, report: &dyn Report) {
    tasks.set(TaskId::Romm, TaskState::Running, t!("Connecting…"));
    let Some(romm) = parts.romm.clone() else {
        tasks.set(
            TaskId::Romm,
            TaskState::Failed,
            t!("This part of Caskade didn't start this session. Restarting Caskade may fix it."),
        );
        return;
    };
    let choice = choice.clone();
    let saved = {
        let romm = romm.clone();
        let choice = choice.clone();
        change(async move {
            let overview = romm.overview().await?;
            let taken: Vec<String> = overview
                .settings
                .servers
                .iter()
                .map(|server| server.id.clone())
                .collect();
            let server = romm_server(&choice, &taken);
            let secret = choice.secret.trim().to_owned();
            romm.save_server(server.clone(), (!secret.is_empty()).then_some(secret))
                .await?;
            Ok(server.id)
        })
        .await
    };
    let id = match saved {
        Ok(id) => id,
        Err(error) => {
            tasks.set(TaskId::Romm, TaskState::Failed, error.summary);
            return;
        }
    };
    tasks.set(TaskId::Romm, TaskState::Running, t!("Syncing…"));
    match change(async move { romm.sync(Some(id)).await }).await {
        Err(error) => tasks.set(TaskId::Romm, TaskState::Failed, error.summary),
        Ok(reports) => {
            let report_now = reports.first();
            let games = report_now.map_or(0, |found| found.added + found.updated + found.unchanged);
            if games > 0 {
                report.cue(Cue::Found);
            }
            let warnings = report_now
                .map(|found| found.warnings.clone())
                .unwrap_or_default();
            let mut detail = vec![game_count(games)];
            detail.extend(warnings.iter().cloned());
            tasks.set(
                TaskId::Romm,
                if warnings.is_empty() {
                    TaskState::Done
                } else {
                    TaskState::Warning
                },
                detail.join(" · "),
            );
        }
    }
}
