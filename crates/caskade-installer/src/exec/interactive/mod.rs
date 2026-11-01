//! Observe interactive installers without driving their user interface.
//! Only the requested game tree is accepted automatically. Other new folders
//! require an explicit decision, with expected files checked through capabilities.
use super::{
    ExecError, ExecRequest, JournalHandle, Result, StepContext,
    context::{relative, subdir},
};
use crate::{HandlerId, Question, RuntimeSpec, Step, runtime::ProcessCommand, safety};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct InteractiveConfig {
    /// Trusted handler-relative payload expectations; otherwise inherited from
    /// silent Run steps of the same handler/program/runtime in this plan set.
    pub expect: BTreeMap<HandlerId, Vec<PathBuf>>,
    /// Explicit native observation scopes, never authorization to move/delete.
    pub outside_roots: Vec<PathBuf>,
    pub settle: Duration,
    pub max_entries: usize,
}
impl Default for InteractiveConfig {
    fn default() -> Self {
        Self {
            expect: BTreeMap::new(),
            outside_roots: vec![],
            settle: Duration::from_millis(100),
            max_entries: 100_000,
        }
    }
}
#[derive(Default)]
struct Events {
    paths: BTreeSet<PathBuf>,
    overflow: bool,
    failed: bool,
}
pub(super) struct Observation {
    _watcher: RecommendedWatcher,
    events: Arc<Mutex<Events>>,
    before: BTreeSet<PathBuf>,
    scopes: Vec<PathBuf>,
    expect: Vec<PathBuf>,
    config: InteractiveConfig,
    prefix: Option<PathBuf>,
}
fn invalid(message: &str) -> ExecError {
    ExecError::InvalidRequest {
        message: message.into(),
    }
}
fn scan(path: &Path, seen: &mut BTreeSet<PathBuf>, limit: usize, depth: usize) -> Result<()> {
    if depth > 64 || seen.len() >= limit {
        return Err(invalid("interactive observation inventory limit"));
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    seen.insert(path.into());
    if metadata.is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            scan(&entry?.path(), seen, limit, depth + 1)?;
        }
    }
    Ok(())
}
fn confined(scope: &Path, path: &Path) -> Result<cap_std::fs::Dir> {
    let root = safety::open_staging(scope)?;
    let rel = path
        .strip_prefix(scope)
        .map_err(|_| invalid("observed folder outside configured scope"))?;
    subdir(&root, rel, false)
}
fn matches(dir: &cap_std::fs::Dir, expect: &[PathBuf]) -> Result<bool> {
    for path in expect {
        let path = relative(path)?;
        let Ok(parent) = subdir(dir, path.parent().unwrap_or(Path::new(".")), false) else {
            return Ok(false);
        };
        let Some(name) = path.file_name() else {
            return Ok(false);
        };
        let Ok(meta) = parent.symlink_metadata(name) else {
            return Ok(false);
        };
        if !meta.is_file() || meta.is_symlink() {
            return Ok(false);
        }
    }
    Ok(true)
}
impl Observation {
    pub(super) fn start(
        request: &ExecRequest,
        config: &InteractiveConfig,
        context: &StepContext,
        command: &ProcessCommand,
        program: &crate::Program,
        runtime: &RuntimeSpec,
    ) -> Result<Self> {
        if config.max_entries == 0 || config.settle > Duration::from_secs(5) {
            return Err(invalid("invalid interactive observation limits"));
        }
        let mut expect = context
            .handler_id()
            .and_then(|id| config.expect.get(id))
            .cloned()
            .unwrap_or_default();
        if expect.is_empty() {
            for candidate in &request.plans.candidates {
                if Some(&candidate.handler) != context.handler_id() {
                    continue;
                }
                for step in &candidate.steps {
                    if let Step::Run {
                        program: p,
                        runtime: r,
                        expect: paths,
                        ..
                    } = step
                        && p == program
                        && r == runtime
                    {
                        expect.extend(paths.iter().cloned());
                    }
                }
            }
        }
        expect.sort();
        expect.dedup();
        for path in &expect {
            relative(path)?;
        }
        let prefix = (*runtime != RuntimeSpec::Native)
            .then(|| crate::runtime::proton::PrefixRuntime::prefix_for(command).join("drive_c"));
        let mut scopes = vec![request.root.clone()];
        scopes.extend(config.outside_roots.iter().cloned());
        if let Some(prefix) = &prefix
            && !prefix.starts_with(&request.root)
        {
            scopes.push(prefix.clone());
        }
        for scope in &mut scopes {
            if !scope.is_absolute() {
                return Err(invalid("interactive observation scopes must be absolute"));
            }
            // Canonical spelling also normalizes Windows extended path prefixes.
            // This is an injected read-only observation scope, not write authority.
            if scope.exists() {
                *scope = std::fs::canonicalize(&*scope)?;
            }
        }
        scopes.sort();
        scopes.dedup();
        let mut before = BTreeSet::new();
        for scope in &scopes {
            if !scope.is_absolute() {
                return Err(invalid("interactive observation scopes must be absolute"));
            }
            scan(scope, &mut before, config.max_entries, 0)?;
        }
        let events = Arc::new(Mutex::new(Events::default()));
        let shared = events.clone();
        let limit = config.max_entries;
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let mut events = shared.lock().expect("interactive watcher poisoned");
                match event {
                    Ok(event) => {
                        for path in event.paths {
                            if events.paths.len() >= limit {
                                events.overflow = true;
                                break;
                            }
                            events.paths.insert(path);
                        }
                    }
                    Err(_) => events.failed = true,
                }
            })
            .map_err(|e| invalid(&format!("interactive watcher: {e}")))?;
        let mut watched = BTreeSet::new();
        for scope in &scopes {
            let mut watch = scope.as_path();
            while !watch.exists() {
                watch = watch
                    .parent()
                    .ok_or_else(|| invalid("no existing watch ancestor"))?;
            }
            if watched.insert(watch.to_owned()) {
                watcher
                    .watch(watch, RecursiveMode::Recursive)
                    .map_err(|e| invalid(&format!("interactive watch scope: {e}")))?;
            }
        }
        Ok(Self {
            _watcher: watcher,
            events,
            before,
            scopes,
            expect,
            config: config.clone(),
            prefix,
        })
    }
    pub(super) fn finish(
        self,
        request: &ExecRequest,
        context: &StepContext,
        journal: &JournalHandle,
    ) -> Result<()> {
        // Native watchers may coalesce or deliver late. A bounded final inventory
        // complements notify events; no result depends solely on notification timing.
        std::thread::sleep(self.config.settle);
        context.checkpoint(0)?;
        let events = self.events.lock().expect("interactive watcher poisoned");
        if events.overflow || events.failed {
            return Err(invalid("interactive watcher lost events"));
        }
        let mut after = BTreeSet::new();
        for scope in &self.scopes {
            scan(scope, &mut after, self.config.max_entries, 0)?;
        }
        let paths: BTreeSet<_> = after
            .difference(&self.before)
            .cloned()
            .chain(events.paths.iter().cloned())
            .collect();
        drop(events);
        let game = &context.working_dir;
        let target_ok = if self.expect.is_empty() {
            paths.iter().any(|p| {
                p.strip_prefix(game).ok().is_some_and(|rel| {
                    !rel.starts_with(".caskade")
                        && matches(&context.dir, &[rel.to_owned()]).unwrap_or(false)
                })
            })
        } else {
            matches(&context.dir, &self.expect)?
        };
        if target_ok {
            return Ok(());
        }
        let mut found = BTreeSet::new();
        for path in &paths {
            let mut candidate = path.as_path();
            while let Some(parent) = candidate.parent() {
                let in_prefix = self
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| candidate.starts_with(prefix));
                if (!candidate.starts_with(game) || in_prefix)
                    && (!candidate.starts_with(request.root.join(".caskade")) || in_prefix)
                    && !self.before.contains(candidate)
                    && !self.scopes.iter().any(|scope| scope == candidate)
                    && let Some(scope) = self
                        .scopes
                        .iter()
                        .filter(|scope| candidate.starts_with(scope))
                        .max_by_key(|scope| scope.components().count())
                    && let Ok(dir) = confined(scope, candidate)
                    && !self.expect.is_empty()
                    && matches(&dir, &self.expect)?
                {
                    found.insert(candidate.to_owned());
                }
                candidate = parent;
                if !self.scopes.iter().any(|scope| candidate.starts_with(scope)) {
                    break;
                }
            }
        }
        // Freshness and full inventory are part of the proof. No file modified
        // in a preexisting folder is ever converted into a preserved new install.
        let mut preserved = Vec::new();
        for folder in found {
            if self.before.iter().any(|old| old.starts_with(&folder)) {
                continue;
            }
            let scope = self
                .scopes
                .iter()
                .filter(|scope| folder.starts_with(scope))
                .max_by_key(|scope| scope.components().count())
                .unwrap();
            let dir = confined(scope, &folder)?;
            let mut tree = BTreeSet::new();
            scan(&folder, &mut tree, self.config.max_entries, 0)?;
            if tree.iter().any(|path| path.is_symlink()) {
                continue;
            }
            // Prefix payloads belong to staging: preserve them in a confined
            // recovery folder before rollback removes that private prefix.
            let retained = if folder.starts_with(game) {
                let root = safety::open_staging(&request.root)?;
                let to = PathBuf::from(".caskade/interactive")
                    .join(&request.job_id)
                    .join(format!("payload-{}", preserved.len()));
                subdir(&root, to.parent().unwrap(), true)?;
                if root.try_exists(&to)? {
                    return Err(safety::SafetyError::Collision(to).into());
                }
                let from = folder
                    .strip_prefix(&request.root)
                    .map_err(|_| invalid("prefix recovery outside request root"))?;
                subdir(&root, from.parent().unwrap(), false)?;
                root.rename(from, &root, &to)?;
                request.root.join(to)
            } else {
                folder.clone()
            };
            drop(dir);
            journal.preserve_observed_folder(
                &retained,
                "interactive expected payload outside requested game folder",
            )?;
            preserved.push(retained);
        }
        let reason = if self.expect.is_empty() {
            "Installer produced no identifiable payload in the requested folder; choose its installed folder."
        } else {
            "Expected payload was not installed in the requested folder; observed matching folders are retained for an explicit choice."
        };
        Err(ExecError::NeedsUser {
            question: Question::ChooseInstallDirectory {
                paths: preserved,
                reason: reason.into(),
            },
        })
    }
}
