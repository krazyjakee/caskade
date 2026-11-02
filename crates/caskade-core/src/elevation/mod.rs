//! Fixed administrator steps (STREAMING-PLAN §8.4, #255): a short list of
//! system changes behind one OS prompt, each recorded so its undo removes
//! exactly what it added.
//!
//! Callers name a [`Plan`]; nothing else crosses. A plan is a fixed set of
//! [`Change`]s per OS. Before asking, Caskade looks (unelevated) at which
//! are already there, asks once for the rest, then looks again: what
//! appeared is the [`Record`] the undo removes. A change that was there
//! before is never touched, by either step. The elevated side gets only
//! names from [`Step`] and paths Caskade works out itself, never text from
//! the webview: on Linux `pkexec /bin/sh -c <constant script> <step names>`
//! ([`linux`]), on Windows Caskade itself started with `runas` and a fixed
//! `--elevated` subcommand ([`windows`]). The elevated side rolls back its
//! own steps when one fails, so a failure, like a cancelled or refused
//! prompt, leaves the system as it was.

pub mod linux;
pub mod windows;

#[cfg(test)]
mod tests;

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A set of administrator steps Caskade knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Plan {
    /// What hosting game streams needs (§4.2, §4.5).
    StreamingHost,
}

impl Plan {
    fn key(self) -> &'static str {
        match self {
            Self::StreamingHost => "streaming-host",
        }
    }

    /// The changes the plan makes on `os`, in the order they are made.
    pub fn changes(self, os: crate::emulation::OperatingSystem) -> &'static [Change] {
        use crate::emulation::OperatingSystem::*;
        match (self, os) {
            (Self::StreamingHost, Linux) => {
                &[Change::UdevRules, Change::ModulesLoad, Change::UhidLoaded]
            }
            (Self::StreamingHost, Windows) => &[
                Change::HostProgram,
                Change::Service,
                Change::FirewallTcp,
                Change::FirewallUdp,
            ],
            (Self::StreamingHost, Macos) => &[],
        }
    }
}

/// One system change a plan makes. The names are all there is: the
/// elevated side maps each to fixed work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Change {
    /// Linux: `/etc/udev/rules.d/60-caskade-streaming.rules` (uinput, uhid
    /// and the virtual pads' nodes for the `input` group and the seat).
    UdevRules,
    /// Linux: `/etc/modules-load.d/60-caskade-streaming.conf` loading uhid
    /// at boot.
    ModulesLoad,
    /// Linux: the uhid module, loaded now.
    UhidLoaded,
    /// Windows: the host program and its configuration, copied from
    /// staging into `%ProgramFiles%\Caskade\Streaming\host` and checked.
    HostProgram,
    /// Windows: the on-demand `CaskadeStreaming` service the interactive
    /// user may start and stop.
    Service,
    /// Windows: the inbound TCP rule for the host program.
    FirewallTcp,
    /// Windows: the inbound UDP rule for the host program.
    FirewallUdp,
}

/// Add or remove one change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Add(Change),
    Remove(Change),
}

/// What runs behind the prompt: steps, and the folder of the host program
/// Caskade installed (Windows' service and firewall rules point into it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub steps: Vec<Step>,
    /// Where Caskade staged the host program (`<data>/streaming/host`).
    pub host_dir: PathBuf,
    /// The SHA-256 of the staged host's manifest, when the host program
    /// is copied ([`windows::write_manifest`]).
    pub manifest: Option<String>,
}

/// How a prompt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// The steps ran.
    Done,
    /// The user dismissed the prompt.
    Cancelled,
    /// The OS refused: a wrong password, or an account that isn't allowed.
    Refused,
}

/// Looks at the system and runs requests behind the OS prompt. The real
/// ones are [`linux::LinuxRunner`] and [`windows::WindowsRunner`]; tests
/// inject their own.
pub trait Runner: Send + Sync {
    /// Whether `change` is on the system now, looked at without elevation.
    fn present(&self, change: Change) -> bool;
    /// Runs `request` behind one prompt. An `Err` means the steps failed
    /// and were rolled back.
    fn run(&self, request: &Request) -> Result<Prompt>;
}

/// What a plan added, kept so its undo removes exactly that.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub added: BTreeSet<Change>,
}

/// What [`Elevation::apply`] or [`Elevation::undo`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Everything was already as asked: no prompt.
    NothingToDo,
    /// The steps ran; these changes appeared (apply) or went (undo).
    Done(BTreeSet<Change>),
    /// The prompt was dismissed; nothing changed.
    Cancelled,
    /// The OS refused the prompt; nothing changed.
    Refused,
}

impl Outcome {
    /// For people, when it didn't go through.
    pub fn message(&self) -> Option<String> {
        match self {
            Self::Cancelled => Some(crate::tr!(
                "The administrator step was cancelled, so nothing changed."
            )),
            Self::Refused => Some(crate::tr!(
                "The administrator step wasn't allowed, so nothing changed. It needs an administrator's password."
            )),
            Self::NothingToDo | Self::Done(_) => None,
        }
    }
}

/// Runs plans and their undos, keeping their records in
/// `<data>/elevation/<plan>.json`.
#[derive(Clone)]
pub struct Elevation {
    runner: Arc<dyn Runner>,
    os: crate::emulation::OperatingSystem,
    records: PathBuf,
    host_dir: PathBuf,
}

impl Elevation {
    /// This machine's runner, records under `<data>/elevation`, and the
    /// host program's folder (`<data>/streaming/host`).
    pub fn new(paths: &crate::AppPaths) -> Self {
        let os = crate::emulation::host_os();
        let runner: Arc<dyn Runner> = match os {
            crate::emulation::OperatingSystem::Windows => Arc::new(windows::WindowsRunner::new()),
            _ => Arc::new(linux::LinuxRunner),
        };
        Self::with_runner(runner, os, paths)
    }

    pub fn with_runner(
        runner: Arc<dyn Runner>,
        os: crate::emulation::OperatingSystem,
        paths: &crate::AppPaths,
    ) -> Self {
        Self {
            runner,
            os,
            records: paths.data.join("elevation"),
            host_dir: paths.data.join("streaming").join("host"),
        }
    }

    fn record_path(&self, plan: Plan) -> PathBuf {
        self.records.join(format!("{}.json", plan.key()))
    }

    /// What `plan` added and still owns; empty when nothing.
    pub fn record(&self, plan: Plan) -> Result<Record> {
        match std::fs::read(self.record_path(plan)) {
            // Names only: anything else in the file is refused, never run.
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                Error::Invalid(crate::tr!(
                    "the record of administrator steps is unreadable: {error}",
                    error = error.to_string()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Record::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, plan: Plan, record: &Record) -> Result<()> {
        let path = self.record_path(plan);
        if record.added.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        std::fs::create_dir_all(&self.records)?;
        write_atomically(&path, &serde_json::to_vec_pretty(record)?)
    }

    /// Makes `plan`'s changes that aren't there yet, behind one prompt.
    /// A cancelled or refused prompt, or failed steps, change nothing.
    pub async fn apply(&self, plan: Plan) -> Result<Outcome> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.apply_blocking(plan))
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?
    }

    fn apply_blocking(&self, plan: Plan) -> Result<Outcome> {
        let changes = plan.changes(self.os);
        if changes.is_empty() {
            return Err(Error::Invalid(crate::tr!(
                "this system doesn't need or support these administrator steps"
            )));
        }
        let mut record = self.record(plan)?;
        let missing: Vec<Change> = changes
            .iter()
            .copied()
            .filter(|change| !self.runner.present(*change))
            .collect();
        if missing.is_empty() {
            return Ok(Outcome::NothingToDo);
        }
        let manifest = if missing.contains(&Change::HostProgram) {
            Some(windows::write_manifest(&self.host_dir)?)
        } else {
            None
        };
        let request = Request {
            steps: missing.iter().copied().map(Step::Add).collect(),
            host_dir: self.host_dir.clone(),
            manifest,
        };
        match self.runner.run(&request)? {
            Prompt::Cancelled => return Ok(Outcome::Cancelled),
            Prompt::Refused => return Ok(Outcome::Refused),
            Prompt::Done => {}
        }
        let added: BTreeSet<Change> = missing
            .into_iter()
            .filter(|change| self.runner.present(*change))
            .collect();
        record.added.extend(added.iter().copied());
        self.save(plan, &record)?;
        Ok(Outcome::Done(added))
    }

    /// Removes what `plan` added and is still there, behind one prompt;
    /// what was there before it stays.
    pub async fn undo(&self, plan: Plan) -> Result<Outcome> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.undo_blocking(plan))
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?
    }

    fn undo_blocking(&self, plan: Plan) -> Result<Outcome> {
        let mut record = self.record(plan)?;
        let ours: Vec<Change> = plan
            .changes(self.os)
            .iter()
            .rev()
            .copied()
            .filter(|change| record.added.contains(change))
            .collect();
        let present: Vec<Change> = ours
            .iter()
            .copied()
            .filter(|change| self.runner.present(*change))
            .collect();
        if present.is_empty() {
            self.save(plan, &Record::default())?;
            return Ok(Outcome::NothingToDo);
        }
        let request = Request {
            steps: present.iter().copied().map(Step::Remove).collect(),
            host_dir: self.host_dir.clone(),
            manifest: None,
        };
        match self.runner.run(&request)? {
            Prompt::Cancelled => return Ok(Outcome::Cancelled),
            Prompt::Refused => return Ok(Outcome::Refused),
            Prompt::Done => {}
        }
        let removed: BTreeSet<Change> = present
            .into_iter()
            .filter(|change| !self.runner.present(*change) || *change == Change::UhidLoaded)
            .collect();
        // Whatever went is no longer ours; a module still in use stays
        // loaded, and is no longer counted as ours either.
        record.added.retain(|change| {
            plan.changes(self.os).contains(change)
                && !removed.contains(change)
                && self.runner.present(*change)
        });
        self.save(plan, &record)?;
        Ok(Outcome::Done(removed))
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path)?;
    Ok(())
}
