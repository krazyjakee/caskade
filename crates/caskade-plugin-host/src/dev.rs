//! Dev mode (#131): a plugin loaded in place from its
//! folder, watched for changes. After a short quiet period:
//! - `plugin.json` changed: the manifest is validated again, grants follow
//!   it, and the hooks and the UI half reload;
//! - the backend changed: it is recompiled and restarted, and its hooks
//!   re-attach (the old instance's subscriptions are gone);
//! - anything else (the UI module, its CSS, assets): the UI half reloads.
//!
//! Every reload emits [`Event::PluginReloaded`]; a broken rebuild carries its
//! error there and the app keeps running.
//!
//! Build output and dependencies (`target/`, `node_modules/`, hidden folders)
//! present at the start are not watched, and changes in them never reload:
//! they hold thousands of folders, and a build rewrites them.

use crate::PluginHost;
use caskade_core::bus::{Bus, Event};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

/// How long a folder must be quiet before a reload: editors and bundlers
/// write several files per save.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// A watched dev plugin; dropping it stops watching.
pub struct DevWatch {
    folder: PathBuf,
    _watches: Arc<Mutex<Watches>>,
    task: tokio::task::JoinHandle<()>,
}

impl DevWatch {
    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

impl Drop for DevWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Loads `folder` as a dev plugin and reloads it on every change. Must be
/// called inside a tokio runtime.
pub async fn watch(
    host: PluginHost,
    bus: Bus,
    folder: &Path,
    debounce: Duration,
) -> Result<DevWatch, crate::HostError> {
    let folder = folder.canonicalize()?;
    host.add_dev_folder(&folder).await?;
    let (sender, mut changes) = mpsc::unbounded_channel::<(PathBuf, bool)>();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && !matches!(event.kind, notify::EventKind::Access(_))
        {
            let created = matches!(event.kind, notify::EventKind::Create(_));
            for path in event.paths {
                let _ = sender.send((path, created));
            }
        }
    })
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut watches = Watches {
        watcher,
        alone: BTreeSet::new(),
    };
    watches
        .add(&folder)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let watches = Arc::new(Mutex::new(watches));
    let (watched, adding) = (folder.clone(), watches.clone());
    let task = tokio::spawn(async move {
        while let Some(first) = changes.recv().await {
            let mut events = vec![first];
            // Gather the rest of this save.
            while let Ok(Some(event)) = tokio::time::timeout(debounce, changes.recv()).await {
                events.push(event);
            }
            let mut paths = BTreeSet::new();
            for (path, created) in events {
                let relative = path.strip_prefix(&watched).unwrap_or(&path);
                if relative.iter().any(|part| unwatched(Path::new(part))) {
                    continue;
                }
                // A folder made in one watched on its own, like a first
                // `dist/`, isn't watched yet.
                if created && path.is_dir() {
                    let mut watches = caskade_core::lock(&adding);
                    if path
                        .parent()
                        .is_some_and(|parent| watches.alone.contains(parent))
                        && let Err(error) = watches.add(&path)
                    {
                        tracing::warn!(path = %path.display(), %error, "dev plugin folder not watched");
                    }
                }
                paths.insert(path);
            }
            if !paths.is_empty() {
                reload(&host, &bus, &watched, &paths).await;
            }
        }
    });
    tracing::info!(folder = %folder.display(), "watching dev plugin");
    Ok(DevWatch {
        folder,
        _watches: watches,
        task,
    })
}

/// A folder a dev plugin's build or package manager fills.
fn unwatched(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') || name == "target" || name == "node_modules")
}

/// What a dev plugin's folder is watched with.
struct Watches {
    watcher: notify::RecommendedWatcher,
    /// Folders watched without what's inside them.
    alone: BTreeSet<PathBuf>,
}

impl Watches {
    /// Watches `dir` but no [`unwatched`] folder below it: all of `dir`
    /// when it holds none, else `dir` on its own (which covers its files)
    /// and each folder in it the same way.
    fn add(&mut self, dir: &Path) -> notify::Result<()> {
        let mut watches = Vec::new();
        if !plan(dir, &mut watches) {
            watches.push((dir.to_owned(), RecursiveMode::Recursive));
        }
        for (path, mode) in watches {
            self.watcher.watch(&path, mode)?;
            if mode == RecursiveMode::NonRecursive {
                self.alone.insert(path);
            }
        }
        Ok(())
    }
}

/// Adds the watches [`Watches::add`] needs for `dir` to `watches`, if it
/// holds an [`unwatched`] folder; false (adding none) if it doesn't.
fn plan(dir: &Path, watches: &mut Vec<(PathBuf, RecursiveMode)>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut split = false;
    let mut inside = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if unwatched(&path) || plan(&path, &mut inside) {
            split = true;
        } else {
            inside.push((path, RecursiveMode::Recursive));
        }
    }
    if split {
        watches.push((dir.to_owned(), RecursiveMode::NonRecursive));
        watches.extend(inside);
    }
    split
}

async fn reload(host: &PluginHost, bus: &Bus, folder: &Path, paths: &BTreeSet<PathBuf>) {
    let changed = |relative: &Path| paths.iter().any(|path| path == &folder.join(relative));
    let before = host.dev_plugin(folder);
    let manifest_changed = changed(Path::new(crate::manifest::MANIFEST));
    let (id, ui, error) = if manifest_changed || before.is_none() {
        // Validate again; grants follow the dev manifest.
        match host.discover().await {
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "dev plugin discovery failed"),
        }
        let summary = host
            .list()
            .await
            .into_iter()
            .find(|plugin| plugin.dev && Path::new(&plugin.folder) == folder);
        match summary {
            Some(summary) => {
                let error = (!summary.errors.is_empty()).then(|| {
                    summary
                        .errors
                        .iter()
                        .map(|error| format!("{}: {}", error.key, error.message))
                        .collect::<Vec<_>>()
                        .join("; ")
                });
                let error = match (error, summary.status) {
                    (None, crate::PluginStatus::Enabled | crate::PluginStatus::Active) => host
                        .reload_backend(&summary.id)
                        .await
                        .err()
                        .map(|e| e.to_string()),
                    (error, _) => error,
                };
                (summary.id, true, error)
            }
            None => return,
        }
    } else {
        let Some(id) = before else {
            return;
        };
        let backend = host
            .manifest(&id)
            .and_then(|manifest| manifest.backend)
            .is_some_and(|backend| changed(&backend));
        if backend {
            let error = host.reload_backend(&id).await.err().map(|e| e.to_string());
            (id, false, error)
        } else {
            (id, true, None)
        }
    };
    match &error {
        Some(error) => tracing::warn!(plugin = %id, %error, "dev plugin reload failed"),
        None => tracing::info!(plugin = %id, ui, "dev plugin reloaded"),
    }
    // Hooks re-attach to the new backend and manifest (#125).
    bus.emit(Event::PluginsChanged { plugin: id.clone() });
    bus.emit(Event::PluginReloaded {
        plugin: id,
        ui,
        error,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_and_dependencies_are_left_unwatched() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for dir in [
            "dist",
            "backend/src",
            "backend/target/debug",
            "node_modules/pkg",
            ".git",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("plugin.json"), "{}").unwrap();
        let mut watches = Vec::new();
        assert!(plan(root, &mut watches));
        watches.sort();
        let relative: Vec<_> = watches
            .iter()
            .map(|(path, mode)| (path.strip_prefix(root).unwrap().to_owned(), *mode))
            .collect();
        assert_eq!(
            relative,
            [
                (PathBuf::new(), RecursiveMode::NonRecursive),
                (PathBuf::from("backend"), RecursiveMode::NonRecursive),
                (PathBuf::from("backend/src"), RecursiveMode::Recursive),
                (PathBuf::from("dist"), RecursiveMode::Recursive),
            ]
        );
        let plain = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(plain.path().join("dist")).unwrap();
        assert!(!plan(plain.path(), &mut Vec::new()), "watched whole");
    }
}
