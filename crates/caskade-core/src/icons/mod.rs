//! Icons from games' own executables, for games with no icon: IGDB's comes
//! first (`crate::metadata`), and the library shows it over these
//! (`db::query`). An install's saved icon (`.caskade/icon.png`), the launch
//! executable's (a Windows `.exe`'s icon resource or a macOS `.app`'s), or,
//! for a game launched through its store, the main `.exe` or `.app` in its
//! install folder. What was looked at is remembered, so an unchanged game's
//! executables are read once.

use crate::{
    Result,
    artwork::ArtworkCache,
    bus::{Bus, Event},
    db::LibraryStore,
    model::{ArtworkKind, GameEntry, InstallLocation, LaunchSpec, StorageRoot},
};
use caskade_discovery::InstallState;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// Artwork source of an executable's icon.
pub const SOURCE: &str = "executable";
/// The most of one file read for its icon: game executables run large.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// How long events settle before a run.
const SETTLE: Duration = Duration::from_secs(5);
/// Store-launched games' executables that are never the game's.
const NOT_THE_GAME: [&str; 9] = [
    "unins", "setup", "install", "crash", "redist", "vcredist", "dxsetup", "prereq", "report",
];

#[derive(Clone)]
pub struct ExeIcons {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    artwork: ArtworkCache,
}

impl ExeIcons {
    pub fn new(store: LibraryStore, bus: Bus, artwork: ArtworkCache) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                artwork,
            }),
        }
    }

    /// Gives every game without an icon its executable's, where it has one.
    /// Returns how many got one.
    pub async fn run(&self) -> Result<usize> {
        let store = &self.inner.store;
        let roots = store.roots().await?;
        let mut added = 0;
        for game_id in store.games_without_icon().await? {
            let entries = store.entries_for_game(&game_id).await?;
            let candidates = candidates(&entries, &roots);
            let signature = signature(&candidates);
            if store.icon_check(&game_id).await?.as_deref() == Some(signature.as_str()) {
                continue;
            }
            if self.extract(&game_id, candidates).await? {
                added += 1;
                self.inner.bus.emit(Event::LibraryGameUpdated {
                    game_id: game_id.clone(),
                });
            }
            store.save_icon_check(&game_id, &signature).await?;
        }
        if added > 0 {
            tracing::info!(added, "executable icons added");
        }
        Ok(added)
    }

    /// The first candidate's icon that reads, as the game's icon artwork.
    async fn extract(&self, game_id: &str, candidates: Vec<PathBuf>) -> Result<bool> {
        for path in candidates {
            let read = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || {
                    caskade_installer::discover::executable_icon(&path, MAX_BYTES)
                })
                .await
            };
            let png = match read {
                Ok(Ok(Some(png))) => png,
                Ok(Ok(None)) => continue,
                Ok(Err(error)) => {
                    tracing::debug!(game = game_id, path = %path.display(), %error, "no executable icon");
                    continue;
                }
                Err(error) => {
                    tracing::warn!(game = game_id, %error, "executable icon read panicked");
                    continue;
                }
            };
            match self.inner.artwork.import_bytes(png).await {
                Ok(cached) => {
                    self.inner
                        .store
                        .upsert_artwork(&cached.record(game_id, ArtworkKind::Icon, SOURCE))
                        .await?;
                    return Ok(true);
                }
                Err(error) => {
                    tracing::warn!(game = game_id, %error, "executable icon not cached");
                }
            }
        }
        Ok(false)
    }

    /// Runs now, then after library scans, new games and installs.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let this = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            loop {
                if let Err(error) = this.run().await {
                    tracing::warn!(%error, "executable icons failed");
                }
                loop {
                    match events.recv().await {
                        Ok(event) if relevant(&event) => break,
                        Ok(_) | Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => return,
                    }
                }
                tokio::time::sleep(SETTLE).await;
                // One run covers every event that arrived meanwhile.
                loop {
                    match events.try_recv() {
                        Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => return,
                    }
                }
            }
        })
    }
}

/// Events that can bring a game an executable.
fn relevant(event: &Event) -> bool {
    matches!(
        event,
        Event::LibraryScanCompleted { .. }
            | Event::LibraryGameAdded { .. }
            | Event::InstallCompleted { .. }
    )
}

/// Where a game's installed entries may keep an icon, best first.
fn candidates(entries: &[GameEntry], roots: &[StorageRoot]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in entries {
        if entry.state != InstallState::Installed {
            continue;
        }
        let dir = match &entry.location {
            Some(InstallLocation::Managed { root_id, rel_dir }) => roots
                .iter()
                .find(|root| &root.id == root_id)
                .map(|root| root.path.join(rel_dir)),
            Some(InstallLocation::External { path }) => Some(path.clone()),
            None => None,
        };
        if let Some(dir) = &dir {
            paths.push(dir.join(".caskade/icon.png"));
        }
        match &entry.launch {
            Some(LaunchSpec::Executable { path, .. }) => {
                let path = match &dir {
                    Some(dir) if path.is_relative() => dir.join(path),
                    _ => path.clone(),
                };
                // A program inside a bundle has the bundle's icon.
                let bundle = path
                    .ancestors()
                    .find(|ancestor| has_extension(ancestor, "app"))
                    .map(Path::to_path_buf);
                paths.push(bundle.unwrap_or(path));
            }
            // Launched through its store or a runner: guess from the folder.
            _ => paths.extend(dir.as_deref().and_then(main_program)),
        }
    }
    paths.retain(|path| path.exists());
    paths.dedup();
    paths
}

/// An install folder's own `.app` bundle, or its largest `.exe` that isn't
/// an installer, uninstaller or crash reporter.
fn main_program(dir: &Path) -> Option<PathBuf> {
    let mut largest: Option<(u64, PathBuf)> = None;
    for item in std::fs::read_dir(dir).ok()?.flatten() {
        let path = item.path();
        if has_extension(&path, "app") && path.is_dir() {
            return Some(path);
        }
        let name = item.file_name().to_string_lossy().to_ascii_lowercase();
        if !has_extension(&path, "exe") || NOT_THE_GAME.iter().any(|word| name.contains(word)) {
            continue;
        }
        let Ok(meta) = item.metadata() else { continue };
        if meta.is_file() && largest.as_ref().is_none_or(|(size, _)| meta.len() > *size) {
            largest = Some((meta.len(), path));
        }
    }
    largest.map(|(_, path)| path)
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// The candidates with each one's size and change time: a new install, an
/// update or a move reads them again.
fn signature(candidates: &[PathBuf]) -> String {
    candidates
        .iter()
        .map(|path| {
            let meta = std::fs::metadata(path).ok();
            let modified = meta
                .as_ref()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_secs());
            let size = meta.map_or(0, |meta| meta.len());
            format!("{}|{size}|{modified}", path.display())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
