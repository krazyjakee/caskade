//! Write-ahead compensation for relocation. A caller holds both root leases.
use super::*;
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    source: PathBuf,
    destination: PathBuf,
    staging: PathBuf,
    copied: bool,
    committed: bool,
    entries: Vec<Undo>,
}
pub(super) struct UndoLog {
    pub entries: Vec<Undo>,
    durable: Option<(Dir, String, Record)>,
}
impl UndoLog {
    pub fn memory() -> Self {
        Self {
            entries: vec![],
            durable: None,
        }
    }
    pub fn create(
        request: &RelocateRequest,
        source: &Path,
        destination: &Path,
        copied: bool,
    ) -> io::Result<Self> {
        let (directory, name, staging) = paths(request)?;
        if directory.try_exists(&name)? {
            return Err(io::Error::other(
                "unfinished move journal; recover it before retrying",
            ));
        }
        let log = Self {
            entries: vec![],
            durable: Some((
                directory,
                name,
                Record {
                    schema_version: 1,
                    source: source.into(),
                    destination: destination.into(),
                    staging,
                    copied,
                    committed: false,
                    entries: vec![],
                },
            )),
        };
        log.persist()?;
        Ok(log)
    }
    pub fn staging(&self) -> Option<&Path> {
        self.durable
            .as_ref()
            .map(|(_, _, record)| record.staging.as_path())
    }
    pub fn record(&mut self, entry: Undo) -> io::Result<()> {
        self.entries.push(entry);
        self.persist()
    }
    fn persist(&self) -> io::Result<()> {
        let Some((dir, name, record)) = &self.durable else {
            return Ok(());
        };
        let record = Record {
            schema_version: record.schema_version,
            source: record.source.clone(),
            destination: record.destination.clone(),
            staging: record.staging.clone(),
            copied: record.copied,
            committed: record.committed,
            entries: self.entries.clone(),
        };
        let temporary = format!("{name}.tmp");
        match dir.remove_file(&temporary) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut file =
            dir.open_with(&temporary, OpenOptions::new().write(true).create_new(true))?;
        serde_json::to_writer(&mut file, &record)?;
        if file.metadata()?.len() > 128 * 1024 * 1024 {
            return Err(io::Error::other("move journal exceeds its recovery budget"));
        }
        file.sync_all()?;
        drop(file);
        dir.rename(&temporary, dir, name)?;
        #[cfg(unix)]
        dir.open(".")?.sync_all()?;
        Ok(())
    }
    pub fn commit(&mut self) -> io::Result<()> {
        if let Some((_, _, record)) = &mut self.durable {
            record.committed = true;
        }
        self.persist()
    }
    pub fn finish(&mut self) -> io::Result<()> {
        if let Some((dir, name, record)) = self.durable.take() {
            dir.remove_file(&name)?;
            #[cfg(unix)]
            dir.open(".")?.sync_all()?;
            // Windows can't remove `.caskade/moves` while this handle is open.
            drop(dir);
            if let Some(root) = record.destination.parent() {
                let parent = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
                // Only remove empty bookkeeping directories created by moves;
                // root.json, other jobs and existing metadata are retained.
                for path in [".caskade/moves", ".caskade/staging", ".caskade"] {
                    let _ = parent.remove_dir(path);
                }
            }
        }
        Ok(())
    }
    pub fn load(request: &RelocateRequest) -> io::Result<Option<Self>> {
        let root = Dir::open_ambient_dir(&request.target_root, cap_std::ambient_authority())?;
        match root.open_dir(".caskade/moves") {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
        let (dir, name, staging) = paths(request)?;
        let file = match dir.open(&name) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if !dir.symlink_metadata(&name)?.is_file() {
            return Err(io::Error::other("move journal is not a regular file"));
        }
        let mut bytes = vec![];
        file.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(io::Error::other("move journal exceeds its budget"));
        }
        let mut record: Record = serde_json::from_slice(&bytes)?;
        let expected_destination = dunce::canonicalize(&request.target_root)?.join(
            request
                .game_dir
                .file_name()
                .ok_or_else(|| io::Error::other("game has no name"))?,
        );
        if record.schema_version != 1
            || record.source != request.game_dir
            || record.destination != expected_destination
            || record.staging != staging
        {
            return Err(io::Error::other(
                "journal does not match the trusted move request",
            ));
        }
        let entries = std::mem::take(&mut record.entries);
        for entry in &entries {
            validate(entry, request, &record)?;
        }
        Ok(Some(Self {
            entries,
            durable: Some((dir, name, record)),
        }))
    }
    pub fn committed_copy(&self) -> Option<(&Path, &Path)> {
        self.durable.as_ref().and_then(|(_, _, r)| {
            (r.committed && r.copied).then_some((r.source.as_path(), r.destination.as_path()))
        })
    }
    pub fn committed(&self) -> bool {
        self.durable.as_ref().is_some_and(|(_, _, r)| r.committed)
    }
}
fn paths(request: &RelocateRequest) -> io::Result<(Dir, String, PathBuf)> {
    if request.job_id.is_empty()
        || request.job_id.len() > 128
        || !request
            .job_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(io::Error::other("invalid move job id"));
    }
    let root = dunce::canonicalize(&request.target_root)?;
    let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
    dir.create_dir_all(".caskade/moves")?;
    dir.create_dir_all(".caskade/staging")?;
    for path in [".caskade", ".caskade/moves", ".caskade/staging"] {
        if !dir.symlink_metadata(path)?.is_dir() {
            return Err(io::Error::other(
                "move metadata must contain only real directories",
            ));
        }
    }
    let directory = dir.open_dir(".caskade/moves")?;
    Ok((
        directory,
        format!("{}.json", request.job_id),
        root.join(".caskade/staging")
            .join(format!("move-{}", request.job_id)),
    ))
}
fn validate(entry: &Undo, request: &RelocateRequest, record: &Record) -> io::Result<()> {
    let confined = |path: &Path| {
        path.is_absolute()
            && !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            && (path.starts_with(&record.destination) || path.starts_with(&record.source))
    };
    let alias = |store: &AliasStore, id: &String| {
        request
            .alias
            .as_ref()
            .is_some_and(|(expected, key)| expected == store && key == id)
    };
    let allowed = match entry {
        Undo::Moved { from, to } => from == &record.destination && to == &record.source,
        Undo::Copied { to } => to == &record.destination || to == &record.staging,
        Undo::File { path, .. } => {
            confined(path)
                || request.references.iter().any(|r| {
                    matches!(&r.location, Location::ExternalFile { path: p, .. } if Path::new(p) == path)
                })
        }
        Undo::Registry(values) => values.iter().all(|value| {
            request.references.iter().any(|r| {
                matches!(&r.location, Location::Registry { hive, key, name, .. }
                    if hive == &value.hive && key == &value.key && name == &value.name)
            })
        }),
        Undo::Link { path, .. } => confined(path),
        Undo::Alias { store, id, target } => alias(store, id) && target == &record.source,
        Undo::AliasCreated { store, id } => alias(store, id),
    };
    if allowed {
        Ok(())
    } else {
        Err(io::Error::other(
            "journal undo exceeds the trusted move capabilities",
        ))
    }
}

impl Relocator {
    pub(super) fn rollback_log(&self, log: &mut UndoLog) -> Result<(), String> {
        // Persist progress after each idempotent undo. In particular, file
        // restores are retired before renaming the directory back, so a
        // second kill during recovery never replays them at a vanished path.
        while !log.entries.is_empty() {
            let index = log
                .entries
                .iter()
                .rposition(|step| !matches!(step, Undo::Alias { .. } | Undo::AliasCreated { .. }))
                .unwrap_or(log.entries.len() - 1);
            self.rollback(vec![log.entries[index].clone()])?;
            log.entries.remove(index);
            log.persist().map_err(|e| e.to_string())?;
        }
        log.finish().map_err(|e| e.to_string())
    }
    /// Recover one known job while holding both root leases. Requests must
    /// come from the app's durable job catalogue, never an imported directory.
    /// Uncommitted jobs roll back; committed jobs finish source cleanup.
    pub fn recover_move(&self, request: &RelocateRequest) -> Result<bool, RelocateError> {
        let Some(mut log) = UndoLog::load(request)? else {
            return Ok(false);
        };
        if log.committed() {
            if let Some((source, destination)) = log.committed_copy() {
                if !destination.is_dir() {
                    return Err(io::Error::other("committed move destination is missing").into());
                }
                if source.try_exists()? {
                    remove_tree(source)?;
                }
            }
            log.finish()?;
        } else {
            self.rollback_log(&mut log)
                .map_err(|error| RelocateError::RollbackFailed {
                    error: "interrupted move".into(),
                    rollback: error,
                })?;
        }
        Ok(true)
    }
}
