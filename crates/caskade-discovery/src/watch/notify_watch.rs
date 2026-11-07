//! The `notify` watcher: one background thread owns the OS watcher, the
//! debouncer and the [`Library`], and calls back with each non-empty delta.

use super::{Debouncer, Library, WatchOptions};
use crate::{
    Error,
    env::HostEnv,
    orchestrator::{Config, builtin},
    types::{Cancel, ScanDelta, Source, SourceId},
};
use notify::{EventKind, RecursiveMode};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Missing watch paths are covered by their nearest existing parent, at
/// most this many levels up.
const MAX_ANCESTORS: usize = 4;

enum Message {
    Events(Vec<PathBuf>),
    Stop,
}

/// A running watch; dropping it stops the thread.
pub struct Watcher {
    sender: SyncSender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Stops watching and waits for an in-flight rescan to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.sender.send(Message::Stop);
            let _ = thread.join();
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Scans every built-in source once (the first delta holds every game),
/// then rescans a source after its files change and calls `on_delta` with
/// each non-empty delta, on a background thread.
pub fn watch<F>(env: HostEnv, config: Config, on_delta: F) -> crate::Result<Watcher>
where
    F: FnMut(ScanDelta) + Send + 'static,
{
    watch_with_sources(env, config, WatchOptions::default(), builtin(), on_delta)
}

/// [`watch`] over the given sources with explicit options.
pub fn watch_with_sources<F>(
    env: HostEnv,
    config: Config,
    options: WatchOptions,
    sources: Vec<Arc<dyn Source>>,
    mut on_delta: F,
) -> crate::Result<Watcher>
where
    F: FnMut(ScanDelta) + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(options.queue.max(1));
    let overflow = Arc::new(AtomicBool::new(false));
    let events = sender.clone();
    let flag = overflow.clone();
    let mut os =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
            Ok(event) => {
                if events.try_send(Message::Events(event.paths)).is_err() {
                    flag.store(true, Ordering::Release);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "file watcher error");
                flag.store(true, Ordering::Release);
            }
        })
        .map_err(|error| Error::Watch(error.to_string()))?;

    let mut library = Library::with_sources(env, config, sources);
    let initial =
        caskade_jobs::global().sync("Scanning library", || library.scan(&Cancel::new()))?;
    if !initial.is_empty() {
        on_delta(initial);
    }
    let mut registrations = Registrations::default();
    registrations.update(&mut os, plan(&library, &options));

    let thread = std::thread::Builder::new()
        .name("discovery-watch".into())
        .spawn(move || {
            let started = Instant::now();
            let now = || started.elapsed();
            let mut debouncer = Debouncer::new(options.debounce, options.max_delay);
            let mut next_poll = now() + options.poll_interval;
            loop {
                let mut wait = next_poll.saturating_sub(now()).min(Duration::from_secs(1));
                if let Some(due) = debouncer.next_due() {
                    wait = wait.min(due.saturating_sub(now()));
                }
                match receiver.recv_timeout(wait) {
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Message::Events(paths)) => {
                        let sources = registrations.sources_for(&paths);
                        tracing::trace!(?paths, ?sources, "watched paths changed");
                        for source in sources {
                            debouncer.event(source, now());
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if overflow.swap(false, Ordering::AcqRel) {
                    for source in library.sources() {
                        debouncer.event(source.id(), now());
                    }
                }
                if now() >= next_poll {
                    next_poll = now() + options.poll_interval;
                    let polled = registrations.unwatched(&library);
                    tracing::trace!(?polled, "polling sources without watch paths");
                    for source in polled {
                        debouncer.event(source, now());
                    }
                }
                let due = debouncer.due(now());
                if due.is_empty() {
                    continue;
                }
                let rescan = Instant::now();
                let result = caskade_jobs::global()
                    .sync("Updating library", || library.rescan(&due, &Cancel::new()));
                tracing::debug!(sources = ?due, took = ?rescan.elapsed(), "rescanned");
                match result {
                    Ok(delta) if !delta.is_empty() => on_delta(delta),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "incremental rescan failed"),
                }
                // Paths that appeared since the last plan may have missed
                // events: rescan their sources once more.
                for source in registrations.update(&mut os, plan(&library, &options)) {
                    debouncer.event(source, now());
                }
            }
        })
        .map_err(|error| Error::Watch(error.to_string()))?;
    Ok(Watcher {
        sender,
        thread: Some(thread),
    })
}

/// Registered path → (recursive, sources it belongs to).
type Plan = BTreeMap<PathBuf, (bool, BTreeSet<SourceId>)>;

/// Where to watch for every source that runs on this host.
fn plan(library: &Library, options: &WatchOptions) -> Plan {
    let env = library.env();
    let mut plan = Plan::new();
    let mut add = |path: PathBuf, recursive: bool, source: &SourceId| {
        let entry = plan.entry(path).or_insert((false, BTreeSet::new()));
        entry.0 |= recursive;
        entry.1.insert(source.clone());
    };
    for source in library.sources() {
        if !source.hosts().contains(env.os()) {
            continue;
        }
        let id = source.id();
        for watch in source.watch_paths(env) {
            if watch.path.exists() {
                add(watch.path, watch.recursive, &id);
            } else if let Some(parent) = existing_ancestor(&watch.path) {
                add(parent, false, &id);
            }
        }
        for location in source.launcher_locations(env) {
            if let Some(path) =
                existing_ancestor(&location).or_else(|| location.exists().then_some(location))
            {
                add(path, false, &id);
            }
        }
    }
    // Deep-scan folders come from the config, which `Source::watch_paths`
    // does not see.
    #[cfg(feature = "deep")]
    if library
        .sources()
        .iter()
        .any(|source| source.id() == crate::deep::ID)
    {
        for watch in crate::deep::watch_paths(env, library.config()) {
            if watch.path.exists() {
                add(watch.path, watch.recursive, &crate::deep::ID);
            } else if let Some(parent) = existing_ancestor(&watch.path) {
                add(parent, false, &crate::deep::ID);
            }
        }
    }
    if plan.len() > options.max_paths {
        tracing::warn!(
            paths = plan.len(),
            max = options.max_paths,
            "too many watch paths; polling the rest"
        );
        plan = plan.into_iter().take(options.max_paths).collect();
    }
    plan
}

/// The path itself if it exists, else its nearest existing parent.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .take(MAX_ANCESTORS + 1)
        .find(|path| path.is_dir() || path.is_file())
        .map(Path::to_path_buf)
}

#[derive(Default)]
struct Registrations {
    current: Plan,
}

impl Registrations {
    /// Applies `plan` to the OS watcher; returns the sources that gained
    /// paths.
    fn update(&mut self, os: &mut impl notify::Watcher, plan: Plan) -> BTreeSet<SourceId> {
        let mut gained = BTreeSet::new();
        for (path, (recursive, _)) in &self.current {
            if plan.get(path).is_none_or(|(mode, _)| mode != recursive) {
                let _ = os.unwatch(path);
            }
        }
        let mut registered = Plan::new();
        for (path, (recursive, sources)) in plan {
            let unchanged = self
                .current
                .get(&path)
                .is_some_and(|(mode, _)| *mode == recursive);
            if !unchanged {
                let mode = if recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                if let Err(error) = os.watch(&path, mode) {
                    tracing::debug!(path = %path.display(), %error, "cannot watch path");
                    continue;
                }
                gained.extend(sources.iter().cloned());
            } else if let Some((_, old)) = self.current.get(&path) {
                gained.extend(sources.difference(old).cloned());
            }
            registered.insert(path, (recursive, sources));
        }
        self.current = registered;
        gained
    }

    /// Sources that an event on any of `paths` concerns.
    fn sources_for(&self, paths: &[PathBuf]) -> BTreeSet<SourceId> {
        let mut sources = BTreeSet::new();
        for path in paths {
            for (registered, (recursive, owners)) in &self.current {
                let hit = path == registered
                    || path.parent() == Some(registered.as_path())
                    || (*recursive && path.starts_with(registered));
                if hit {
                    sources.extend(owners.iter().cloned());
                }
            }
        }
        sources
    }

    /// Sources with no registered path: polled.
    fn unwatched(&self, library: &Library) -> BTreeSet<SourceId> {
        let watched: BTreeSet<_> = self
            .current
            .values()
            .flat_map(|(_, sources)| sources)
            .collect();
        library
            .sources()
            .iter()
            .map(|source| source.id())
            .filter(|id| !watched.contains(id))
            .collect()
    }
}
