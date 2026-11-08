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

use crate::PluginHost;
use caskade_core::bus::{Bus, Event};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::mpsc;

/// How long a folder must be quiet before a reload: editors and bundlers
/// write several files per save.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// A watched dev plugin; dropping it stops watching.
pub struct DevWatch {
    folder: PathBuf,
    _watcher: notify::RecommendedWatcher,
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
    let (sender, mut changes) = mpsc::unbounded_channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && !matches!(event.kind, notify::EventKind::Access(_))
        {
            for path in event.paths {
                let _ = sender.send(path);
            }
        }
    })
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    watcher
        .watch(&folder, RecursiveMode::Recursive)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let watched = folder.clone();
    let task = tokio::spawn(async move {
        while let Some(first) = changes.recv().await {
            let mut paths = BTreeSet::from([first]);
            // Gather the rest of this save.
            while let Ok(Some(path)) = tokio::time::timeout(debounce, changes.recv()).await {
                paths.insert(path);
            }
            reload(&host, &bus, &watched, &paths).await;
        }
    });
    tracing::info!(folder = %folder.display(), "watching dev plugin");
    Ok(DevWatch {
        folder,
        _watcher: watcher,
        task,
    })
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
