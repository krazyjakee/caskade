use super::{ProcessIdentity, ProcessInfo, ProcessSnapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Explicit emulator tracking; ordinary games retain automatic path/ancestry hints.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum TrackingMode {
    #[default]
    Automatic,
    ProcessTree,
    OriginalProcess,
    Folder,
    ProcessName,
}

/// Trusted launch metadata, assembled from the selected entry and discovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackingTarget {
    #[serde(default)]
    pub mode: TrackingMode,
    pub real_dir: Option<PathBuf>,
    pub alias_dir: Option<PathBuf>,
    pub executables: Vec<PathBuf>,
    pub exe_names: Vec<String>,
    pub steam_app_id: Option<u32>,
    #[serde(default)]
    pub excluded_executables: Vec<PathBuf>,
}

impl TrackingTarget {
    /// Discovery hints supplement paths resolved from the current entry.
    pub fn with_hint(mut self, hint: &caskade_discovery::TrackingHint) -> Self {
        if self.real_dir.is_none() {
            self.real_dir = hint.install_dir.clone();
        }
        self.exe_names.extend(hint.exe_names.iter().cloned());
        self.exe_names.sort();
        self.exe_names.dedup();
        self.steam_app_id = hint.steam_app_id.filter(|id| *id > 0).or(self.steam_app_id);
        self
    }

    fn contains(&self, path: &Path) -> bool {
        self.real_dir
            .iter()
            .chain(self.alias_dir.iter())
            .any(|root| below(path, root))
    }

    fn excluded(&self, process: &ProcessInfo) -> bool {
        if process
            .executable
            .as_ref()
            .is_some_and(|exe| self.excluded_executables.iter().any(|p| equal(exe, p)))
        {
            return true;
        }
        let in_game = process
            .executable
            .as_deref()
            .is_some_and(|exe| self.contains(exe));
        process
            .executable
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .into_iter()
            .chain(process.name.as_deref())
            .any(|name| {
                let helper = [
                    "steamwebhelper",
                    "steamwebhelper.exe",
                    "wineserver",
                    "wineserver64",
                    "services.exe",
                    "wineboot.exe",
                    "winedevice.exe",
                    "rpcss.exe",
                    "plugplay.exe",
                    "svchost.exe",
                    "conhost.exe",
                    "werfault.exe",
                    "winedbg.exe",
                ]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n));
                (helper && !in_game)
                    || caskade_discovery::data::Data::builtin().is_ok_and(|data| {
                        data.launchers.launchers.iter().any(|launcher| {
                            launcher.exes.iter().any(|n| name.eq_ignore_ascii_case(n))
                        })
                    })
            })
    }

    fn matches(&self, process: &ProcessInfo, descendant: bool) -> bool {
        if self.excluded(process) {
            return false;
        }
        match self.mode {
            TrackingMode::OriginalProcess => return false,
            TrackingMode::ProcessTree => return descendant,
            TrackingMode::Folder => {
                return process
                    .executable
                    .as_deref()
                    .is_some_and(|p| self.contains(p));
            }
            TrackingMode::ProcessName => {
                return process
                    .executable
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|p| p.to_str())
                    .into_iter()
                    .chain(process.name.as_deref())
                    .any(|name| {
                        self.exe_names.iter().any(|wanted| {
                            if cfg!(windows) {
                                name.eq_ignore_ascii_case(wanted)
                            } else {
                                name == wanted
                            }
                        })
                    });
            }
            TrackingMode::Automatic => {}
        }
        if self.steam_app_id.is_some() && self.steam_app_id == process.steam_app_id {
            return true;
        }
        if let Some(exe) = &process.executable
            && (self.executables.iter().any(|target| equal(exe, target)) || self.contains(exe))
        {
            return true;
        }
        // Wine/Proton expose the host runner as executable. Its argv can carry
        // the actual game path; plain options and relative names do not match.
        if process
            .arguments
            .iter()
            .any(|arg| arg.is_absolute() && self.contains(arg))
        {
            return true;
        }
        let name_matches = process
            .executable
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .into_iter()
            .chain(process.name.as_deref())
            .chain(
                process
                    .arguments
                    .iter()
                    .filter_map(|p| p.to_str())
                    .filter_map(|arg| arg.rsplit(['/', '\\']).next()),
            )
            .any(|name| {
                self.exe_names.iter().any(|hint| {
                    if cfg!(windows) || hint.to_ascii_lowercase().ends_with(".exe") {
                        name.eq_ignore_ascii_case(hint)
                    } else {
                        name == hint
                    }
                })
            });

        name_matches && (descendant || process.cwd.as_deref().is_some_and(|cwd| self.contains(cwd)))
    }
}

/// A baseline excludes unrelated processes that predate this launch. Descendant
/// ancestry is retained after the launcher exits, but active game membership is
/// always checked against PID + start time (never PID alone).
pub struct ProcessTracker {
    target: TrackingTarget,
    /// Whether all of the target's paths are canonical yet.
    resolved: bool,
    baseline: BTreeSet<ProcessIdentity>,
    lineage: BTreeMap<u32, ProcessIdentity>,
    games: BTreeSet<ProcessIdentity>,
}

impl ProcessTracker {
    pub fn new(target: TrackingTarget, baseline: &ProcessSnapshot) -> Self {
        Self {
            target,
            resolved: false,
            baseline: baseline.processes.iter().map(|p| p.identity).collect(),
            lineage: BTreeMap::new(),
            games: BTreeSet::new(),
        }
    }

    /// What was running before this session, by identity.
    pub(super) fn baseline(&self) -> &BTreeSet<ProcessIdentity> {
        &self.baseline
    }

    /// Seed the spawned launcher; it is not counted as game time unless it
    /// independently matches the target. Recovery seeds only recorded games.
    pub fn launcher(&mut self, identity: ProcessIdentity) {
        self.lineage.insert(identity.pid, identity);
        self.baseline.remove(&identity);
    }

    pub fn recover(&mut self, identities: &[ProcessIdentity]) {
        for identity in identities {
            self.games.insert(*identity);
            self.lineage.insert(identity.pid, *identity);
            self.baseline.remove(identity);
        }
    }

    /// Path canonicalization may touch a disconnected mount, so app callers
    /// perform matching on the blocking pool as well as snapshot collection.
    pub async fn observe_async(
        mut self,
        snapshot: ProcessSnapshot,
    ) -> Result<(Self, Vec<ProcessIdentity>), super::TrackingError> {
        tokio::task::spawn_blocking(move || {
            let active = self.observe(&snapshot);
            (self, active)
        })
        .await
        .map_err(|e| super::TrackingError::Worker(e.to_string()))
    }

    pub fn observe(&mut self, snapshot: &ProcessSnapshot) -> Vec<ProcessIdentity> {
        // Once, not on every comparison of every poll; a path that
        // doesn't exist yet is tried again on the next.
        if !self.resolved {
            let target = &mut self.target;
            self.resolved = true;
            for path in (target.real_dir.iter_mut())
                .chain(target.alias_dir.iter_mut())
                .chain(&mut target.executables)
                .chain(&mut target.excluded_executables)
            {
                match std::fs::canonicalize(&*path) {
                    Ok(canonical) => *path = canonical,
                    Err(_) => self.resolved = false,
                }
            }
        }
        let current: BTreeMap<_, _> = snapshot
            .processes
            .iter()
            .map(|p| (p.identity.pid, p.identity))
            .collect();
        let mut children: BTreeMap<u32, Vec<&ProcessInfo>> = BTreeMap::new();
        let mut active = BTreeSet::new();
        for process in &snapshot.processes {
            if self.baseline.contains(&process.identity) {
                continue;
            }
            if let Some(parent) = process.parent {
                children.entry(parent).or_default().push(process);
            }
            if !self.target.excluded(process)
                && (self.games.contains(&process.identity) || self.target.matches(process, false))
            {
                self.games.insert(process.identity);
                self.lineage.insert(process.identity.pid, process.identity);
                active.insert(process.identity);
            }
        }
        let mut queue: Vec<_> = self.lineage.values().copied().collect();
        let mut visited = BTreeSet::new();
        while let Some(parent) = queue.pop() {
            let is_game = self.games.contains(&parent);
            if !visited.insert((parent, is_game))
                || current.get(&parent.pid).is_some_and(|id| *id != parent)
            {
                continue;
            }
            if self.target.mode == TrackingMode::OriginalProcess {
                continue;
            }
            for process in children.get(&parent.pid).into_iter().flatten() {
                let id = process.identity;
                // A child older than its parent is a reused parent PID. On
                // Windows and macOS a clock set back can make a real child
                // look like that: it then joins only by matching the target.
                if id == parent || id.started_at < parent.started_at {
                    continue;
                }
                self.lineage.insert(id.pid, id);
                if !self.target.excluded(process)
                    && ((is_game
                        && matches!(
                            self.target.mode,
                            TrackingMode::Automatic | TrackingMode::ProcessTree
                        ))
                        || self.games.contains(&id)
                        || self.target.matches(process, true))
                {
                    self.games.insert(id);
                    active.insert(id);
                }
                queue.push(id);
            }
        }
        active.into_iter().collect()
    }
}

fn normalized(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}
/// Whether `a` is `b`, a canonical path.
fn equal(a: &Path, b: &Path) -> bool {
    let a = normalized(a);
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == *b
    }
}
/// Whether `path` is inside `root`, a canonical path.
fn below(path: &Path, root: &Path) -> bool {
    let path = normalized(path);
    if cfg!(windows) {
        let (path, root) = (
            path.to_string_lossy().to_lowercase(),
            root.to_string_lossy().to_lowercase(),
        );
        Path::new(&path).starts_with(Path::new(&root))
    } else {
        path.starts_with(root)
    }
}

#[cfg(test)]
mod tests;
