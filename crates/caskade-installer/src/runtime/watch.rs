//! Out-of-root side effects of native installers. While a `Run` step executes,
//! the user-visible locations installers write to (Start menu, Desktop,
//! AppData, ProgramData, Documents and their XDG/macOS equivalents) are
//! watched with `notify`; files and directories created there are recorded in
//! the executor journal, so rollback removes them and the manifest lists them.
//! A location that can't be watched is left out with a warning.
//! Pre-existing files that an installer merely modifies are never recorded:
//! their original contents are unknown, and deleting them would lose user data.
use crate::{
    exec::{JournalEntry, JournalHandle, ProcessObservation, ProcessObserver, Result, StepContext},
    manifest::ExternalRoots,
    runtime::ProcessCommand,
};
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
    time::{Duration, SystemTime},
};

/// Delay for late events after the process exits (Windows and FSEvents batch;
/// FSEvents can take a second to deliver).
const SETTLE: Duration = if cfg!(target_os = "macos") {
    Duration::from_secs(1)
} else {
    Duration::from_millis(250)
};
/// Late events keep arriving in batches: wait for a quiet `SETTLE`, at most this long.
const MAX_SETTLE: Duration = Duration::from_secs(5);
/// Bound on entries discovered by walking newly created directories.
const MAX_WALK: usize = 100_000;

/// Whether a path was made during the run, so rollback and uninstall may
/// remove it. A birth time decides: before the watch began, the path was
/// already there, whatever an event says. Without one (filesystems that don't
/// keep it), only the watcher's own create event counts; a path found through
/// an ancestor or a folder walk then counts as pre-existing and is left alone.
fn admitted(born: Option<SystemTime>, observed: bool, started: SystemTime) -> bool {
    match born {
        Some(born) => born >= started,
        None => observed,
    }
}

/// Every entry of `directory`, descending only into folders modified since
/// `since`: adding or removing an entry updates a folder's modification time,
/// so this reaches what a run made without reading untouched trees. The
/// storage root (the game itself) is left out.
fn changed(
    directory: &Path,
    since: SystemTime,
    storage: &Path,
    paths: &mut BTreeSet<PathBuf>,
    budget: &mut usize,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        if path.starts_with(storage) {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() && metadata.modified().is_ok_and(|modified| modified >= since) {
            changed(&path, since, storage, paths, budget);
        }
        paths.insert(path);
    }
}

fn walk(directory: &Path, paths: &mut BTreeSet<PathBuf>, budget: &mut usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        // Never follow links out of the created tree.
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        paths.insert(path.clone());
        if is_dir {
            walk(&path, paths, budget);
        }
    }
}

/// Standard per-user and machine-wide locations for this host, as named
/// scopes (the manifest stores scope-relative paths, never absolute ones).
/// Missing variables or directories are skipped.
pub fn default_watch_roots() -> ExternalRoots {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut roots = ExternalRoots::new();
    let mut add = |scope: &str, path: Option<PathBuf>| {
        if let Some(path) = path.filter(|path| path.is_dir()) {
            roots.insert(scope.into(), path);
        }
    };
    if cfg!(windows) {
        let profile = var("USERPROFILE");
        // Roaming AppData holds the per-user Start menu; ProgramData the
        // all-users one.
        add("user.app_data", var("APPDATA"));
        add("user.local_app_data", var("LOCALAPPDATA"));
        add("machine.program_data", var("PROGRAMDATA"));
        add(
            "user.desktop",
            profile.as_ref().map(|home| home.join("Desktop")),
        );
        add(
            "user.documents",
            profile.as_ref().map(|home| home.join("Documents")),
        );
        add(
            "public.desktop",
            var("PUBLIC").map(|public| public.join("Desktop")),
        );
    } else if let Some(home) = var("HOME") {
        if cfg!(target_os = "macos") {
            add(
                "user.application_support",
                Some(home.join("Library/Application Support")),
            );
            add("user.preferences", Some(home.join("Library/Preferences")));
            add("user.applications", Some(home.join("Applications")));
        } else {
            add(
                "user.data",
                Some(var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"))),
            );
            add(
                "user.config",
                Some(var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"))),
            );
        }
        add("user.desktop", Some(home.join("Desktop")));
        add("user.documents", Some(home.join("Documents")));
    }
    roots
}

/// [`ProcessObserver`] that journals files created outside the storage root.
/// Convert the journal with [`crate::manifest::effects_from_journal`] and
/// [`Self::external_roots`] to record the effects in the install manifest.
#[derive(Debug, Clone)]
pub struct OutOfRootWatcher {
    roots: ExternalRoots,
}

impl OutOfRootWatcher {
    pub fn new(roots: ExternalRoots) -> Self {
        Self { roots }
    }
    pub fn for_host() -> Self {
        Self::new(default_watch_roots())
    }
    pub fn external_roots(&self) -> &ExternalRoots {
        &self.roots
    }
}

impl ProcessObserver for OutOfRootWatcher {
    fn start(
        &self,
        _: &ProcessCommand,
        _: &JournalHandle,
        context: &StepContext,
    ) -> Result<Box<dyn ProcessObservation>> {
        // The final game directory's parent is the storage root; staging and
        // the game itself live below it and are never external effects.
        let game = context.final_destination_path(Path::new("."))?;
        let storage = game.parent().map(Path::to_path_buf).unwrap_or(game);
        let storage = dunce::canonicalize(&storage).unwrap_or(storage);
        let started = SystemTime::now();
        let (send, events) = channel();
        let mut watcher = notify::recommended_watcher(send).map_err(watch_error)?;
        let mut watched = vec![];
        for root in self.roots.values() {
            let Ok(root) = dunce::canonicalize(root) else {
                continue;
            };
            // A storage root inside a watched location is excluded by path;
            // a watched location inside the storage root is not watched.
            if root.starts_with(&storage) {
                continue;
            }
            // A location too big to watch (inotify's limit on folders) is
            // left out rather than failing the install: what the installer
            // writes there is then neither recorded nor rolled back.
            if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
                tracing::warn!(%error, root = %root.display(), "location not watched");
                let _ = watcher.unwatch(&root);
                continue;
            }
            watched.push(root);
        }
        Ok(Box::new(Observation {
            watcher: Some(watcher),
            events,
            storage,
            watched,
            started,
        }))
    }
}

fn watch_error(error: notify::Error) -> crate::exec::ExecError {
    crate::exec::ExecError::Io(std::io::Error::other(format!(
        "out-of-root watcher failed: {error}"
    )))
}

struct Observation {
    watcher: Option<notify::RecommendedWatcher>,
    events: Receiver<notify::Result<notify::Event>>,
    storage: PathBuf,
    watched: Vec<PathBuf>,
    started: SystemTime,
}

impl ProcessObservation for Observation {
    fn finish(mut self: Box<Self>, journal: &JournalHandle) -> Result<()> {
        std::thread::sleep(SETTLE);
        let mut events = vec![];
        let deadline = std::time::Instant::now() + MAX_SETTLE;
        while let Ok(event) = self.events.recv_timeout(SETTLE) {
            events.push(event);
            if std::time::Instant::now() >= deadline {
                break;
            }
        }
        drop(self.watcher.take());
        events.extend(self.events.try_iter());
        let mut created = BTreeSet::new();
        let mut rescan = BTreeSet::new();
        for event in events {
            let event = event.map_err(watch_error)?;
            // FSEvents gives up on per-file events when it coalesces and asks
            // for a rescan of a folder (every watched location if it names none).
            if event.need_rescan() {
                if event.paths.is_empty() {
                    rescan.extend(self.watched.iter().cloned());
                } else {
                    rescan.extend(event.paths);
                }
                continue;
            }
            match event.kind {
                EventKind::Create(_) => created.extend(event.paths),
                // Renames into a watched location appear as the new name.
                EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                    if let Some(path) = event.paths.last() {
                        created.insert(path.clone());
                    }
                }
                _ => {}
            }
        }
        // FSEvents coalesces events, so a file can arrive without the
        // directories made for it: its ancestors inside a watched location
        // are candidates, and so (below) is everything inside a new folder,
        // including entries made before the recursive watch reached it
        // (inotify adds watches on new directories asynchronously).
        let observed = created;
        let mut derived = BTreeSet::new();
        for path in &observed {
            let Some(root) = self.watched.iter().find(|root| path.starts_with(root)) else {
                continue;
            };
            for ancestor in path.ancestors().skip(1) {
                if ancestor == root.as_path() || !ancestor.starts_with(root) {
                    break;
                }
                derived.insert(ancestor.to_path_buf());
            }
        }
        let born = |path: &Path| {
            std::fs::symlink_metadata(path)
                .and_then(|metadata| metadata.created())
                .ok()
        };
        let directories: Vec<_> = observed
            .iter()
            .chain(&derived)
            .filter(|path| {
                path.is_dir() && admitted(born(path), observed.contains(*path), self.started)
            })
            .cloned()
            .collect();
        let mut budget = MAX_WALK;
        for directory in directories.into_iter().chain(rescan) {
            walk(&directory, &mut derived, &mut budget);
        }
        // Events still go missing (FSEvents loses some in the stream's first
        // moments and coalesces others): the folders the run changed are
        // found by their modification times, and what they hold is admitted
        // by birth time like any other derived path.
        for root in &self.watched {
            changed(root, self.started, &self.storage, &mut derived, &mut budget);
        }
        // Parents sort before children; rollback replays in reverse, so
        // children are removed before their directories. Only effects that
        // still exist, are outside the storage root and were made during the
        // run are recorded.
        let paths: BTreeSet<_> = observed.union(&derived).cloned().collect();
        for path in paths {
            if path.starts_with(&self.storage) {
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !admitted(
                metadata.created().ok(),
                observed.contains(&path),
                self.started,
            ) {
                continue;
            }
            let entry = if metadata.is_dir() {
                JournalEntry::ExternalDirectory { path }
            } else {
                JournalEntry::ExternalFile {
                    path,
                    original: None,
                }
            };
            journal.record(entry)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::admitted;
    use std::time::{Duration, SystemTime};

    #[test]
    fn only_paths_made_during_the_run_are_admitted() {
        let started = SystemTime::now();
        let before = started - Duration::from_secs(1);
        let after = started + Duration::from_millis(1);
        // A birth time decides, even against a create event.
        assert!(admitted(Some(after), false, started));
        assert!(admitted(Some(started), true, started));
        assert!(!admitted(Some(before), true, started));
        assert!(!admitted(Some(before), false, started));
        // No birth time: the watcher's create event counts, an ancestor or a
        // folder walk doesn't.
        assert!(admitted(None, true, started));
        assert!(!admitted(None, false, started));
    }
}
