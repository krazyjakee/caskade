use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};

use crate::{RuntimeSpec, runtime::ProcessCommand};

use super::{
    ExecError, Result,
    api::{Runtimes, one_component},
    context::subdir,
};

/// Effects recorded by a runtime watcher or a Delegate step. Compensation is
/// replayed in reverse order on rollback, and retained in a successful report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEntry {
    /// Informational; a retained user-choice payload is never rollback cleanup.
    InstallLocation {
        path: PathBuf,
        reason: String,
    },
    ExternalFile {
        path: PathBuf,
        original: Option<Vec<u8>>,
    },
    ExternalDirectory {
        path: PathBuf,
    },
    Compensation {
        runtime: RuntimeSpec,
        command: ProcessCommand,
        /// Runs first; the command is skipped unless it exits 0.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        only_if: Option<ProcessCommand>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Record {
    schema_version: u32,
    pub job_id: String,
    pub relative_dir: PathBuf,
    pub final_owned: bool,
    pub completed: bool,
    pub entries: Vec<JournalEntry>,
}

/// Thread-safe durable journal supplied to native side-effect monitors.
#[derive(Clone)]
pub struct JournalHandle {
    state: Arc<Mutex<Record>>,
    directory: Arc<Dir>,
    name: PathBuf,
}

impl JournalHandle {
    // Private to the watcher: callers cannot use this to exempt arbitrary paths.
    pub(super) fn preserve_observed_folder(&self, path: &Path, reason: &str) -> Result<()> {
        let mut state = self.state.lock().expect("executor journal poisoned");
        if state.entries.iter().any(|entry| {
            matches!(entry,
            JournalEntry::ExternalFile { path: file, original: Some(_) } if file.starts_with(path))
        }) {
            return Err(ExecError::InvalidRequest {
                message: "observed install modified preexisting files".into(),
            });
        }
        state.entries.retain(|entry| match entry {
            JournalEntry::ExternalFile {
                path: file,
                original: None,
            }
            | JournalEntry::ExternalDirectory { path: file } => !file.starts_with(path),
            _ => true,
        });
        state.entries.push(JournalEntry::InstallLocation {
            path: path.into(),
            reason: reason.into(),
        });
        self.persist(&state)
    }

    pub fn record(&self, entry: JournalEntry) -> Result<()> {
        let mut state = self.state.lock().expect("executor journal poisoned");
        state.entries.push(entry);
        self.persist(&state)
    }
    pub fn entries(&self) -> Vec<JournalEntry> {
        self.state
            .lock()
            .expect("executor journal poisoned")
            .entries
            .clone()
    }
    fn persist(&self, state: &Record) -> Result<()> {
        let temporary = self.name.with_extension("tmp");
        // The job exclusively owns this temp name. Do not follow a preexisting
        // symbolic link, including one left by an interrupted native installer.
        remove(&self.directory, &temporary)?;
        let mut file = self
            .directory
            .open_with(&temporary, OpenOptions::new().write(true).create_new(true))?;
        serde_json::to_writer(&mut file, state)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        self.directory
            .rename(&temporary, &self.directory, &self.name)?;
        #[cfg(unix)]
        self.directory.open(".")?.sync_all()?;
        Ok(())
    }
    pub(super) fn set_final_owned(&self) -> Result<()> {
        let mut state = self.state.lock().expect("executor journal poisoned");
        state.final_owned = true;
        self.persist(&state)
    }
    pub(super) fn release_final(&self) -> Result<()> {
        let mut state = self.state.lock().expect("executor journal poisoned");
        state.final_owned = false;
        self.persist(&state)
    }
    pub(super) fn complete(&self) -> Result<()> {
        let mut state = self.state.lock().expect("executor journal poisoned");
        state.completed = true;
        self.persist(&state)
    }
    pub(super) fn snapshot(&self) -> Record {
        self.state
            .lock()
            .expect("executor journal poisoned")
            .clone()
    }
    pub(super) fn remove(&self) -> Result<()> {
        remove(&self.directory, &self.name)
    }
}

pub(super) fn prepare(root: &Dir, job_id: &str, relative_dir: PathBuf) -> Result<JournalHandle> {
    let directory = subdir(root, Path::new(".caskade/journals"), true)?;
    let name = PathBuf::from(format!("{job_id}.json"));
    if directory.try_exists(&name)? {
        return Err(ExecError::InvalidRequest {
            message: "job already has an unrecovered journal".into(),
        });
    }
    let journal = JournalHandle {
        state: Arc::new(Mutex::new(Record {
            schema_version: 1,
            job_id: job_id.into(),
            relative_dir,
            final_owned: false,
            completed: false,
            entries: vec![],
        })),
        directory: Arc::new(directory),
        name,
    };
    journal.persist(&journal.snapshot())?;
    Ok(journal)
}

pub(super) fn runtime<'a>(
    runtimes: &'a Runtimes,
    spec: &RuntimeSpec,
) -> Result<&'a Arc<dyn crate::runtime::Runtime>> {
    runtimes
        .iter()
        .find(|(candidate, _)| candidate == spec)
        .map(|(_, runtime)| runtime)
        .ok_or_else(|| ExecError::MissingRuntime {
            runtime: spec.clone(),
        })
}

pub(super) fn rollback(root: &Dir, record: &Record, runtimes: &Runtimes) -> Result<()> {
    let mut errors = vec![];
    for entry in record.entries.iter().rev() {
        let result = match entry {
            JournalEntry::InstallLocation { .. } => Ok(()),
            JournalEntry::ExternalFile { path, original } => {
                restore_file(path, original.as_deref())
            }
            JournalEntry::ExternalDirectory { path }
                if record.entries.iter().any(|entry| {
                    matches!(entry,
                JournalEntry::InstallLocation { path: kept, .. } if kept.starts_with(path))
                }) =>
            {
                Ok(())
            }
            JournalEntry::ExternalDirectory { path } => match fs::remove_dir(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result.map_err(Into::into),
            },
            JournalEntry::Compensation {
                runtime: spec,
                command,
                only_if,
            } => (|| {
                let runtime = runtime(runtimes, spec)?;
                if only_if
                    .as_ref()
                    .is_some_and(|check| !needed(&**runtime, check))
                {
                    return Ok(());
                }
                let id = runtime.spawn(command)?;
                let result = runtime.wait(id, Duration::from_secs(300));
                let killed = runtime.kill(id);
                let exit = result?;
                killed?;
                if exit.code != 0 {
                    return Err(ExecError::ProcessFailed { code: exit.code });
                }
                Ok(())
            })(),
        };
        if let Err(error) = result {
            errors.push(error.to_string());
        }
    }
    if record.final_owned
        && let Err(error) = remove(root, &record.relative_dir)
    {
        errors.push(error.to_string());
    }
    let stage = Path::new(".caskade/staging").join(&record.job_id);
    if let Err(error) = remove(root, &stage) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ExecError::Rollback {
            message: errors.join("; "),
        })
    }
}

/// Whether a delegate command still has something to do: its check exits 0,
/// or the check can't run, and then the command runs and reports for itself.
pub(super) fn needed(runtime: &dyn crate::runtime::Runtime, check: &ProcessCommand) -> bool {
    let exit = runtime.spawn(check).and_then(|id| {
        let exit = runtime.wait(id, Duration::from_secs(30));
        let _ = runtime.kill(id);
        exit
    });
    match exit {
        Ok(exit) => exit.code == 0,
        Err(error) => {
            tracing::warn!(%error, program = %check.program.display(), "delegate check failed");
            true
        }
    }
}

fn restore_file(path: &Path, original: Option<&[u8]>) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => fs::remove_file(path)?,
        Ok(metadata) if !metadata.is_file() => {
            return Err(ExecError::Rollback {
                message: format!("external file replaced by non-file: {}", path.display()),
            });
        }
        Ok(_) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    if let Some(contents) = original {
        fs::write(path, contents)?;
    } else {
        match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            result => result?,
        }
    }
    Ok(())
}

pub(super) fn remove(root: &Dir, path: &Path) -> Result<()> {
    match root.symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => root.remove_dir_all(path)?,
        Ok(_) => root.remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn prune(root: &Dir) {
    for path in [".caskade/staging", ".caskade/journals", ".caskade"] {
        if root
            .symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
        {
            let _ = root.remove_dir(path);
        }
    }
}

/// Rolls back every unfinished journal in the root. A journal whose rollback
/// fails stays for the next recovery, and the others still go: one install
/// whose undo can't run must not hold up the root.
pub(super) fn recover(root: &Dir, runtimes: &Runtimes) -> Result<usize> {
    let mut count = 0;
    let mut failed = vec![];
    if root.try_exists(".caskade/journals")? {
        let journals = subdir(root, Path::new(".caskade/journals"), false)?;
        for entry in journals.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            if entry.file_type()?.is_symlink() {
                return Err(ExecError::InvalidRequest {
                    message: "symlink in journal directory".into(),
                });
            }
            if Path::new(&name).extension().is_some_and(|ext| ext == "tmp") {
                journals.remove_file(&name)?;
                continue;
            }
            if Path::new(&name).extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let record: Record = serde_json::from_reader(journals.open(&name)?)?;
            one_component(Path::new(&record.job_id))?;
            one_component(&record.relative_dir)?;
            if record.schema_version != 1
                || name != std::ffi::OsStr::new(&format!("{}.json", record.job_id))
            {
                return Err(ExecError::InvalidRequest {
                    message: "invalid recovery journal header".into(),
                });
            }
            if !record.completed
                && let Err(error) = rollback(root, &record, runtimes)
            {
                let error = match error {
                    ExecError::Rollback { message } => message,
                    error => error.to_string(),
                };
                failed.push(format!("{}: {error}", record.relative_dir.display()));
                continue;
            }
            journals.remove_file(name)?;
            count += 1;
        }
    }
    if root.try_exists(".caskade/staging")? {
        let staging = subdir(root, Path::new(".caskade/staging"), false)?;
        for entry in staging.entries()? {
            remove(&staging, Path::new(&entry?.file_name()))?;
            count += 1;
        }
    }
    prune(root);
    if failed.is_empty() {
        Ok(count)
    } else {
        Err(ExecError::Rollback {
            message: format!("interrupted install not undone: {}", failed.join("; ")),
        })
    }
}

#[cfg(test)]
mod interactive_recovery_tests {
    use super::*;
    use crate::testing::TestRoot;
    #[test]
    fn retained_payload_survives_startup_recovery_while_unrelated_effects_are_reverted() {
        let root = TestRoot::new().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let retained = outside.path().join("parent/Game");
        fs::create_dir_all(&retained).unwrap();
        let payload = retained.join("game.exe");
        fs::write(&payload, b"game").unwrap();
        let unrelated = outside.path().join("unrelated");
        fs::write(&unrelated, b"temporary").unwrap();
        let prior = outside.path().join("prior");
        fs::write(&prior, b"modified").unwrap();
        let cap = crate::safety::open_staging(root.path()).unwrap();
        let journal = prepare(&cap, "interactive-recovery", "Game".into()).unwrap();
        for entry in [
            JournalEntry::ExternalDirectory {
                path: outside.path().join("parent"),
            },
            JournalEntry::ExternalDirectory {
                path: retained.clone(),
            },
            JournalEntry::ExternalFile {
                path: payload.clone(),
                original: None,
            },
            JournalEntry::ExternalFile {
                path: unrelated.clone(),
                original: None,
            },
            JournalEntry::ExternalFile {
                path: prior.clone(),
                original: Some(b"original".to_vec()),
            },
        ] {
            journal.record(entry).unwrap();
        }
        journal
            .preserve_observed_folder(&retained, "matched expected files")
            .unwrap();
        drop(journal);
        assert_eq!(recover(&cap, &vec![]).unwrap(), 1);
        assert_eq!(fs::read(payload).unwrap(), b"game");
        assert!(!unrelated.exists());
        assert_eq!(fs::read(prior).unwrap(), b"original");
        root.assert_clean();
    }
    #[test]
    fn an_undo_that_fails_keeps_its_journal_without_holding_up_the_others() {
        use crate::testing::{FakeAction, FakeRuntime, FakeScript};
        let root = TestRoot::new().unwrap();
        let cap = crate::safety::open_staging(root.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let leftover = outside.path().join("leftover");
        fs::write(&leftover, b"temporary").unwrap();
        let removal = ProcessCommand {
            program: "apt".into(),
            args: vec!["remove".into(), "game".into()],
            env: Default::default(),
            cwd: root.path().into(),
        };
        let stuck = prepare(&cap, "a-stuck", "Stuck Game".into()).unwrap();
        stuck
            .record(JournalEntry::Compensation {
                runtime: RuntimeSpec::Native,
                command: removal.clone(),
                only_if: Some(ProcessCommand {
                    program: "dpkg-query".into(),
                    args: vec!["--status".into(), "game".into()],
                    ..removal.clone()
                }),
            })
            .unwrap();
        drop(stuck);
        let other = prepare(&cap, "b-other", "Other Game".into()).unwrap();
        other
            .record(JournalEntry::ExternalFile {
                path: leftover.clone(),
                original: None,
            })
            .unwrap();
        drop(other);
        // The package is installed and its removal fails (apt's 100).
        let exits = |codes: &[i32]| -> Runtimes {
            vec![(
                RuntimeSpec::Native,
                Arc::new(FakeRuntime::new(codes.iter().map(|&code| FakeScript {
                    actions: vec![FakeAction::Exit { code }],
                }))),
            )]
        };
        let error = recover(&cap, &exits(&[0, 100])).unwrap_err().to_string();
        assert!(error.contains("Stuck Game"), "{error}");
        assert!(!leftover.exists());
        assert!(cap.exists(".caskade/journals/a-stuck.json"));
        assert!(!cap.exists(".caskade/journals/b-other.json"));
        // Removed by hand since: nothing to undo, so the journal goes.
        assert_eq!(recover(&cap, &exits(&[1])).unwrap(), 1);
        root.assert_clean();
    }
    #[test]
    fn preservation_never_discards_a_preexisting_file_inverse() {
        let root = TestRoot::new().unwrap();
        let cap = crate::safety::open_staging(root.path()).unwrap();
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("game.exe");
        fs::write(&path, b"modified").unwrap();
        let journal = prepare(&cap, "interactive-original", "Game".into()).unwrap();
        journal
            .record(JournalEntry::ExternalFile {
                path: path.clone(),
                original: Some(b"original".to_vec()),
            })
            .unwrap();
        assert!(
            journal
                .preserve_observed_folder(folder.path(), "unsafe claim")
                .is_err()
        );
        assert_eq!(journal.entries().len(), 1);
        drop(journal);
        recover(&cap, &vec![]).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"original");
        root.assert_clean();
    }
}
